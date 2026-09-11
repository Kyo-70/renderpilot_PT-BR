//! Closed durable adapter for RenoDX shared-Vulkan mutations.
//!
//! This is the only bridge from a prepared RenoDX game projection to the
//! shared-Vulkan transaction. It validates an immutable install or update
//! matrix and then publishes both participants through one durable commit.

use std::path::{Path, PathBuf};

use renderpilot_domain::{
    GameId, GameProxyTopology, InstalledAddon, RenoDxReshadeIniAuthority, SharedArtifactRecord,
};
use renderpilot_platform_windows::vulkan_layer::{LayerRegistry, SharedVulkanLayerPlan};
use renderpilot_storage_sqlite::SharedArtifactMutation;

use crate::addons::renodx::platform::vulkan::shared_artifact::downloaded_record;
use crate::addons::reshade::fetch::Download;
use crate::addons::reshade::source::ReshadeSource;
use crate::addons::shared_vulkan_mutation::{
    CatalogProjection, MutationIdentity, PeerUnchangedRequest, PhysicalParticipants, Request,
    ScopeSpec, TrustedRoots,
};
use crate::{Context, ServiceError};

#[cfg(test)]
mod tests;
mod validation;

/// Source metadata for a newly downloaded shared ReShade layer.
pub(crate) type SharedLayerSource<'a> = (&'a ReshadeSource, &'a Download);

/// All immutable inputs needed for one active RenoDX shared mutation.
pub(crate) struct ActiveSharedMutationRequest<'a> {
    pub(crate) context: &'a Context,
    pub(crate) feature: &'a str,
    pub(crate) game_id: &'a GameId,
    pub(crate) game_root: &'a Path,
    pub(crate) topology: &'a GameProxyTopology,
    pub(crate) before_record: Option<&'a InstalledAddon>,
    pub(crate) after_record: InstalledAddon,
    pub(crate) game_intents: Vec<crate::addons::shared_vulkan_mutation::FileIntent>,
    pub(crate) shared_plan: SharedVulkanLayerPlan,
    pub(crate) reshade_ini_authority: Option<&'a RenoDxReshadeIniAuthority>,
    pub(crate) layer_dir: &'a Path,
    pub(crate) source: Option<SharedLayerSource<'a>>,
    pub(crate) shared_record: Option<&'a SharedArtifactRecord>,
    pub(crate) registry: &'a dyn LayerRegistry,
}

struct ActiveSharedMutationValidation<'a> {
    feature: &'a str,
    game_id: &'a GameId,
    game_root: &'a Path,
    topology: &'a GameProxyTopology,
    before_record: Option<&'a InstalledAddon>,
    after_record: &'a InstalledAddon,
    game_intents: &'a [crate::addons::shared_vulkan_mutation::FileIntent],
    shared_plan: &'a SharedVulkanLayerPlan,
    reshade_ini_authority: Option<&'a RenoDxReshadeIniAuthority>,
    layer_dir: &'a Path,
    source: Option<SharedLayerSource<'a>>,
    shared_record: Option<&'a SharedArtifactRecord>,
}

impl ActiveSharedMutationRequest<'_> {
    fn validation(&self) -> ActiveSharedMutationValidation<'_> {
        ActiveSharedMutationValidation {
            feature: self.feature,
            game_id: self.game_id,
            game_root: self.game_root,
            topology: self.topology,
            before_record: self.before_record,
            after_record: &self.after_record,
            game_intents: &self.game_intents,
            shared_plan: &self.shared_plan,
            reshade_ini_authority: self.reshade_ini_authority,
            layer_dir: self.layer_dir,
            source: self.source,
            shared_record: self.shared_record,
        }
    }
}

/// Closed error surface for the active RenoDX shared mutation adapter.
#[derive(Debug)]
pub(crate) enum ActiveSharedMutationError {
    InvalidInput(&'static str),
    InvalidPath(PathBuf),
    Artifact(ServiceError),
    Transaction(ServiceError),
}

impl std::fmt::Display for ActiveSharedMutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => {
                write!(formatter, "invalid RenoDX shared mutation: {reason}")
            }
            Self::InvalidPath(path) => write!(
                formatter,
                "invalid RenoDX shared-mutation path: {}",
                path.display()
            ),
            Self::Artifact(error) | Self::Transaction(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ActiveSharedMutationError {}

impl From<ServiceError> for ActiveSharedMutationError {
    fn from(error: ServiceError) -> Self {
        Self::Transaction(error)
    }
}

/// Publishes one active RenoDX install or update and its shared-layer
/// registration as one durable peer transaction. No filesystem or storage
/// rereads occur after the validated projections enter this adapter.
pub(crate) fn execute_active_shared_mutation(
    request: ActiveSharedMutationRequest<'_>,
) -> Result<InstalledAddon, ActiveSharedMutationError> {
    validation::validate_inputs(&request.validation())?;

    let ActiveSharedMutationRequest {
        context,
        feature,
        game_id,
        game_root,
        topology,
        before_record,
        after_record,
        game_intents,
        shared_plan,
        reshade_ini_authority,
        layer_dir,
        source,
        shared_record,
        registry,
    } = request;

    let game_scope = crate::file_mutation::MutationScope::single(game_root)
        .map_err(ActiveSharedMutationError::Transaction)?;
    let roots = TrustedRoots::game_shared(&game_scope, layer_dir)
        .map_err(|error| ActiveSharedMutationError::Transaction(error.into()))?;
    let mut composed = crate::addons::shared_vulkan_mutation::compose(None, Some(shared_plan))
        .map_err(ActiveSharedMutationError::Transaction)?;
    composed
        .prepend_files(game_intents)
        .map_err(ActiveSharedMutationError::Transaction)?;
    let downloaded_shared_record = source
        .map(|(source, download)| {
            downloaded_record(layer_dir, source, download)
                .map_err(ActiveSharedMutationError::Artifact)
        })
        .transpose()?;
    let shared_artifact = match (downloaded_shared_record.as_ref(), shared_record) {
        (Some(record), None) | (None, Some(record)) => SharedArtifactMutation::Upsert(record),
        (None, None) => SharedArtifactMutation::Keep,
        (Some(_), Some(_)) => {
            return Err(ActiveSharedMutationError::InvalidInput(
                "shared install cannot carry a prepared update record",
            ));
        }
    };

    let mutation_id = ulid::Ulid::generate().to_string();
    let identity = MutationIdentity::new(
        &mutation_id,
        ScopeSpec::game_upsert(game_id, &after_record),
        feature,
    );
    let physical = PhysicalParticipants::new(roots, composed, Some(registry));
    let projection = CatalogProjection::new(shared_artifact);
    let request = Request::new(context, identity, physical, projection);

    match reshade_ini_authority {
        Some(authority) => crate::addons::shared_vulkan_mutation::execute_peer_unchanged(
            PeerUnchangedRequest::new_with_renodx_reshade_ini(
                request,
                game_id,
                before_record,
                Some(&after_record),
                Some(topology),
                Some(topology),
                authority,
            ),
        ),
        None => crate::addons::shared_vulkan_mutation::execute_peer_unchanged(
            PeerUnchangedRequest::new(
                request,
                game_id,
                before_record,
                Some(&after_record),
                Some(topology),
                Some(topology),
            ),
        ),
    }
    .map_err(ActiveSharedMutationError::Transaction)?;

    Ok(after_record)
}
