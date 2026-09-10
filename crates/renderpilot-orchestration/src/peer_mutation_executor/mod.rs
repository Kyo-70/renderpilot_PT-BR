//! Cross-crate peer mutation owner.
//!
//! Context owns this executor rather than a naked SQLite handle.  The
//! executor keeps the peer runtime private while exposing only the repository
//! delegation needed by existing catalog/read paths.

mod aggregate_membership;
pub(crate) mod ancestor_io;
mod ancestors;
mod observation;
mod program;
#[cfg(test)]
mod read_guard_tests;
mod read_guards;
mod recovery;
#[cfg(test)]
pub(crate) mod timing_tests;

pub(crate) use ancestor_io::PeerAncestorAuthorities;
pub(crate) use ancestors::{PeerAncestorManifestEntry, PeerAncestorPlan};
pub(crate) use observation::{
    PeerPathObservation, PeerPathSnapshot, observe_peer_path as observe_peer_path_state,
    observe_peer_path_snapshot,
};
pub(crate) use program::{
    EndpointEvidence, EndpointExpectation, EndpointObservation, EndpointPostcondition,
    ExactEndpoint, ExactEndpointProgram, PeerExecutionClass, PeerProgramEnvelope, PeerRouteError,
    VerifiedPeerFile, build_peer_program, render_peer_root, render_peer_roots,
};
pub(crate) use read_guards::{observe_peer_path, observe_peer_read_guards};
use renderpilot_application::OptiScalerStateRepository;
use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, PeerEndpointEvidence, PeerEndpointIntent,
    PeerEndpointOperation, PeerFileImage, PlannedGameProxyTopology,
};
use renderpilot_storage_sqlite::{
    AggregateBefore, GameAggregateMutation, MetadataAggregatePreparation,
    MetadataAggregateTransition, PeerCommitPreparation, PeerStorageRuntime, PlannedAggregateAfter,
    PreparedPeerCommitPermit, SqliteStorage,
};

use crate::addons::engine::apply::{PeerMutationPreflight, apply_peer_mutation};
use crate::addons::engine::{InstallChanges, PeerMutationPlan};
use crate::addons::peer_lifecycle::PeerMutationPackage;
use crate::file_mutation::DurableFileTransaction;
use crate::game_mutation_lock::GameMutationGuard;
use crate::{Context, ServiceError};

/// Move-only ceremony result. The reservation, exact plan, native O1
/// preflight, and storage permit cannot be split by an adapter.
pub(crate) struct PreparedPeerFileMutation<'a> {
    executor: &'a PeerMutationExecutor,
    transaction: DurableFileTransaction,
    permit: PreparedPeerCommitPermit,
    package: PeerMutationPackage<'a>,
}

/// Inputs that cross the public route-selection boundary into ordinary peer
/// preparation. The route is a closed choice, so a caller cannot accidentally
/// enable incompatible specialized projections on the same durable permit.
struct OrdinaryPeerPreparationInput<'a, 'b> {
    context: &'a Context,
    guard: &'b GameMutationGuard,
    feature: &'b str,
    subject_id: Option<&'b str>,
    package: PeerMutationPackage<'a>,
}

impl<'a> PreparedPeerFileMutation<'a> {
    /// Consumes the Prepared state and returns an Applied state containing the
    /// only evidence that may cross into storage. A failed apply leaves the
    /// durable row Prepared for recovery.
    pub(crate) fn apply(
        self,
        changes: &mut InstallChanges,
    ) -> Result<AppliedPeerFileMutation<'a>, ServiceError> {
        let Self {
            executor,
            transaction,
            permit,
            package,
        } = self;
        transaction.verify_before_apply()?;
        observe_peer_read_guards(&package)?;
        let ancestor_authorities = transaction.apply_peer_ancestors(changes)?;
        let evidence = apply_peer_mutation(
            package.plan(),
            package.preflight(),
            &ancestor_authorities,
            changes,
        )
        .and_then(|evidence| domain_evidence(package.plan(), evidence))?;
        package
            .contract()
            .validate_evidence(&evidence)
            .map_err(|error| crate::failed(format!("peer evidence rejected: {error}")))?;
        Ok(AppliedPeerFileMutation {
            executor,
            transaction,
            permit,
            package,
            evidence,
        })
    }
}

/// Move-only post-apply state. Evidence is private and cannot be replaced by
/// an adapter between the filesystem apply and the SQLite CAS commit.
pub(crate) struct AppliedPeerFileMutation<'a> {
    executor: &'a PeerMutationExecutor,
    transaction: DurableFileTransaction,
    permit: PreparedPeerCommitPermit,
    package: PeerMutationPackage<'a>,
    evidence: Vec<PeerEndpointEvidence>,
}

