use renderpilot_application::InstalledAddonRepository;
use renderpilot_domain::{
    AddonKind, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, FileReceipt,
    GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime, InstalledAddon,
    Launcher, LibraryComponent, LibraryTechnology, PathRef, PeerCatalogDeletedBaseline,
    PeerCatalogPhysicalContract, PeerCatalogRollbackClaim, PeerEndpointEvidence,
    PeerEndpointIntent, PeerEndpointRole, PeerFileImage, PeerReadGuardEvidence,
    PeerReadGuardExpectation, PlannedGameProxyTopology, Platform, ProxyImplementation, ProxyLink,
    ProxyPeerRoute, ProxyRootPrestate, Sha256Hash, Swappability, required_read_guards_with_catalog,
};

use crate::repositories::ComponentBaselineMutation;
use crate::{BeginFileMutationPreparation, PeerCommitPreparation, SqliteStorage};

pub(super) const ACTIVE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const ORIGINAL: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const STABLE: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const EXTRA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

pub(super) struct FullFixture {
    pub(super) runtime: super::super::permit::PeerStorageRuntime,
    pub(super) game_id: GameId,
    pub(super) before_peer: InstalledAddon,
    pub(super) peer: InstalledAddon,
    pub(super) topology: GameProxyTopology,
    pub(super) planned_topology: PlannedGameProxyTopology,
    pub(super) claim: PeerCatalogRollbackClaim,
    pub(super) changed_id: ComponentId,
    pub(super) stable_id: ComponentId,
    pub(super) intents: Vec<PeerEndpointIntent>,
    pub(super) before_images: Vec<Option<PeerFileImage>>,
    pub(super) read_guards: Vec<PeerReadGuardEvidence>,
    pub(super) manifest_json: String,
    pub(super) mutation_id: String,
}

