use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileOwnership, FileReceipt, GameId, GameProxyTopology, InstalledAddon,
    InstalledAddonHostKind, ManagedAddonFile, ManagedFileBaseline, ManagedFileMode, PathRef,
    PeerEndpointRole, PlannedGameProxyTopology, ProxyImplementation, ProxyLink, ProxyRootPrestate,
    RenoDxConfigReceipt, RenoDxSetPathBaseline, RenoDxSetPathValue, Sha256Hash,
};

use crate::addons::renodx::peer::{
    RenoDxRootAuthority, compose_active_uninstall, snapshot_active_uninstall,
};
use crate::addons::reshade::proxy::HostKind;
use crate::peer_mutation_executor::{EndpointPostcondition, PeerPathSnapshot};

fn digest(bytes: &[u8]) -> Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn path(root: &Path, name: &str) -> PathRef {
    PathRef::new(root.join(name).to_string_lossy().into_owned()).expect("path")
}

fn topology(
    root: &Path,
    game_id: &GameId,
    host: Option<(&PathRef, &Sha256Hash, FileOwnership)>,
) -> GameProxyTopology {
    let root_slot = path(root, "dxgi.dll");
    GameProxyTopology {
        id: "optiscaler:renodx-active-uninstall-test".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("receipt"),
        },
        downstream: host.map(|(path, hash, ownership)| ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: path.clone(),
            receipt: match ownership {
                FileOwnership::Owned => FileReceipt::owned("reshade", hash.clone()),
                FileOwnership::Reused => FileReceipt::reused("reshade", hash.clone()),
            }
            .expect("receipt"),
        }),
        downstream_origin: host.map(|_| root_slot),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn proxy_case(
    mode: ManagedFileMode,
    baseline: ManagedFileBaseline,
    host_bytes: &[u8],
    sidecar_bytes: Option<&[u8]>,
) -> (
    tempfile::TempDir,
    InstalledAddon,
    GameProxyTopology,
    RenoDxRootAuthority,
) {
    let game = tempfile::tempdir().expect("game");
    let host_path = game.path().join("ReShade64.dll");
    std::fs::write(&host_path, host_bytes).expect("host");
    if let Some(bytes) = sidecar_bytes {
        std::fs::write(host_path.with_extension("dll.bak"), bytes).expect("sidecar");
    }
    let addon_path = game.path().join("renodx.addon64");
    std::fs::write(&addon_path, b"addon").expect("addon");
    let game_id = GameId::new("manual:renodx-active-uninstall-test").expect("game id");
    let host_ref = path(game.path(), "ReShade64.dll");
    let host_hash = digest(host_bytes);
    let record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(game.path(), "renodx.addon64"),
    )
    .with_host_kind(InstalledAddonHostKind::Proxy)
    .try_with_managed_files(vec![match mode {
        ManagedFileMode::Owned => {
            ManagedAddonFile::owned(host_ref.clone(), baseline, host_hash.clone())
        }
        ManagedFileMode::Reused => ManagedAddonFile::reused(host_ref.clone(), host_hash.clone()),
    }])
    .expect("managed host");
    let topology = topology(
        game.path(),
        &game_id,
        Some((
            &host_ref,
            &host_hash,
            match mode {
                ManagedFileMode::Owned => FileOwnership::Owned,
                ManagedFileMode::Reused => FileOwnership::Reused,
            },
        )),
    );
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Proxy, None, None).expect("authority");
    (game, record, topology, authority)
}

fn compose_case(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    authority: &RenoDxRootAuthority,
) -> Result<
    crate::addons::renodx::peer::ActiveUninstallComposition,
    crate::addons::renodx::peer::RenoDxActiveUninstallError,
> {
    let input = snapshot_active_uninstall(record, topology, authority).expect("snapshot");
    compose_active_uninstall(input.input())
}

type Result<T, E = crate::addons::renodx::peer::RenoDxActiveUninstallError> =
    std::result::Result<T, E>;