impl AppliedPeerFileMutation<'_> {
    /// Consumes the permit exactly once. Storage commits the aggregate and
    /// filesystem row in one CAS transaction; any error retains Prepared.
    pub(crate) fn commit(self) -> Result<(), ServiceError> {
        let Self {
            executor,
            transaction,
            permit,
            package,
            evidence,
        } = self;
        let read_guards = observe_peer_read_guards(&package)?;
        executor.commit_ordinary_peer(permit, evidence, read_guards)?;
        if let Err(error) = transaction.cleanup_committed(executor.repositories()) {
            log::warn!("peer transaction committed but cleanup is pending: {error}");
        }
        Ok(())
    }
}

fn domain_evidence(
    plan: &PeerMutationPlan,
    evidence: Vec<EndpointEvidence>,
) -> Result<Vec<PeerEndpointEvidence>, ServiceError> {
    if evidence.len() != plan.program().endpoints().len() {
        return Err(crate::failed(
            "peer evidence cardinality changed during apply",
        ));
    }
    evidence
        .into_iter()
        .zip(plan.program().endpoints())
        .zip(plan.payloads())
        .map(|((observed, endpoint), payload)| {
            let before = domain_image(observed.before())?;
            let after = domain_image(observed.after())?;
            let operation = match (&before, &after) {
                (None, Some(_)) => PeerEndpointOperation::Create,
                (Some(_), Some(_)) => PeerEndpointOperation::Replace,
                (Some(_), None) => PeerEndpointOperation::Remove,
                (None, None) => {
                    return Err(crate::failed(format!(
                        "peer endpoint became a no-op: {}",
                        observed.path().as_str()
                    )));
                }
            };
            let (planned_digest, planned_length) = match &after {
                Some(image) => (
                    Some(image.sha256().clone()),
                    Some(
                        payload
                            .as_ref()
                            .map_or(image.length(), |bytes| bytes.len() as u64),
                    ),
                ),
                None => (None, None),
            };
            let intent = PeerEndpointIntent::new(
                observed.path().clone(),
                endpoint.role(),
                operation,
                planned_digest,
                planned_length,
            )
            .map_err(|error| crate::failed(error.to_string()))?;
            Ok(PeerEndpointEvidence::new(intent, before, after))
        })
        .collect()
}

fn domain_image(observation: &EndpointObservation) -> Result<Option<PeerFileImage>, ServiceError> {
    match observation {
        EndpointObservation::Absent => Ok(None),
        EndpointObservation::File(file) => PeerFileImage::new(
            file.identity().to_owned(),
            file.digest().clone(),
            file.length(),
        )
        .map(Some)
        .map_err(|error| crate::failed(error.to_string())),
    }
}

/// Lowers the retained native O1 observations once, with the same exact
/// cardinality and endpoint/preimage checks used by apply and evidence.
///
/// The package carries this immutable projection into the domain catalog
/// derivation; it must never be reconstructed from a second filesystem read.
pub(crate) fn domain_preimages(
    plan: &PeerMutationPlan,
    preflight: &PeerMutationPreflight,
) -> Result<Vec<Option<PeerFileImage>>, ServiceError> {
    if !preflight.matches_plan(plan) {
        return Err(crate::failed(
            "peer O1 preflight differs from its immutable endpoint plan",
        ));
    }
    let expected = plan.program().endpoints().len();
    if preflight.before().len() != expected {
        return Err(crate::failed(
            "peer O1 preflight cardinality changed during package planning",
        ));
    }
    preflight.before().iter().map(domain_image).collect()
}

#[derive(Debug)]
pub(crate) struct PeerMutationExecutor {
    runtime: PeerStorageRuntime,
}

impl PeerMutationExecutor {
    pub(crate) fn new(storage: SqliteStorage) -> Self {
        Self {
            runtime: PeerStorageRuntime::new(storage),
        }
    }

    pub(crate) fn repositories(&self) -> &SqliteStorage {
        self.runtime.repositories()
    }

    /// Builds the complete metadata aggregate image from one authenticated
    /// operation snapshot. Storage repeats the aggregate read while opening
    /// its reservation; orchestration never performs a second topology/state
    /// read between the image and the durable CAS boundary.
    fn build_metadata_aggregate_mutation(
        &self,
        guard: &GameMutationGuard,
        before_peer: &InstalledAddon,
        after_peer: &InstalledAddon,
        topology: Option<&GameProxyTopology>,
    ) -> Result<GameAggregateMutation, ServiceError> {
        if guard.game_id() != before_peer.game_id()
            || guard.game_id() != after_peer.game_id()
            || topology.is_some_and(|value| guard.game_id() != &value.game_id)
        {
            return Err(ServiceError::invalid_input(
                "peer metadata images must belong to the guarded game",
            ));
        }

        let state = self
            .repositories()
            .get_optiscaler_install_state(guard.game_id())?;
        let before = AggregateBefore::new(
            guard.game_id().clone(),
            state.clone(),
            topology.cloned(),
            Some(before_peer.clone()),
        )?;
        let planned_after = PlannedAggregateAfter::new(
            guard.game_id().clone(),
            state,
            topology.map(|value| PlannedGameProxyTopology::Exact(value.clone())),
            Some(after_peer.clone()),
        )?;
        GameAggregateMutation::new(ulid::Ulid::generate().to_string(), before, planned_after)
            .map_err(Into::into)
    }

