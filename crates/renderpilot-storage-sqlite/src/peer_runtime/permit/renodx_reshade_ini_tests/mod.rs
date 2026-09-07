use renderpilot_domain::mutation_features::RENODX_INSTALL;
use renderpilot_domain::{
    AddonKind, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, FileReceipt,
    GameId, GameProxyTopology, InstalledAddon, LibraryComponent, LibraryTechnology, PathRef,
    PeerCatalogDeletedBaseline, PeerCatalogRollbackClaim, ProxyImplementation, ProxyLink,
    ProxyPeerRoute, ProxyRootPrestate, RenoDxReshadeIniAuthority, Sha256Hash, Swappability,
};
use serde_json::json;

use super::{PeerCommitPreparation, PeerStorageRuntime};
use crate::repositories::ComponentBaselineMutation;
use crate::{BeginFileMutationPreparation, SqliteStorage};

mod commit;
mod preparation;
mod shared;
mod shared_preparation;
mod shared_roots;

pub(super) const ROOT: &str = "C:/game";
pub(super) const INI: &str = "C:/game/ReShade.ini";
const ADDON: &str = "C:/game/RenoDx.addon64";
const BEFORE_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const AFTER_DIGEST: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

pub(super) fn path(value: &str) -> PathRef {
    PathRef::new(value).expect("path")
}

pub(super) fn hash(value: &str) -> Sha256Hash {
    Sha256Hash::new(value).expect("digest")
}

pub(super) fn authority(feature: &str) -> RenoDxReshadeIniAuthority {
    RenoDxReshadeIniAuthority::try_from_feature(feature, path(ROOT)).expect("authority")
}

pub(super) fn main_after(game_id: &GameId) -> InstalledAddon {
    InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, path(ADDON))
        .with_created_file(path(INI))
}

pub(super) fn topology(game_id: &GameId) -> GameProxyTopology {
    let root_slot = path("C:/game/dxgi.dll");
    GameProxyTopology {
        id: "topology:renodx-permit".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer-id", hash(BEFORE_DIGEST)).expect("outer receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

pub(super) fn install_manifest(id: &str) -> String {
    serde_json::to_string(&json!({
        "format_version": 1,
        "roots": [ROOT],
        "snapshots": [
            {"path": ADDON, "snapshot": null},
            {"path": INI, "snapshot": null}
        ],
        "peer_program": {
            "format": 1,
            "transaction_owner": id,
            "execution_class": "ordinary",
            "roots": [ROOT],
            "stage": [],
            "custody": [],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": ADDON,
                "role": "disjoint",
                "operation": "create",
                "planned_sha256": BEFORE_DIGEST,
                "planned_length": 8,
                "before": null,
                "read_guards": ["C:/game:renodx.addon64"],
                "subtree_publishes": []
            }, {
                "ordinal": 1,
                "path": INI,
                "role": "renodx_reshade_ini",
                "operation": "create",
                "planned_sha256": AFTER_DIGEST,
                "planned_length": 4,
                "before": null,
                "read_guards": ["C:/game:reshade.ini"],
                "subtree_publishes": []
            }]
        }
    }))
    .expect("install manifest")
}

pub(super) fn begin(storage: &SqliteStorage, id: &str, game_id: &GameId, feature: &str) {
    storage
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: feature.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin preparation");
}

pub(super) fn finish_install(
    runtime: &PeerStorageRuntime,
    id: &str,
    game_id: &GameId,
    after: &InstalledAddon,
    manifest: &str,
    component_set: Option<&[renderpilot_domain::LibraryComponent]>,
) -> renderpilot_application::AppResult<super::PreparedPeerCommitPermit> {
    let authority = authority(RENODX_INSTALL);
    finish_install_with_authority(
        runtime,
        id,
        game_id,
        after,
        manifest,
        component_set,
        Some(&authority),
    )
}

pub(super) fn finish_install_with_authority(
    runtime: &PeerStorageRuntime,
    id: &str,
    game_id: &GameId,
    after: &InstalledAddon,
    manifest: &str,
    component_set: Option<&[renderpilot_domain::LibraryComponent]>,
    renodx_reshade_ini: Option<&RenoDxReshadeIniAuthority>,
) -> renderpilot_application::AppResult<super::PreparedPeerCommitPermit> {
    finish_install_with_catalog(FinishInstallWithCatalogInput {
        runtime,
        id,
        game_id,
        after,
        manifest,
        component_set,
        baseline_mutations: &[],
        catalog_claim: None,
        renodx_reshade_ini,
    })
}

#[derive(Clone, Copy)]
pub(super) struct FinishInstallWithCatalogInput<'a> {
    pub(super) runtime: &'a PeerStorageRuntime,
    pub(super) id: &'a str,
    pub(super) game_id: &'a GameId,
    pub(super) after: &'a InstalledAddon,
    pub(super) manifest: &'a str,
    pub(super) component_set: Option<&'a [LibraryComponent]>,
    pub(super) baseline_mutations: &'a [ComponentBaselineMutation<'a>],
    pub(super) catalog_claim: Option<&'a PeerCatalogRollbackClaim>,
    pub(super) renodx_reshade_ini: Option<&'a RenoDxReshadeIniAuthority>,
}

pub(super) fn finish_install_with_catalog(
    input: FinishInstallWithCatalogInput<'_>,
) -> renderpilot_application::AppResult<super::PreparedPeerCommitPermit> {
    input
        .runtime
        .finish_file_peer_preparation(PeerCommitPreparation {
            mutation_id: input.id,
            game_id: input.game_id,
            feature: RENODX_INSTALL,
            subject_id: None,
            manifest_json: input.manifest,
            canonical_game_root: ROOT,
            initial_read_guards: &[],
            before_peer: None,
            after_peer: Some(input.after),
            before_topology: None,
            planned_after_topology: None,
            route: ProxyPeerRoute::DurableDisjoint,
            component_set: input.component_set,
            baseline_mutations: input.baseline_mutations,
            catalog_claim: input.catalog_claim,
            renodx_reshade_ini: input.renodx_reshade_ini,
        })
}

pub(super) fn catalog_claim(game_id: &GameId) -> PeerCatalogRollbackClaim {
    let component_id = ComponentId::new("component:renodx-catalog").expect("component id");
    let file = PathRef::new("C:/game/catalog.dll").expect("catalog file");
    let component = LibraryComponent::new(
        component_id.clone(),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::DlssSuperResolution,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(file.clone()).with_sha256(hash(BEFORE_DIGEST)));
    let baseline = ComponentRollbackBaseline::new(vec![
        ComponentFile::new(file).with_sha256(hash(BEFORE_DIGEST)),
    ]);
    PeerCatalogRollbackClaim::new(
        vec![component],
        vec![PeerCatalogDeletedBaseline::new(component_id, baseline)],
    )
    .expect("catalog claim")
}