impl FullFixture {
    pub(super) fn new(mutation_id: &str) -> Self {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new(format!("game:catalog-runtime-{mutation_id}")).expect("game id");
        let game = GameInstallation::new(
            GameIdentity::new(game_id.clone(), "Catalog Runtime", Launcher::Steam)
                .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new("C:/game").expect("root"),
        );
        let changed_id = ComponentId::new("component:catalog-changed").expect("component id");
        let stable_id = ComponentId::new("component:catalog-stable").expect("component id");
        let changed_path = PathRef::new("C:/game/catalog.dll").expect("path");
        let stable_path = PathRef::new("C:/game/stable.dll").expect("path");
        let changed = LibraryComponent::new(
            changed_id.clone(),
            game_id.clone(),
            ComponentKind::NativeLibrary,
            LibraryTechnology::DlssSuperResolution,
            Swappability::Swappable,
        )
        .with_file(
            ComponentFile::new(changed_path.clone())
                .with_sha256(Sha256Hash::new(ACTIVE).expect("active digest")),
        );
        let stable = LibraryComponent::new(
            stable_id.clone(),
            game_id.clone(),
            ComponentKind::NativeLibrary,
            LibraryTechnology::DlssSuperResolution,
            Swappability::Swappable,
        )
        .with_file(
            ComponentFile::new(stable_path.clone())
                .with_sha256(Sha256Hash::new(STABLE).expect("stable digest")),
        );
        let changed_baseline = ComponentRollbackBaseline::new(vec![
            ComponentFile::new(changed_path)
                .with_sha256(Sha256Hash::new(ORIGINAL).expect("original digest")),
        ]);
        let stable_baseline = ComponentRollbackBaseline::new(vec![
            ComponentFile::new(stable_path)
                .with_sha256(Sha256Hash::new(STABLE).expect("stable baseline digest")),
        ]);
        let components = vec![changed, stable];
        storage
            .save_scan_result(&game, &components, &[])
            .expect("component projection");
        storage
            .recover_component_rollback_baseline(&game_id, &changed_id, &changed_baseline)
            .expect("changed baseline");
        storage
            .recover_component_rollback_baseline(&game_id, &stable_id, &stable_baseline)
            .expect("stable baseline");
        let claim = PeerCatalogRollbackClaim::new(
            components,
            vec![
                PeerCatalogDeletedBaseline::new(changed_id.clone(), changed_baseline),
                PeerCatalogDeletedBaseline::new(stable_id.clone(), stable_baseline),
            ],
        )
        .expect("claim");

        let peer = InstalledAddon::new(
            game_id.clone(),
            AddonKind::Luma,
            PathRef::new("C:/game/luma.addon64").expect("addon path"),
        );
        storage
            .upsert_installed_addon(&peer)
            .expect("peer projection");
        let before_peer = storage
            .get_installed_addon(&game_id)
            .expect("read peer")
            .expect("peer exists");
        let peer = before_peer
            .clone()
            .with_created_file(PathRef::new("C:/game/extra.dll").expect("extra peer path"));

        let topology = GameProxyTopology {
            id: "topology:catalog-runtime".to_owned(),
            game_id: game_id.clone(),
            root_slot: PathRef::new("C:/game/outer.dll").expect("outer path"),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: PathRef::new("C:/game/outer.dll").expect("outer path"),
                receipt: FileReceipt::owned(
                    "outer-id",
                    Sha256Hash::new(ACTIVE).expect("outer digest"),
                )
                .expect("outer receipt"),
            },
            downstream: Some(ProxyLink {
                implementation: ProxyImplementation::ReShade,
                path: PathRef::new("C:/game/downstream.dll").expect("downstream path"),
                receipt: FileReceipt::owned(
                    "downstream-id",
                    Sha256Hash::new(ORIGINAL).expect("downstream digest"),
                )
                .expect("downstream receipt"),
            }),
            downstream_origin: Some(PathRef::new("C:/game/outer.dll").expect("origin path")),
            root_prestate: ProxyRootPrestate::Absent,
        };
        storage
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO game_proxy_topologies
                         (id, game_id, topology_json, created_at, updated_at)
                         VALUES (?1, ?2, ?3, 1, 1)",
                        rusqlite::params![
                            topology.id,
                            game_id.as_str(),
                            serde_json::to_string(&topology).expect("topology json"),
                        ],
                    )
                    .map_err(crate::error::storage_error)?;
                Ok(())
            })
            .expect("topology projection");

        let intents = vec![
            PeerEndpointIntent::remove(
                PathRef::new("C:/game/catalog.dll.bak").expect("sidecar path"),
                PeerEndpointRole::Disjoint,
            )
            .expect("remove intent"),
            PeerEndpointIntent::replace(
                PathRef::new("C:/game/catalog.dll").expect("catalog path"),
                PeerEndpointRole::Disjoint,
                Some(Sha256Hash::new(ORIGINAL).expect("planned digest")),
                Some(8),
            )
            .expect("replace intent"),
            PeerEndpointIntent::create(
                PathRef::new("C:/game/extra.dll").expect("extra path"),
                PeerEndpointRole::Disjoint,
                Some(Sha256Hash::new(EXTRA).expect("extra digest")),
                Some(8),
            )
            .expect("create intent"),
        ];
        let before_images = vec![
            Some(
                PeerFileImage::new(
                    "catalog-sidecar",
                    Sha256Hash::new(ORIGINAL).expect("sidecar digest"),
                    8,
                )
                .expect("sidecar image"),
            ),
            Some(
                PeerFileImage::new(
                    "catalog-live",
                    Sha256Hash::new(ACTIVE).expect("live digest"),
                    8,
                )
                .expect("live image"),
            ),
            None,
        ];
        let physical_catalog = PeerCatalogPhysicalContract::derive(
            &claim,
            Some(&before_peer),
            Some(&peer),
            &intents,
            &before_images,
        )
        .expect("catalog physical contract");
        let planned = PlannedGameProxyTopology::Exact(topology.clone());
        let requirements = required_read_guards_with_catalog(
            Some(&before_peer),
            Some(&peer),
            Some(&topology),
            Some(&planned),
            ProxyPeerRoute::DurableDisjoint,
            &intents,
            Some(&physical_catalog),
        )
        .expect("catalog-aware read guards");
        let read_guards = guard_evidence(&requirements, "catalog-runtime");
        let manifest_json = ordinary_manifest(&intents, &before_images, mutation_id);
        Self {
            runtime: super::super::permit::PeerStorageRuntime::new(storage),
            game_id,
            before_peer,
            peer,
            topology,
            planned_topology: planned,
            claim,
            changed_id,
            stable_id,
            intents,
            before_images,
            read_guards,
            manifest_json,
            mutation_id: mutation_id.to_owned(),
        }
    }

    pub(super) fn prepare_with_guards(
        &self,
        initial_read_guards: &[PeerReadGuardEvidence],
    ) -> renderpilot_application::AppResult<super::super::permit::PreparedPeerCommitPermit> {
        self.runtime
            .repositories()
            .begin_file_mutation_preparation(&BeginFileMutationPreparation {
                id: self.mutation_id.clone(),
                game_id: self.game_id.clone(),
                feature: "luma_install".to_owned(),
                subject_id: None,
                initial_manifest_json: "{}".to_owned(),
            })?;
        let baseline_mutations = self
            .claim
            .deleted_baselines()
            .iter()
            .map(|entry| ComponentBaselineMutation::Delete {
                component_id: entry.component_id(),
            })
            .collect::<Vec<_>>();
        self.runtime
            .finish_file_peer_preparation(PeerCommitPreparation {
                mutation_id: &self.mutation_id,
                game_id: &self.game_id,
                feature: "luma_install",
                subject_id: None,
                manifest_json: &self.manifest_json,
                canonical_game_root: "C:/game",
                initial_read_guards,
                before_peer: Some(&self.before_peer),
                after_peer: Some(&self.peer),
                before_topology: Some(&self.topology),
                planned_after_topology: Some(&self.planned_topology),
                route: ProxyPeerRoute::DurableDisjoint,
                component_set: Some(self.claim.after_components()),
                baseline_mutations: &baseline_mutations,
                catalog_claim: Some(&self.claim),
                renodx_reshade_ini: None,
            })
    }

    pub(super) fn prepare(
        &self,
    ) -> renderpilot_application::AppResult<super::super::permit::PreparedPeerCommitPermit> {
        self.prepare_with_guards(&self.read_guards)
    }

    pub(super) fn endpoint_evidence(&self) -> Vec<PeerEndpointEvidence> {
        vec![
            PeerEndpointEvidence::new(self.intents[0].clone(), self.before_images[0].clone(), None),
            PeerEndpointEvidence::new(
                self.intents[1].clone(),
                self.before_images[1].clone(),
                Some(
                    PeerFileImage::new(
                        "catalog-restored",
                        Sha256Hash::new(ORIGINAL).expect("restored digest"),
                        8,
                    )
                    .expect("restored image"),
                ),
            ),
            PeerEndpointEvidence::new(
                self.intents[2].clone(),
                None,
                Some(
                    PeerFileImage::new(
                        "extra-created",
                        Sha256Hash::new(EXTRA).expect("extra digest"),
                        8,
                    )
                    .expect("extra image"),
                ),
            ),
        ]
    }
}

