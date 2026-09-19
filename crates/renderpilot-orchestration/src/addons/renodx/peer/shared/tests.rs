use super::*;
use crate::addons::shared_vulkan_mutation::FileIntent;
use renderpilot_domain::{
    AddonKind, FileReceipt, PathRef, ProxyImplementation, ProxyLink, ProxyRootPrestate,
    RenoDxReshadeIniFeature, SharedArtifactKind, SharedArtifactOrigin, SharedArtifactSource,
};
use renderpilot_platform_windows::vulkan_layer::{
    DirectoryMutation, DirectoryObservation, FileMutation, FileObservation, LAYER_DLL_NAME,
    LayerPlanOperation, RegistryMutation, RegistryValueState, SharedVulkanLayerPlan,
};

fn path(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
}

fn record(game_id: &GameId, game_root: &std::path::Path) -> InstalledAddon {
    InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        path(&game_root.join("renodx.addon64")),
    )
    .with_host_kind(renderpilot_domain::InstalledAddonHostKind::SharedVulkanLayer)
    .with_registered_exe_path(path(&game_root.join("game.exe")))
}

fn topology(game_id: &GameId, game_root: &std::path::Path) -> GameProxyTopology {
    let root_slot = path(&game_root.join("dxgi.dll"));
    GameProxyTopology {
        id: "renodx:shared-test".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", hash('a')).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn hash(value: char) -> renderpilot_domain::Sha256Hash {
    renderpilot_domain::Sha256Hash::new(value.to_string().repeat(64)).expect("hash")
}

fn register_plan(layer_dir: &std::path::Path) -> SharedVulkanLayerPlan {
    SharedVulkanLayerPlan {
        operation: LayerPlanOperation::RegisterApp,
        files: Vec::new(),
        registry: Some(RegistryMutation {
            manifest_path: layer_dir.join("ReShade64.json"),
            before: RegistryValueState::Absent,
            after: RegistryValueState::Present {
                value_type: 4,
                raw_bytes: vec![0; 4],
            },
        }),
        directory: DirectoryMutation {
            path: layer_dir.to_path_buf(),
            before: DirectoryObservation {
                exists: true,
                entries: Vec::new(),
            },
            create_if_absent: false,
            remove_if_empty: false,
        },
        unregister_outcome: None,
    }
}

fn install_plan(layer_dir: &std::path::Path, bytes: &[u8]) -> SharedVulkanLayerPlan {
    SharedVulkanLayerPlan {
        operation: LayerPlanOperation::InstallAndRegister,
        files: vec![FileMutation {
            path: layer_dir.join(LAYER_DLL_NAME),
            before: FileObservation::Absent,
            after: FileObservation::Present(bytes.to_vec()),
        }],
        registry: None,
        directory: DirectoryMutation {
            path: layer_dir.to_path_buf(),
            before: DirectoryObservation {
                exists: false,
                entries: Vec::new(),
            },
            create_if_absent: true,
            remove_if_empty: false,
        },
        unregister_outcome: None,
    }
}

fn refresh_plan(layer_dir: &std::path::Path, before: &[u8], after: &[u8]) -> SharedVulkanLayerPlan {
    SharedVulkanLayerPlan {
        operation: LayerPlanOperation::Refresh,
        files: vec![FileMutation {
            path: layer_dir.join(LAYER_DLL_NAME),
            before: FileObservation::Present(before.to_vec()),
            after: FileObservation::Present(after.to_vec()),
        }],
        registry: None,
        directory: DirectoryMutation {
            path: layer_dir.to_path_buf(),
            before: DirectoryObservation {
                exists: true,
                entries: Vec::new(),
            },
            create_if_absent: false,
            remove_if_empty: false,
        },
        unregister_outcome: None,
    }
}

fn download(bytes: Vec<u8>) -> Download {
    Download {
        digest: crate::addons::reshade::fetch::sha256_hex(&bytes),
        bytes,
        etag: None,
        last_modified: None,
    }
}

fn source() -> ReshadeSource {
    ReshadeSource {
        channel: crate::addons::reshade::types::ReshadeChannel::Stable,
        url: "https://example.invalid/reshade.zip".to_owned(),
    }
}

fn shared_record(layer_dir: &std::path::Path, bytes: &[u8]) -> SharedArtifactRecord {
    SharedArtifactRecord::new(
        SharedArtifactKind::RenoDxVulkanLayer,
        path(layer_dir),
        path(&layer_dir.join("ReShade64.json")),
        path(&layer_dir.join(LAYER_DLL_NAME)),
        SharedArtifactOrigin::RenderPilotCreated,
    )
    .with_source(SharedArtifactSource::known(
        "https://example.invalid/reshade.zip",
        None,
        crate::addons::reshade::fetch::sha256_hex(bytes),
        None,
        "stable",
    ))
    .with_created_files(vec![
        path(&layer_dir.join("ReShade64.json")),
        path(&layer_dir.join(LAYER_DLL_NAME)),
    ])
}

fn intent(game_root: &std::path::Path) -> FileIntent {
    FileIntent {
        live_path: game_root.join("renodx.addon64"),
        before: None,
        after: Some(vec![1]),
    }
}

struct ActiveSharedValidationFixture<'a> {
    feature: &'a str,
    game_root: &'a std::path::Path,
    layer_dir: &'a std::path::Path,
    topology: &'a GameProxyTopology,
    before_record: Option<&'a InstalledAddon>,
    after_record: &'a InstalledAddon,
    intents: &'a [FileIntent],
    plan: &'a SharedVulkanLayerPlan,
    authority: Option<&'a RenoDxReshadeIniAuthority>,
    source: Option<SharedLayerSource<'a>>,
    shared_record: Option<&'a SharedArtifactRecord>,
}