    /// Commits metadata for an existing peer under the caller's game guard.
    ///
    /// Both topology-present and topology-absent peers use the same durable
    /// metadata aggregate CAS. A committed row is cleaned up opportunistically;
    /// if cleanup cannot complete, the durable committed fence remains for
    /// recovery while the successful database mutation stays authoritative.
    pub(crate) fn commit_metadata_aggregate(
        &self,
        guard: &GameMutationGuard,
        before_peer: &InstalledAddon,
        after_peer: &InstalledAddon,
        topology: Option<&GameProxyTopology>,
    ) -> Result<(), ServiceError> {
        let mutation =
            self.build_metadata_aggregate_mutation(guard, before_peer, after_peer, topology)?;
        let permit = self
            .runtime
            .prepare_metadata_aggregate(MetadataAggregatePreparation::new(
                mutation,
                MetadataAggregateTransition::PeerMetadataRefresh,
            ))?;
        let committed = self.runtime.commit_metadata_aggregate(permit, Vec::new())?;
        if let Err(error) = self.runtime.cleanup_metadata_aggregate(committed) {
            log::warn!("peer metadata aggregate committed but cleanup is pending: {error}");
        }
        Ok(())
    }

    fn finish_file_peer_preparation(
        &self,
        preparation: PeerCommitPreparation<'_>,
    ) -> renderpilot_application::AppResult<PreparedPeerCommitPermit> {
        self.runtime.finish_file_peer_preparation(preparation)
    }

    /// Reserves and seals one ordinary file-peer route before any live write.
    /// The returned aggregate is the only adapter-facing continuation.
    pub(crate) fn prepare_ordinary_file_peer<'a>(
        &'a self,
        context: &'a Context,
        guard: &GameMutationGuard,
        feature: &str,
        subject_id: Option<&str>,
        package: PeerMutationPackage<'a>,
    ) -> Result<PreparedPeerFileMutation<'a>, ServiceError> {
        self.prepare_ordinary_file_peer_inner(OrdinaryPeerPreparationInput {
            context,
            guard,
            feature,
            subject_id,
            package,
        })
    }

    fn prepare_ordinary_file_peer_inner<'a>(
        &'a self,
        input: OrdinaryPeerPreparationInput<'a, '_>,
    ) -> Result<PreparedPeerFileMutation<'a>, ServiceError> {
        let OrdinaryPeerPreparationInput {
            context,
            guard,
            feature,
            subject_id,
            package,
        } = input;
        let initial_read_guards = observe_peer_read_guards(&package)?;
        let transaction = DurableFileTransaction::prepare_peer_mutation(
            context, guard, feature, subject_id, &package,
        )?;
        if let Err(error) = transaction.verify_before_apply() {
            return match transaction.abandon_preparing(context) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(crate::failed(format!(
                    "{error}; abandoning peer preparation failed: {cleanup}"
                ))),
            };
        }
        if let Err(error) = renderpilot_storage_sqlite::validate_file_peer_program_manifest(
            transaction.manifest_json(),
        ) {
            return match transaction.abandon_preparing(context) {
                Ok(()) => Err(error.into()),
                Err(cleanup) => Err(crate::failed(format!(
                    "{error}; abandoning peer preparation failed: {cleanup}"
                ))),
            };
        }
        let preparation = PeerCommitPreparation {
            mutation_id: transaction.id(),
            game_id: guard.game_id(),
            feature,
            subject_id,
            manifest_json: transaction.manifest_json(),
            canonical_game_root: package.canonical_game_root(),
            initial_read_guards: &initial_read_guards,
            before_peer: package.before_peer(),
            after_peer: package.after_peer(),
            before_topology: Some(package.before_topology()),
            planned_after_topology: Some(package.planned_after_topology()),
            route: package.route(),
            component_set: package.component_set(),
            baseline_mutations: package.baseline_mutations(),
            catalog_claim: package.catalog_claim(),
            renodx_reshade_ini: None,
        };
        let permit_result = self.finish_file_peer_preparation(preparation);
        let permit = match permit_result {
            Ok(permit) => permit,
            Err(error) => {
                let error: ServiceError = error.into();
                return transaction.abandon_preparing(context).and(Err(error));
            }
        };
        Ok(PreparedPeerFileMutation {
            executor: self,
            transaction,
            permit,
            package,
        })
    }

    fn commit_ordinary_peer(
        &self,
        permit: PreparedPeerCommitPermit,
        evidence: Vec<PeerEndpointEvidence>,
        read_guards: Vec<renderpilot_domain::PeerReadGuardEvidence>,
    ) -> renderpilot_application::AppResult<()> {
        self.runtime
            .seal_and_commit_ordinary_peer(permit, evidence, read_guards)
    }
}