#[test]
fn reused_host_is_released_without_touching_the_host_or_topology() {
    let (_game, record, topology, authority) = proxy_case(
        ManagedFileMode::Reused,
        ManagedFileBaseline::Present {
            sha256: digest(b"reshade"),
        },
        b"reshade",
        None,
    );
    let composition = compose_case(&record, &topology, &authority).expect("composition");

    assert_eq!(composition.program.endpoints().len(), 1);
    assert_eq!(
        composition.program.endpoints()[0].role(),
        PeerEndpointRole::Disjoint
    );
    assert_eq!(composition.game_intents.len(), 1);
    assert_eq!(
        composition.planned_topology,
        PlannedGameProxyTopology::Exact(topology)
    );
}

#[test]
fn owned_absent_host_is_removed_without_a_sidecar_endpoint() {
    let (_game, record, topology, authority) = proxy_case(
        ManagedFileMode::Owned,
        ManagedFileBaseline::Absent,
        b"renodx-host",
        None,
    );
    let composition = compose_case(&record, &topology, &authority).expect("composition");

    assert_eq!(composition.program.endpoints().len(), 2);
    assert_eq!(
        composition.program.endpoints()[1].role(),
        PeerEndpointRole::TopologyDownstream
    );
    assert!(matches!(
        composition.program.endpoints()[1].after(),
        EndpointPostcondition::Absent
    ));
    let PlannedGameProxyTopology::Exact(after) = composition.planned_topology else {
        panic!("expected exact topology");
    };
    assert!(after.downstream.is_none());
    assert!(after.downstream_origin.is_none());
    assert_eq!(after.root_prestate, ProxyRootPrestate::Absent);
}

#[test]
fn owned_present_host_restores_baseline_then_removes_its_sidecar_adjacent() {
    let (_game, record, topology, authority) = proxy_case(
        ManagedFileMode::Owned,
        ManagedFileBaseline::Present {
            sha256: digest(b"reshade-baseline"),
        },
        b"renodx-host",
        Some(b"reshade-baseline"),
    );
    let composition = compose_case(&record, &topology, &authority).expect("composition");

    assert_eq!(composition.program.endpoints().len(), 3);
    let endpoints = composition.program.endpoints();
    assert_eq!(endpoints[1].role(), PeerEndpointRole::TopologyDownstream);
    assert_eq!(endpoints[2].role(), PeerEndpointRole::Disjoint);
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::File(_)
    ));
    assert!(matches!(
        endpoints[2].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(
        composition.game_intents[1].after,
        Some(b"reshade-baseline".to_vec())
    );
    assert!(matches!(
        composition.planned_topology,
        PlannedGameProxyTopology::Exact(_)
    ));
}

#[test]
fn created_config_is_removed_through_typed_exact_endpoint() {
    let game = tempfile::tempdir().expect("game");
    let addon = game.path().join("renodx.addon64");
    let ini = game.path().join("ReShade.ini");
    std::fs::write(&addon, b"addon").expect("addon");
    std::fs::write(&ini, b"[ADDON]\nAddonPath=.\n").expect("ini");
    let game_id = GameId::new("manual:renodx-active-vulkan-ini").expect("game id");
    let record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(game.path(), "renodx.addon64"),
    )
    .with_created_file(path(game.path(), "ReShade.ini"))
    .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer);
    let topology = topology(game.path(), &game_id, None);
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None).expect("authority");
    let composition = compose_case(&record, &topology, &authority).expect("composition");

    assert_eq!(composition.program.endpoints().len(), 2);
    assert_eq!(
        composition.program.endpoints()[1].role(),
        PeerEndpointRole::RenoDxReshadeIni
    );
    assert!(composition.reshade_ini_authority.is_some());
    assert!(composition.payloads[1].is_none());
}

#[test]
fn active_uninstall_ignores_wrong_path_config_receipt() {
    let game = tempfile::tempdir().expect("game");
    let addon = game.path().join("renodx.addon64");
    let ini = game.path().join("ReShade.ini");
    std::fs::write(&addon, b"addon").expect("addon");
    std::fs::write(&ini, b"[ADDON]\nAddonPath=.\n").expect("ini");
    let game_id = GameId::new("manual:renodx-active-wrong-receipt").expect("game id");
    let record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(game.path(), "renodx.addon64"),
    )
    .with_created_file(path(game.path(), "ReShade.ini"))
    .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer)
    .with_renodx_config_receipt(Some(RenoDxConfigReceipt::new(
        path(&game.path().join("other"), "ReShade.ini"),
        RenoDxSetPathBaseline::Absent,
        false,
        RenoDxSetPathValue::One,
    )))
    .expect("receipt invariant");
    let topology = topology(game.path(), &game_id, None);
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None).expect("authority");

    let composition = compose_case(&record, &topology, &authority).expect("composition");
    assert_eq!(composition.program.endpoints().len(), 2);
    assert!(composition.reshade_ini_authority.is_some());
}