fn validate_request(
    ActiveSharedValidationFixture {
        feature,
        game_root,
        layer_dir,
        topology,
        before_record,
        after_record,
        intents,
        plan,
        authority,
        source,
        shared_record,
    }: ActiveSharedValidationFixture<'_>,
) -> Result<(), ActiveSharedMutationError> {
    validation::validate_inputs(&ActiveSharedMutationValidation {
        feature,
        game_id: after_record.game_id(),
        game_root,
        topology,
        before_record,
        after_record,
        game_intents: intents,
        shared_plan: plan,
        reshade_ini_authority: authority,
        layer_dir,
        source,
        shared_record,
    })
}

macro_rules! validate_request {
    ($feature:expr, $game_root:expr, $layer_dir:expr, $topology:expr, $before_record:expr, $after_record:expr, $intents:expr, $plan:expr, $authority:expr, $source:expr, $shared_record:expr $(,)?) => {
        validate_request(ActiveSharedValidationFixture {
            feature: $feature,
            game_root: $game_root,
            layer_dir: $layer_dir,
            topology: $topology,
            before_record: $before_record,
            after_record: $after_record,
            intents: $intents,
            plan: $plan,
            authority: $authority,
            source: $source,
            shared_record: $shared_record,
        })
    };
}

#[test]
fn install_matrix_preserves_register_route_and_typed_authority() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-install").expect("game id");
    let after = record(&game_id, game.path());
    let topology = topology(&game_id, game.path());
    let authority =
        RenoDxReshadeIniAuthority::new(RenoDxReshadeIniFeature::Install, path(game.path()))
            .expect("authority");
    validate_request!(
        renderpilot_domain::mutation_features::RENODX_INSTALL,
        game.path(),
        layer.path(),
        &topology,
        None,
        &after,
        &[intent(game.path())],
        &register_plan(layer.path()),
        Some(&authority),
        None,
        None,
    )
    .expect("register install should validate");

    let mut no_op = register_plan(layer.path());
    no_op.registry = None;
    assert!(matches!(
        validate_request!(
            renderpilot_domain::mutation_features::RENODX_INSTALL,
            game.path(),
            layer.path(),
            &topology,
            None,
            &after,
            &[intent(game.path())],
            &no_op,
            None,
            None,
            None,
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared-layer plan is a no-op; use the ordinary game route"
        ))
    ));
}

#[test]
fn install_matrix_requires_download_to_match_the_dll_postimage() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-download").expect("game id");
    let after = record(&game_id, game.path());
    let topology = topology(&game_id, game.path());
    let bytes = vec![2, 3, 4];
    let plan = install_plan(layer.path(), &bytes);
    let source = source();
    let mut bad_plan = plan.clone();
    bad_plan.files[0].after = FileObservation::Present(vec![9]);
    assert!(matches!(
        validate_request!(
            renderpilot_domain::mutation_features::RENODX_INSTALL,
            game.path(),
            layer.path(),
            &topology,
            None,
            &after,
            &[intent(game.path())],
            &bad_plan,
            None,
            Some((&source, &download(bytes.clone()))),
            None,
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared install plan does not publish the prepared ReShade DLL"
        ))
    ));
    let mut bad_download = download(bytes);
    bad_download.digest = "0".repeat(64);
    assert!(matches!(
        validate_request!(
            renderpilot_domain::mutation_features::RENODX_INSTALL,
            game.path(),
            layer.path(),
            &topology,
            None,
            &after,
            &[intent(game.path())],
            &plan,
            None,
            Some((&source, &bad_download)),
            None,
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared download digest does not match its bytes"
        ))
    ));
}

#[test]
fn update_accepts_refresh_with_only_shared_physical_intents() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-update").expect("game id");
    let before = record(&game_id, game.path());
    let after = before.clone().with_addon_version("next");
    let topology = topology(&game_id, game.path());
    let plan = refresh_plan(layer.path(), &[1], &[8, 9]);
    let record = shared_record(layer.path(), &[8, 9]);

    validate_request!(
        renderpilot_domain::mutation_features::RENODX_UPDATE,
        game.path(),
        layer.path(),
        &topology,
        Some(&before),
        &after,
        &[],
        &plan,
        None,
        None,
        Some(&record),
    )
    .expect("shared-only refresh should validate");
}

