use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, ManagedAddonFile,
    ManagedFileBaseline, PathRef, PeerEndpointRole, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Sha256Hash,
};
use sha2::{Digest, Sha256};

use super::error::RenoDxActiveUpdateError;
use super::*;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

const OLD_ADDON: &[u8] = b"old renodx addon";
const NEW_ADDON: &[u8] = b"new renodx addon";
const OLD_HOST: &[u8] = b"old reshade host";
const NEW_HOST: &[u8] = b"new reshade host";

fn path(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("valid path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(Sha256::digest(bytes))).expect("valid digest")
}

fn snapshot(root: &Path, endpoint: &Path) -> PeerPathSnapshot {
    observe_peer_path_snapshot(&path(endpoint), &path(root)).expect("snapshot")
}

fn topology(root: &Path, with_host: bool) -> GameProxyTopology {
    let root_slot = root.join("dxgi.dll");
    let root_ref = path(&root_slot);
    let downstream = with_host.then(|| ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path: path(&root.join("ReShade64.dll")),
        receipt: FileReceipt::owned("reshade-host", digest(OLD_HOST)).expect("receipt"),
    });
    GameProxyTopology {
        id: "optiscaler:test".to_owned(),
        game_id: GameId::new("manual:renodx-active-update").expect("game id"),
        root_slot: root_ref.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_ref,
            receipt: FileReceipt::owned("optiscaler-proxy", digest(b"outer")).expect("receipt"),
        },
        downstream,
        downstream_origin: with_host.then_some(path(&root_slot)),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn record(
    game_id: &GameId,
    addon_path: PathRef,
    host: Option<(&Path, &[u8], &[u8])>,
) -> InstalledAddon {
    let record = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, addon_path);
    let Some((host_path, baseline, installed)) = host else {
        return record;
    };
    record
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            path(host_path),
            ManagedFileBaseline::Present {
                sha256: digest(baseline),
            },
            digest(installed),
        )])
        .expect("host claim")
}

struct ActiveUpdateFixtureInput<'a> {
    before: &'a renderpilot_domain::InstalledAddon,
    after: &'a renderpilot_domain::InstalledAddon,
    topology: &'a GameProxyTopology,
    root: &'a Path,
    addon_path: PathRef,
    addon_snapshot: &'a PeerPathSnapshot,
    addon_bytes: Vec<u8>,
    host: Option<RenoDxActiveUpdateHostInput<'a>>,
}