#[test]
fn active_uninstall_keeps_running_when_typed_cleanup_is_ambiguous() {
    let game = tempfile::tempdir().expect("game");
    let addon = game.path().join("renodx.addon64");
    let ini = game.path().join("ReShade.ini");
    let bytes = b"[renodx]\nSet_Path\n[ADDON]\nAddonPath=.\nUser=keep\n";
    std::fs::write(&addon, b"addon").expect("addon");
    std::fs::write(&ini, bytes).expect("ini");
    let game_id = GameId::new("manual:renodx-active-ambiguous-receipt").expect("game id");
    let record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(game.path(), "renodx.addon64"),
    )
    .with_created_file(path(game.path(), "ReShade.ini"))
    .with_backed_up_file(path(game.path(), "ReShade.ini"))
    .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer)
    .with_renodx_config_receipt(Some(RenoDxConfigReceipt::new(
        path(game.path(), "ReShade.ini"),
        RenoDxSetPathBaseline::Absent,
        true,
        RenoDxSetPathValue::One,
    )))
    .expect("receipt invariant");
    let topology = topology(game.path(), &game_id, None);
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None).expect("authority");

    let composition = compose_case(&record, &topology, &authority).expect("composition");
    let after = composition.game_intents[1].after.as_ref().expect("after");
    assert!(String::from_utf8_lossy(after).contains("Set_Path"));
    assert!(
        after
            .windows(b"User=keep".len())
            .any(|window| window == b"User=keep")
    );
    assert!(
        !after
            .windows(b"AddonPath".len())
            .any(|window| window == b"AddonPath")
    );
}

#[test]
fn created_and_backed_config_is_transformed_and_retained_as_replace() {
    let game = tempfile::tempdir().expect("game");
    let addon = game.path().join("renodx.addon64");
    let ini = game.path().join("ReShade.ini");
    let original = b"[GENERAL]\nPreset=mine.ini\n\n[ADDON]\nAddonPath=.\nUser=keep\n";
    std::fs::write(&addon, b"addon").expect("addon");
    std::fs::write(&ini, original).expect("ini");
    let game_id = GameId::new("manual:renodx-active-backed-ini").expect("game id");
    let record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(game.path(), "renodx.addon64"),
    )
    .with_created_file(path(game.path(), "ReShade.ini"))
    .with_backed_up_file(path(game.path(), "ReShade.ini"))
    .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer);
    let topology = topology(game.path(), &game_id, None);
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None).expect("authority");
    let composition = compose_case(&record, &topology, &authority).expect("composition");

    assert_eq!(composition.program.endpoints().len(), 2);
    assert_eq!(
        composition.program.endpoints()[1].role(),
        PeerEndpointRole::RenoDxReshadeIni
    );
    assert_eq!(composition.game_intents[1].before, Some(original.to_vec()));
    let after = composition.game_intents[1].after.as_ref().expect("after");
    assert!(
        !after
            .windows(b"AddonPath".len())
            .any(|window| window == b"AddonPath")
    );
    assert!(
        after
            .windows(b"User=keep".len())
            .any(|window| window == b"User=keep")
    );
}

#[test]
fn unclaimed_config_without_renodx_keys_is_omitted() {
    let game = tempfile::tempdir().expect("game");
    let addon = game.path().join("renodx.addon64");
    std::fs::write(&addon, b"addon").expect("addon");
    std::fs::write(
        game.path().join("ReShade.ini"),
        b"[GENERAL]\r\nPreset=mine.ini\r\n",
    )
    .expect("ini");
    let game_id = GameId::new("manual:renodx-active-unclaimed-ini").expect("game id");
    let record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(game.path(), "renodx.addon64"),
    )
    .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer);
    let topology = topology(game.path(), &game_id, None);
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None).expect("authority");
    let composition = compose_case(&record, &topology, &authority).expect("composition");

    assert_eq!(composition.program.endpoints().len(), 1);
    assert!(composition.reshade_ini_authority.is_none());
}