pub(super) fn guard_evidence(
    requirements: &[renderpilot_domain::PeerReadGuardRequirement],
    identity_suffix: &str,
) -> Vec<PeerReadGuardEvidence> {
    requirements
        .iter()
        .map(|requirement| {
            let observed = match requirement.expectation() {
                PeerReadGuardExpectation::Absent => None,
                PeerReadGuardExpectation::Digest { sha256 } => Some(
                    PeerFileImage::new(format!("{identity_suffix}-digest"), sha256.clone(), 8)
                        .expect("digest image"),
                ),
                PeerReadGuardExpectation::Receipt { identity, sha256 } => Some(
                    PeerFileImage::new(identity.clone(), sha256.clone(), 8).expect("receipt image"),
                ),
            };
            PeerReadGuardEvidence::new(requirement.path().clone(), observed)
        })
        .collect()
}

pub(super) fn ordinary_manifest(
    intents: &[PeerEndpointIntent],
    before_images: &[Option<PeerFileImage>],
    mutation_id: &str,
) -> String {
    let endpoint = |index: usize| {
        let intent = &intents[index];
        let before = before_images[index].as_ref().map(|image| {
            serde_json::json!({
                "identity": image.identity(),
                "sha256": image.sha256().as_str(),
                "length": image.length(),
            })
        });
        let operation = match intent.operation() {
            renderpilot_domain::PeerEndpointOperation::Create => "create",
            renderpilot_domain::PeerEndpointOperation::Replace => "replace",
            renderpilot_domain::PeerEndpointOperation::Remove => "remove",
        };
        let planned_sha256 = intent
            .planned_sha256()
            .map(Sha256Hash::as_str)
            .map_or(serde_json::Value::Null, serde_json::Value::from);
        serde_json::json!({
            "ordinal": index,
            "path": intent.path().as_str(),
            "role": "disjoint",
            "operation": operation,
            "planned_sha256": planned_sha256,
            "planned_length": intent.planned_length(),
            "before": before,
            "read_guards": [capability_token(intent.path().as_str())],
            "subtree_publishes": [],
        })
    };
    serde_json::to_string(&serde_json::json!({
        "format_version": 1,
        "roots": ["C:/game"],
        "snapshots": intents.iter().zip(before_images).map(|(intent, before)| serde_json::json!({
            "path": intent.path().as_str(),
            "snapshot": before.as_ref().map(|_| format!("C:/transaction/{mutation_id}/{}.before", intent.path().as_str().rsplit('/').next().expect("basename"))),
        })).collect::<Vec<_>>(),
        "peer_program": {
            "format": 1,
            "transaction_owner": mutation_id,
            "execution_class": "ordinary",
            "roots": ["C:/game"],
            "stage": [],
            "custody": [
                capability_token("C:/game/catalog.dll"),
                capability_token("C:/game/catalog.dll.bak"),
            ],
            "created_ancestors": [],
            "endpoints": (0..intents.len()).map(endpoint).collect::<Vec<_>>(),
        }
    }))
    .expect("manifest")
}

fn capability_token(path: &str) -> String {
    path.strip_prefix("C:/game/")
        .map(|relative| format!("C:/game:{relative}"))
        .expect("path under game root")
}

pub(super) fn addon_without_timestamps(addon: &InstalledAddon) -> InstalledAddon {
    addon.clone().with_timestamps(None, None)
}