fn input(input: ActiveUpdateFixtureInput<'_>) -> RenoDxActiveUpdateInput<'_> {
    let ActiveUpdateFixtureInput {
        before,
        after,
        topology,
        root,
        addon_path,
        addon_snapshot,
        addon_bytes,
        host,
    } = input;
    RenoDxActiveUpdateInput {
        before_peer: before,
        after_peer: after,
        topology,
        canonical_game_root: root,
        payload_root: None,
        addon_path,
        addon_snapshot,
        addon_bytes,
        host,
        config: None,
    }
}

macro_rules! input {
    ($before:expr, $after:expr, $topology:expr, $root:expr, $addon_path:expr, $addon_snapshot:expr, $addon_bytes:expr, $host:expr $(,)?) => {
        input(ActiveUpdateFixtureInput {
            before: $before,
            after: $after,
            topology: $topology,
            root: $root,
            addon_path: $addon_path,
            addon_snapshot: $addon_snapshot,
            addon_bytes: $addon_bytes,
            host: $host,
        })
    };
}

#[test]
fn addon_only_change_is_one_disjoint_replace() {
    let root = tempfile::tempdir().expect("root");
    let addon = root.path().join("renodx.addon64");
    std::fs::write(&addon, OLD_ADDON).expect("addon");
    let topology = topology(root.path(), false);
    let before = record(&topology.game_id, path(&addon), None);
    let after = before.clone();

    let result = compose_active_update(input!(
        &before,
        &after,
        &topology,
        root.path(),
        path(&addon),
        &snapshot(root.path(), &addon),
        NEW_ADDON.to_vec(),
        None,
    ))
    .expect("compose");
    let physical = result.physical().expect("physical result");
    assert_eq!(physical.program().endpoints().len(), 1);
    assert_eq!(
        physical.program().endpoints()[0].role(),
        PeerEndpointRole::Disjoint
    );
    assert_eq!(physical.game_intents().len(), 1);
    assert_eq!(physical.payloads()[0], Some(NEW_ADDON.to_vec()));
}

#[test]
fn addon_and_owned_reshade_host_are_replaced_in_one_program() {
    let root = tempfile::tempdir().expect("root");
    let addon = root.path().join("renodx.addon64");
    let host = root.path().join("ReShade64.dll");
    std::fs::write(&addon, OLD_ADDON).expect("addon");
    std::fs::write(&host, OLD_HOST).expect("host");
    let topology = topology(root.path(), true);
    let addon_ref = path(&addon);
    let before = record(
        &topology.game_id,
        addon_ref.clone(),
        Some((&host, OLD_HOST, OLD_HOST)),
    );
    let after = record(
        &topology.game_id,
        addon_ref.clone(),
        Some((&host, OLD_HOST, NEW_HOST)),
    );

    let result = compose_active_update(input!(
        &before,
        &after,
        &topology,
        root.path(),
        addon_ref,
        &snapshot(root.path(), &addon),
        NEW_ADDON.to_vec(),
        Some(RenoDxActiveUpdateHostInput::new(
            path(&host),
            &snapshot(root.path(), &host),
            NEW_HOST.to_vec(),
        )),
    ))
    .expect("compose");
    let physical = result.physical().expect("physical result");
    assert_eq!(physical.program().endpoints().len(), 2);
    assert_eq!(
        physical.program().endpoints()[0].role(),
        PeerEndpointRole::Disjoint
    );
    assert_eq!(
        physical.program().endpoints()[1].role(),
        PeerEndpointRole::TopologyDownstream
    );
    assert_eq!(physical.game_intents().len(), 2);
    assert_eq!(
        physical.planned_topology(),
        &renderpilot_domain::PlannedGameProxyTopology::Exact(topology)
    );
}

#[test]
fn unchanged_endpoint_is_noop_and_record_only_change_is_metadata() {
    let root = tempfile::tempdir().expect("root");
    let addon = root.path().join("renodx.addon64");
    std::fs::write(&addon, OLD_ADDON).expect("addon");
    let topology = topology(root.path(), false);
    let addon_ref = path(&addon);
    let before = record(&topology.game_id, addon_ref.clone(), None);

    let noop = compose_active_update(input!(
        &before,
        &before,
        &topology,
        root.path(),
        addon_ref.clone(),
        &snapshot(root.path(), &addon),
        OLD_ADDON.to_vec(),
        None,
    ))
    .expect("noop compose");
    assert!(matches!(noop, RenoDxActiveUpdateComposition::Noop));

    let after = before.clone().with_addon_version("new-version");
    let metadata = compose_active_update(input!(
        &before,
        &after,
        &topology,
        root.path(),
        addon_ref,
        &snapshot(root.path(), &addon),
        OLD_ADDON.to_vec(),
        None,
    ))
    .expect("metadata compose");
    assert_eq!(metadata.metadata().expect("metadata").after_peer(), &after);
}

#[test]
fn missing_or_relocated_preimage_is_rejected() {
    let root = tempfile::tempdir().expect("root");
    let addon = root.path().join("renodx.addon64");
    std::fs::write(&addon, OLD_ADDON).expect("addon");
    let topology = topology(root.path(), false);
    let addon_ref = path(&addon);
    let before = record(&topology.game_id, addon_ref.clone(), None);

    let missing = compose_active_update(input!(
        &before,
        &before,
        &topology,
        root.path(),
        addon_ref.clone(),
        &PeerPathSnapshot::Absent,
        NEW_ADDON.to_vec(),
        None,
    ))
    .expect_err("missing retained preimage must fail closed");
    assert!(matches!(
        missing,
        RenoDxActiveUpdateError::BeforeImageMismatch(_)
    ));

    let relocated = root.path().join("other.dll");
    let relocated_error = compose_active_update(input!(
        &before,
        &before,
        &topology,
        root.path(),
        addon_ref,
        &snapshot(root.path(), &addon),
        NEW_ADDON.to_vec(),
        Some(RenoDxActiveUpdateHostInput::new(
            path(&relocated),
            &PeerPathSnapshot::Absent,
            NEW_HOST.to_vec(),
        )),
    ))
    .expect_err("host relocation must fail closed");
    assert!(matches!(
        relocated_error,
        RenoDxActiveUpdateError::InvalidPath(_)
    ));
}