#[test]
fn channel_switch_accepts_refresh_with_stable_shared_peer_projection() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-channel-switch").expect("game id");
    let before = record(&game_id, game.path());
    let after = before.clone().with_reshade_channel("nightly");
    let topology = topology(&game_id, game.path());
    let plan = refresh_plan(layer.path(), &[1], &[8, 9]);
    let shared_record = shared_record(layer.path(), &[8, 9]);

    validate_request!(
        renderpilot_domain::mutation_features::RENODX_SWITCH_RESHADE_CHANNEL,
        game.path(),
        layer.path(),
        &topology,
        Some(&before),
        &after,
        &[],
        &plan,
        None,
        None,
        Some(&shared_record),
    )
    .expect("stable shared peer projection should validate");
}

#[test]
fn channel_switch_rejects_non_refresh_plan_and_download_source() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-channel-switch-matrix").expect("game id");
    let before = record(&game_id, game.path());
    let after = before.clone().with_reshade_channel("nightly");
    let topology = topology(&game_id, game.path());
    let plan = refresh_plan(layer.path(), &[1], &[8, 9]);
    let shared_record = shared_record(layer.path(), &[8, 9]);
    let feature = renderpilot_domain::mutation_features::RENODX_SWITCH_RESHADE_CHANNEL;

    assert!(matches!(
        validate_request!(
            feature,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &after,
            &[],
            &register_plan(layer.path()),
            None,
            None,
            Some(&shared_record),
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared channel switch plan is not a refresh plan"
        ))
    ));

    let download = download(vec![5, 6]);
    let source = source();
    assert!(matches!(
        validate_request!(
            feature,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &after,
            &[],
            &plan,
            None,
            Some((&source, &download)),
            Some(&shared_record),
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared channel switch cannot carry layer download metadata"
        ))
    ));
}

#[test]
fn update_rejects_cross_combination_matrix() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-update-matrix").expect("game id");
    let before = record(&game_id, game.path());
    let after = before.clone().with_addon_version("next");
    let topology = topology(&game_id, game.path());
    let plan = refresh_plan(layer.path(), &[1], &[8, 9]);
    let record = shared_record(layer.path(), &[8, 9]);
    let feature = renderpilot_domain::mutation_features::RENODX_UPDATE;

    assert!(matches!(
        validate_request!(
            feature,
            game.path(),
            layer.path(),
            &topology,
            None,
            &after,
            &[],
            &plan,
            None,
            None,
            Some(&record)
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared update is missing its exact before record"
        ))
    ));
    assert!(matches!(
        validate_request!(
            feature,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &after,
            &[],
            &register_plan(layer.path()),
            None,
            None,
            Some(&record)
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared update plan is not a refresh plan"
        ))
    ));
    let bytes = vec![5, 6];
    let download = download(bytes);
    let source = source();
    assert!(matches!(
        validate_request!(
            feature,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &after,
            &[],
            &plan,
            None,
            Some((&source, &download)),
            Some(&record)
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared update cannot carry layer download metadata"
        ))
    ));
    assert!(matches!(
        validate_request!(
            feature,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &after,
            &[],
            &plan,
            None,
            None,
            None
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared update is missing its prepared shared-artifact record"
        ))
    ));
    let authority =
        RenoDxReshadeIniAuthority::new(RenoDxReshadeIniFeature::Update, path(game.path()))
            .expect("authority");
    validate_request!(
        feature,
        game.path(),
        layer.path(),
        &topology,
        Some(&before),
        &after,
        &[],
        &plan,
        Some(&authority),
        None,
        Some(&record)
    )
    .expect("shared update accepts matching ReShade.ini authority");
}

#[test]
fn update_binds_shared_record_to_the_refresh_postimage() {
    let game = tempfile::tempdir().expect("game");
    let layer = tempfile::tempdir().expect("layer");
    let game_id = GameId::new("manual:shared-provenance").expect("game id");
    let before = record(&game_id, game.path());
    let topology = topology(&game_id, game.path());
    let plan = refresh_plan(layer.path(), &[1], &[8, 9]);
    let mut wrong_digest = shared_record(layer.path(), &[7]);
    assert!(matches!(
        validate_request!(
            renderpilot_domain::mutation_features::RENODX_UPDATE,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &before,
            &[],
            &plan,
            None,
            None,
            Some(&wrong_digest),
        ),
        Err(ActiveSharedMutationError::InvalidInput(
            "shared update record digest does not match its DLL postimage"
        ))
    ));
    wrong_digest = SharedArtifactRecord::new(
        SharedArtifactKind::RenoDxVulkanLayer,
        path(&game.path().join("wrong")),
        path(&layer.path().join("ReShade64.json")),
        path(&layer.path().join(LAYER_DLL_NAME)),
        SharedArtifactOrigin::RenderPilotCreated,
    );
    assert!(matches!(
        validate_request!(
            renderpilot_domain::mutation_features::RENODX_UPDATE,
            game.path(),
            layer.path(),
            &topology,
            Some(&before),
            &before,
            &[],
            &plan,
            None,
            None,
            Some(&wrong_digest),
        ),
        Err(ActiveSharedMutationError::InvalidPath(_))
    ));
}