#[test]
fn topology_receipt_drift_is_rejected_before_composition() {
    let (_game, record, mut topology, authority) = proxy_case(
        ManagedFileMode::Reused,
        ManagedFileBaseline::Present {
            sha256: digest(b"reshade"),
        },
        b"reshade",
        None,
    );
    topology.downstream.as_mut().expect("downstream").receipt =
        FileReceipt::reused("reshade", digest(b"expected")).expect("receipt");
    let error = compose_case(&record, &topology, &authority).expect_err("topology drift");
    assert!(error.to_string().contains("topology receipt"));
}

#[test]
fn relocated_downstream_is_rejected_even_when_the_record_matches_it() {
    let (_game, record, mut topology, authority) = proxy_case(
        ManagedFileMode::Reused,
        ManagedFileBaseline::Present {
            sha256: digest(b"reshade"),
        },
        b"reshade",
        None,
    );
    let relocated = PathRef::new(
        Path::new(record.addon_file().as_str())
            .parent()
            .expect("root")
            .join("relocated/ReShade64.dll")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("relocated path");
    topology.downstream.as_mut().expect("downstream").path = relocated;
    topology.downstream_origin = Some(topology.root_slot.clone());
    let error = snapshot_active_uninstall(&record, &topology, &authority)
        .expect_err("relocated downstream");
    assert!(error.to_string().contains("managed host claim"));
}

#[test]
fn repeated_canonical_addon_claim_is_one_endpoint() {
    let game = tempfile::tempdir().expect("game");
    let addon = path(game.path(), "renodx.addon64");
    std::fs::write(addon.as_str(), b"addon").expect("addon");
    let game_id = GameId::new("manual:renodx-active-duplicate-addon").expect("game id");
    let record = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, addon.clone())
        .with_created_file(addon)
        .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer);
    let topology = topology(game.path(), &game_id, None);
    let authority =
        RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None).expect("authority");
    let composition = compose_case(&record, &topology, &authority).expect("composition");
    assert_eq!(composition.program.endpoints().len(), 1);
}

#[test]
fn missing_owned_present_sidecar_fails_closed() {
    let (_game, record, topology, authority) = proxy_case(
        ManagedFileMode::Owned,
        ManagedFileBaseline::Present {
            sha256: digest(b"baseline"),
        },
        b"renodx-host",
        None,
    );
    let error = compose_case(&record, &topology, &authority).expect_err("missing sidecar");
    assert!(error.to_string().contains("sidecar") || error.to_string().contains("path"));
}

#[test]
fn endpoint_payloads_and_intents_remain_aligned() {
    let (_game, record, topology, authority) = proxy_case(
        ManagedFileMode::Owned,
        ManagedFileBaseline::Present {
            sha256: digest(b"baseline"),
        },
        b"renodx-host",
        Some(b"baseline"),
    );
    let composition = compose_case(&record, &topology, &authority).expect("composition");
    assert_eq!(
        composition.program.endpoints().len(),
        composition.payloads.len()
    );
    assert_eq!(
        composition.program.endpoints().len(),
        composition.game_intents.len()
    );
    assert!(
        composition
            .game_intents
            .iter()
            .all(|intent| intent.live_path.is_absolute())
    );
}

#[test]
fn snapshot_keeps_absent_host_without_following_a_sidecar() {
    let (_game, record, topology, authority) = proxy_case(
        ManagedFileMode::Owned,
        ManagedFileBaseline::Absent,
        b"renodx-host",
        None,
    );
    let input = snapshot_active_uninstall(&record, &topology, &authority).expect("snapshot");
    let host = input
        .input()
        .endpoints
        .iter()
        .find(|endpoint| endpoint.path().file_name() == Some("ReShade64.dll"))
        .expect("host endpoint");
    assert!(host.backup().is_none());
    assert!(matches!(host.snapshot(), PeerPathSnapshot::File(_)));
}
