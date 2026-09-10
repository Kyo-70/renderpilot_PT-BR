use std::path::Path;

use renderpilot_domain::{
    AddonKind, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, GameId,
    InstalledAddon, LibraryComponent, LibraryTechnology, ManagedAddonFile, ManagedFileBaseline,
    ManagedFileMode, PathRef, PeerCatalogDeletedBaseline, PeerCatalogRollbackClaim, Swappability,
};

use crate::addons::luma::peer::{
    active_update::{dlss::project_dlss, model::LumaActiveUpdateDlssInput},
    effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder},
};
use crate::addons::luma::{
    peer::root_authority::LumaPeerRootAuthority, test_support::build_nvidia_dlss_pe,
};
use crate::catalog::cascade::{CascadeResult, ValidatedRollbackPlan};
use crate::coordinated_files::CatalogPathClaim;

fn path(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn empty_cascade() -> CascadeResult {
    CascadeResult::empty_for_test()
}

fn record(root: &Path, managed: Vec<ManagedAddonFile>) -> InstalledAddon {
    let game_id = GameId::new("manual:active-update-dlss").expect("game id");
    record_for_game(root, game_id, managed)
}

fn record_for_game(root: &Path, game_id: GameId, managed: Vec<ManagedAddonFile>) -> InstalledAddon {
    InstalledAddon::new(game_id, AddonKind::Luma, path(&root.join("Luma.addon")))
        .try_with_managed_files(managed)
        .expect("managed record")
}

fn authority(root: &Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(root, &root.join("dxgi.dll")).expect("authority")
}

fn claim(active: Vec<renderpilot_domain::Sha256Hash>) -> CatalogPathClaim {
    CatalogPathClaim::for_test(active, None)
}

fn claim_with_baseline(
    active: Vec<renderpilot_domain::Sha256Hash>,
    baseline: ManagedFileBaseline,
) -> CatalogPathClaim {
    CatalogPathClaim::for_test(active, Some(baseline))
}

fn component(file_path: &Path, game_id: &GameId, id: &str, bytes: &[u8]) -> LibraryComponent {
    LibraryComponent::new(
        ComponentId::new(id).expect("component id"),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(
        ComponentFile::new(path(file_path))
            .with_sha256(renderpilot_detection::sha256_bytes(bytes).expect("component digest")),
    )
}

fn cascade_for_test(
    target: &Path,
    current: &[u8],
    baseline: &[u8],
    spec_id: &str,
    claim_id: &str,
    next_components: Option<Vec<LibraryComponent>>,
) -> CascadeResult {
    let game_id = GameId::new("manual:active-update-dlss").expect("game id");
    let spec_component = component(target, &game_id, spec_id, current);
    let claim_component = component(target, &game_id, claim_id, current);
    let baseline_file = ComponentFile::new(path(target))
        .with_sha256(renderpilot_detection::sha256_bytes(baseline).expect("baseline digest"));
    let persisted_baseline = ComponentRollbackBaseline::new(vec![baseline_file]);
    let claim = PeerCatalogRollbackClaim::new(
        vec![claim_component],
        vec![PeerCatalogDeletedBaseline::new(
            ComponentId::new(claim_id).expect("claim id"),
            persisted_baseline.clone(),
        )],
    )
    .expect("catalog claim");
    let next_components = next_components.unwrap_or_else(|| claim.after_components().to_vec());
    CascadeResult::from_parts_for_test(
        vec![ValidatedRollbackPlan::for_test(
            spec_component,
            persisted_baseline,
        )],
        Some(claim),
        next_components,
    )
}

fn run(
    root: &Path,
    before: &InstalledAddon,
    input: LumaActiveUpdateDlssInput,
    catalog: &CatalogPathClaim,
) -> (
    crate::addons::luma::peer::active_update::model::DlssProjection,
    Option<crate::addons::luma::peer::effects::LumaPeerEffects>,
) {
    let authority = authority(root);
    let cascade = empty_cascade();
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = project_dlss(
        before,
        &authority,
        input,
        catalog,
        &cascade,
        &mut accumulator,
    )
    .expect("projection");
    let effects = accumulator.finalize().expect("effects");
    (projection, effects)
}

fn reject(
    root: &Path,
    before: &InstalledAddon,
    input: LumaActiveUpdateDlssInput,
    catalog: &CatalogPathClaim,
    cascade: &CascadeResult,
) -> String {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    project_dlss(
        before,
        &authority(root),
        input,
        catalog,
        cascade,
        &mut accumulator,
    )
    .expect_err("invalid active update projection")
    .to_string()
}

#[test]
fn none_absent_creates_owned_endpoint_without_catalog_claim() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), Vec::new());
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(bundled),
        },
        &claim(Vec::new()),
    );
    let binding = projection.into_binding().expect("binding");
    assert_eq!(binding.mode(), ManagedFileMode::Owned);
    assert!(matches!(binding.baseline(), ManagedFileBaseline::Absent));
    assert_eq!(effects.expect("create").program().endpoints().len(), 1);
}

#[test]
fn none_older_live_acquires_original_before_replacement() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    std::fs::write(&target, &live).expect("live");
    let before = record(root.path(), Vec::new());
    let (_, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(bundled),
        },
        &claim(Vec::new()),
    );
    let effects = effects.expect("acquisition");
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(effects.payloads()[0], Some(live));
}

#[test]
fn none_equal_or_newer_live_is_adopted_as_reused_without_effects() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 8, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    let (projection, effects) = run(
        root.path(),
        &record(root.path(), Vec::new()),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 7, 0, 0])),
        },
        &claim(vec![live_hash]),
    );
    assert_eq!(
        projection.into_binding().expect("reused binding").mode(),
        ManagedFileMode::Reused
    );
    assert!(effects.is_none());
}

#[test]
fn reused_equal_or_newer_live_is_kept_without_physical_effect() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 8, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("digest");
    std::fs::write(&target, live).expect("live");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::reused(path(&target), live_hash.clone())],
    );
    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 7, 0, 0])),
        },
        &claim(vec![live_hash]),
    );
    let binding = projection.into_binding().expect("binding");
    assert_eq!(binding.mode(), ManagedFileMode::Reused);
    assert!(effects.is_none());
}

#[test]
fn reused_older_live_is_adopted_only_after_exact_digest_and_empty_catalog() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, live).expect("live");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::reused(path(&target), live_hash)],
    );
    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(bundled),
        },
        &claim(Vec::new()),
    );
    assert_eq!(
        projection.into_binding().expect("binding").mode(),
        ManagedFileMode::Owned
    );
    assert_eq!(effects.expect("acquisition").program().endpoints().len(), 2);
}

#[test]
fn owned_newer_live_replaces_without_rewriting_baseline() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let original = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let bundled = build_nvidia_dlss_pe([3, 8, 0, 0]);
    let baseline_hash = renderpilot_detection::sha256_bytes(&original).expect("baseline hash");
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    std::fs::write(target.with_extension("dll.bak"), &original).expect("sidecar");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Present {
                sha256: baseline_hash.clone(),
            },
            live_hash,
        )],
    );
    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(bundled),
        },
        &claim(Vec::new()),
    );
    let binding = projection.into_binding().expect("binding");
    assert_eq!(binding.mode(), ManagedFileMode::Owned);
    assert_eq!(
        binding.baseline(),
        &ManagedFileBaseline::Present {
            sha256: baseline_hash
        }
    );
    assert_eq!(effects.expect("replace").program().endpoints().len(), 1);
}

#[test]
fn owned_absent_baseline_removes_directly_when_dlss_is_dropped() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, live).expect("live");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            live_hash,
        )],
    );
    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &claim(Vec::new()),
    );
    assert!(projection.into_binding().is_none());
    let effects = effects.expect("remove");
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn owned_present_baseline_restores_live_and_releases_the_exact_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let baseline = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let baseline_hash = renderpilot_detection::sha256_bytes(&baseline).expect("baseline hash");
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    std::fs::write(target.with_extension("dll.bak"), &baseline).expect("sidecar");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Present {
                sha256: baseline_hash,
            },
            live_hash,
        )],
    );
    let (_, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &claim(Vec::new()),
    );
    let effects = effects.expect("restore and release");
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(effects.payloads(), &[Some(baseline), None]);
}

#[test]
fn owned_release_consumed_by_catalog_cascade_emits_no_direct_duplicate() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let baseline = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let baseline_hash = renderpilot_detection::sha256_bytes(&baseline).expect("baseline hash");
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    std::fs::write(target.with_extension("dll.bak"), &baseline).expect("sidecar");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Present {
                sha256: baseline_hash.clone(),
            },
            live_hash.clone(),
        )],
    );
    let cascade = cascade_for_test(
        &target,
        &live,
        &baseline,
        "component:active-update-dlss",
        "component:active-update-dlss",
        None,
    );
    let (_, effects) = {
        let authority = authority(root.path());
        let mut accumulator =
            LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
        let projection = project_dlss(
            &before,
            &authority,
            LumaActiveUpdateDlssInput::Full {
                bundled_bytes: None,
            },
            &claim_with_baseline(
                vec![live_hash],
                ManagedFileBaseline::Present {
                    sha256: baseline_hash,
                },
            ),
            &cascade,
            &mut accumulator,
        )
        .expect("cascade release");
        (projection, accumulator.finalize().expect("effects"))
    };
    assert!(effects.is_some());
    let effects = effects.expect("catalog cascade effects");
    let target_count = effects
        .program()
        .endpoints()
        .iter()
        .filter(|endpoint| endpoint.path() == &path(&target))
        .count();
    assert_eq!(
        target_count, 1,
        "cascade must own the target release exactly once"
    );
    assert_eq!(effects.program().endpoints().len(), 2);
}

#[test]
fn nonempty_catalog_cascade_is_rejected_for_preserve_and_non_owned_routes() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let baseline = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    let cascade = cascade_for_test(
        &target,
        &live,
        &baseline,
        "component:active-update-dlss",
        "component:active-update-dlss",
        None,
    );

    let none_error = reject(
        root.path(),
        &record(root.path(), Vec::new()),
        LumaActiveUpdateDlssInput::Preserve,
        &claim(vec![live_hash.clone()]),
        &cascade,
    );
    assert!(none_error.contains("owned DLSS release"));

    let reused = record(
        root.path(),
        vec![ManagedAddonFile::reused(path(&target), live_hash.clone())],
    );
    let reused_error = reject(
        root.path(),
        &reused,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &claim(vec![live_hash.clone()]),
        &cascade,
    );
    assert!(reused_error.contains("owned DLSS release"));

    let owned = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            live_hash.clone(),
        )],
    );
    let owned_error = reject(
        root.path(),
        &owned,
        LumaActiveUpdateDlssInput::Preserve,
        &claim(vec![live_hash]),
        &cascade,
    );
    assert!(owned_error.contains("owned DLSS release"));
}

#[test]
fn malformed_catalog_cascade_fails_closed_on_component_next_game_and_count() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let baseline = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let baseline_hash = renderpilot_detection::sha256_bytes(&baseline).expect("baseline hash");
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    std::fs::write(target.with_extension("dll.bak"), &baseline).expect("sidecar");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Present {
                sha256: baseline_hash.clone(),
            },
            live_hash.clone(),
        )],
    );
    let catalog = claim_with_baseline(
        vec![live_hash],
        ManagedFileBaseline::Present {
            sha256: baseline_hash,
        },
    );

    let component_mismatch = cascade_for_test(
        &target,
        &live,
        &baseline,
        "component:spec-id",
        "component:claim-id",
        None,
    );
    assert!(
        reject(
            root.path(),
            &before,
            LumaActiveUpdateDlssInput::Full {
                bundled_bytes: None,
            },
            &catalog,
            &component_mismatch,
        )
        .contains("rollback specs")
    );

    let next_mismatch = cascade_for_test(
        &target,
        &live,
        &baseline,
        "component:active-update-dlss",
        "component:active-update-dlss",
        Some(Vec::new()),
    );
    assert!(
        reject(
            root.path(),
            &before,
            LumaActiveUpdateDlssInput::Full {
                bundled_bytes: None,
            },
            &catalog,
            &next_mismatch,
        )
        .contains("rollback specs")
    );

    let other_game = GameId::new("manual:other-game").expect("game id");
    assert!(
        reject(
            root.path(),
            &record_for_game(root.path(), other_game, before.managed_files().to_vec()),
            LumaActiveUpdateDlssInput::Full {
                bundled_bytes: None,
            },
            &catalog,
            &cascade_for_test(
                &target,
                &live,
                &baseline,
                "component:active-update-dlss",
                "component:active-update-dlss",
                None,
            ),
        )
        .contains("another game")
    );

    let valid = cascade_for_test(
        &target,
        &live,
        &baseline,
        "component:active-update-dlss",
        "component:active-update-dlss",
        None,
    );
    let extra_spec = ValidatedRollbackPlan::for_test(
        component(
            &root.path().join("other.dll"),
            before.game_id(),
            "component:other",
            b"other",
        ),
        ComponentRollbackBaseline::new(Vec::new()),
    );
    let claim = valid.catalog_claim().cloned();
    let next_components = valid.next_components;
    let mut specs = valid.rollback_specs;
    specs.push(extra_spec);
    let count_mismatch = CascadeResult::from_parts_for_test(specs, claim, next_components);
    assert!(
        reject(
            root.path(),
            &before,
            LumaActiveUpdateDlssInput::Full {
                bundled_bytes: None,
            },
            &catalog,
            &count_mismatch,
        )
        .contains("multiple catalog rollback")
    );
}

#[test]
fn preserve_none_reused_and_owned_are_record_only() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");

    let (projection, effects) = run(
        root.path(),
        &record(root.path(), Vec::new()),
        LumaActiveUpdateDlssInput::Preserve,
        &claim(Vec::new()),
    );
    assert!(projection.into_binding().is_none());
    assert!(effects.is_none());

    let reused = record(
        root.path(),
        vec![ManagedAddonFile::reused(path(&target), live_hash.clone())],
    );
    let (projection, effects) = run(
        root.path(),
        &reused,
        LumaActiveUpdateDlssInput::Preserve,
        &claim(vec![live_hash.clone()]),
    );
    assert_eq!(
        projection.into_binding().expect("reused binding").mode(),
        ManagedFileMode::Reused
    );
    assert!(effects.is_none());

    let owned = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            live_hash.clone(),
        )],
    );
    let (projection, effects) = run(
        root.path(),
        &owned,
        LumaActiveUpdateDlssInput::Preserve,
        &claim(vec![live_hash]),
    );
    assert_eq!(
        projection.into_binding().expect("owned binding").mode(),
        ManagedFileMode::Owned
    );
    assert!(effects.is_none());
}

#[test]
fn none_without_bundled_dlss_never_inspects_foreign_live_bytes() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let foreign = b"foreign file that is deliberately not a PE image";
    std::fs::write(&target, foreign).expect("foreign live");
    let catalog_hash = renderpilot_detection::sha256_bytes(foreign).expect("catalog hash");
    let before = record(root.path(), Vec::new());

    for input in [
        LumaActiveUpdateDlssInput::Preserve,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
    ] {
        let (projection, effects) = run(
            root.path(),
            &before,
            input,
            &claim(vec![catalog_hash.clone()]),
        );
        assert!(projection.into_binding().is_none());
        assert!(effects.is_none());
    }

    assert_eq!(std::fs::read(&target).expect("foreign live"), foreign);
}

#[test]
fn persisted_no_payload_routes_authenticate_bytes_without_parsing_dlss_metadata() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let retained = b"persisted bytes that are not a DLSS PE image";
    std::fs::write(&target, retained).expect("retained live");
    let digest = renderpilot_detection::sha256_bytes(retained).expect("digest");

    let reused = record(
        root.path(),
        vec![ManagedAddonFile::reused(path(&target), digest.clone())],
    );
    let (projection, effects) = run(
        root.path(),
        &reused,
        LumaActiveUpdateDlssInput::Preserve,
        &claim(vec![digest.clone()]),
    );
    assert_eq!(
        projection.into_binding().expect("reused binding").mode(),
        ManagedFileMode::Reused
    );
    assert!(effects.is_none());

    let (projection, effects) = run(
        root.path(),
        &reused,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &claim(vec![digest.clone()]),
    );
    assert!(projection.into_binding().is_none());
    assert!(effects.is_none());

    let owned = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            digest,
        )],
    );
    let (projection, effects) = run(
        root.path(),
        &owned,
        LumaActiveUpdateDlssInput::Preserve,
        &claim(Vec::new()),
    );
    assert_eq!(
        projection.into_binding().expect("owned binding").mode(),
        ManagedFileMode::Owned
    );
    assert!(effects.is_none());
}

#[test]
fn owned_routes_use_persisted_digest_instead_of_stale_catalog_active_hash() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    let stale_catalog_hash =
        renderpilot_detection::sha256_bytes(b"stale catalog image").expect("catalog hash");
    std::fs::write(&target, &live).expect("live");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            live_hash,
        )],
    );
    let catalog = claim(vec![stale_catalog_hash]);

    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Preserve,
        &catalog,
    );
    assert_eq!(
        projection.into_binding().expect("owned binding").mode(),
        ManagedFileMode::Owned
    );
    assert!(effects.is_none());

    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 8, 0, 0])),
        },
        &catalog,
    );
    assert_eq!(
        projection.into_binding().expect("updated binding").mode(),
        ManagedFileMode::Owned
    );
    assert_eq!(effects.expect("replace").program().endpoints().len(), 1);

    let (projection, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &catalog,
    );
    assert!(projection.into_binding().is_none());
    assert_eq!(effects.expect("remove").program().endpoints().len(), 1);
}

#[test]
fn dropping_none_or_reused_is_record_only_and_missing_reused_fails_closed() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");

    let (projection, effects) = run(
        root.path(),
        &record(root.path(), Vec::new()),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &claim(vec![live_hash.clone()]),
    );
    assert!(projection.into_binding().is_none());
    assert!(effects.is_none());

    let reused = record(
        root.path(),
        vec![ManagedAddonFile::reused(path(&target), live_hash.clone())],
    );
    let (projection, effects) = run(
        root.path(),
        &reused,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: None,
        },
        &claim(vec![live_hash.clone()]),
    );
    assert!(projection.into_binding().is_none());
    assert!(effects.is_none());

    std::fs::remove_file(&target).expect("remove live");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = project_dlss(
        &reused,
        &authority(root.path()),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 8, 0, 0])),
        },
        &claim(vec![live_hash]),
        &empty_cascade(),
        &mut accumulator,
    )
    .expect_err("missing reused live must reject adoption");
    assert!(error.to_string().contains("absent"));
    assert!(accumulator.finalize().expect("effects").is_none());
}

#[test]
fn incompatible_live_and_catalog_baseline_drift_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let incompatible = build_nvidia_dlss_pe([1, 5, 0, 0]);
    let incompatible_hash = renderpilot_detection::sha256_bytes(&incompatible).expect("hash");
    std::fs::write(&target, &incompatible).expect("live");
    let before = record(root.path(), Vec::new());
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = project_dlss(
        &before,
        &authority(root.path()),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 8, 0, 0])),
        },
        &claim(vec![incompatible_hash]),
        &empty_cascade(),
        &mut accumulator,
    )
    .expect_err("incompatible live must reject");
    assert!(error.to_string().contains("incompatible"));
    assert!(accumulator.finalize().expect("effects").is_none());

    let compatible = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let compatible_hash = renderpilot_detection::sha256_bytes(&compatible).expect("hash");
    std::fs::write(&target, &compatible).expect("live");
    let owned = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            compatible_hash.clone(),
        )],
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = project_dlss(
        &owned,
        &authority(root.path()),
        LumaActiveUpdateDlssInput::Preserve,
        &claim_with_baseline(
            vec![compatible_hash],
            ManagedFileBaseline::Present {
                sha256: renderpilot_detection::sha256_bytes(&build_nvidia_dlss_pe([3, 6, 0, 0]))
                    .expect("baseline hash"),
            },
        ),
        &empty_cascade(),
        &mut accumulator,
    )
    .expect_err("catalog baseline disagreement must reject");
    assert!(error.to_string().contains("baseline"));
    assert!(accumulator.finalize().expect("effects").is_none());
}

#[test]
fn generic_dlss_alias_and_existing_effect_are_rejected_without_duplicates() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let before = record(root.path(), Vec::new()).with_created_file(path(&target));
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = project_dlss(
        &before,
        &authority(root.path()),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 7, 0, 0])),
        },
        &claim(Vec::new()),
        &empty_cascade(),
        &mut accumulator,
    )
    .expect_err("generic target alias must reject");
    assert!(error.to_string().contains("generic"));

    let empty_before = record(root.path(), Vec::new());
    accumulator
        .create(
            crate::addons::luma::peer::effects::LumaPeerEffectGroup::DlssCascade,
            path(&target),
            build_nvidia_dlss_pe([3, 6, 0, 0]),
        )
        .expect("seed endpoint");
    let error = project_dlss(
        &empty_before,
        &authority(root.path()),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 7, 0, 0])),
        },
        &claim(Vec::new()),
        &empty_cascade(),
        &mut accumulator,
    )
    .expect_err("duplicate physical target must reject");
    assert!(error.to_string().contains("conflicting"));
    assert_eq!(
        accumulator
            .finalize()
            .expect("effects")
            .expect("seed effect")
            .program()
            .endpoints()
            .len(),
        1
    );

    let duplicate = ManagedAddonFile::owned(
        path(&target),
        ManagedFileBaseline::Absent,
        renderpilot_detection::sha256_bytes(&build_nvidia_dlss_pe([3, 7, 0, 0])).expect("digest"),
    );
    let mut record_json = serde_json::to_value(record(root.path(), Vec::new())).expect("record");
    record_json["managed_files"] = serde_json::json!([
        serde_json::to_value(&duplicate).expect("first binding"),
        serde_json::to_value(&duplicate).expect("duplicate binding")
    ]);
    let malformed: InstalledAddon = serde_json::from_value(record_json).expect("record");
    let error = reject(
        root.path(),
        &malformed,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 8, 0, 0])),
        },
        &claim(Vec::new()),
        &empty_cascade(),
    );
    assert!(error.contains("unique"));
}

#[test]
fn owned_equal_bytes_and_newer_live_are_exact_noops_without_downgrade() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 8, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, &live).expect("live");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Absent,
            live_hash,
        )],
    );
    let (_, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(live),
        },
        &claim(Vec::new()),
    );
    assert!(effects.is_none());

    let (_, effects) = run(
        root.path(),
        &before,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 7, 0, 0])),
        },
        &claim(Vec::new()),
    );
    assert!(effects.is_none());
}

#[test]
fn owned_present_baseline_requires_the_exact_sidecar_image() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let baseline = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let baseline_hash = renderpilot_detection::sha256_bytes(&baseline).expect("baseline hash");
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, live).expect("live");
    std::fs::write(target.with_extension("dll.bak"), b"wrong baseline").expect("sidecar");
    let before = record(
        root.path(),
        vec![ManagedAddonFile::owned(
            path(&target),
            ManagedFileBaseline::Present {
                sha256: baseline_hash,
            },
            live_hash,
        )],
    );
    let authority = authority(root.path());
    let cascade = empty_cascade();
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    assert!(
        project_dlss(
            &before,
            &authority,
            LumaActiveUpdateDlssInput::Full {
                bundled_bytes: Some(build_nvidia_dlss_pe([3, 8, 0, 0])),
            },
            &claim(Vec::new()),
            &cascade,
            &mut accumulator,
        )
        .is_err()
    );
    assert!(accumulator.finalize().expect("effects").is_none());
}

#[test]
fn catalog_drift_rejects_before_any_effect_is_added() {
    let root = tempfile::tempdir().expect("root");
    let target = root
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let live = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = renderpilot_detection::sha256_bytes(&live).expect("live hash");
    std::fs::write(&target, live).expect("live");
    let before = record(root.path(), Vec::new());
    let authority = authority(root.path());
    let cascade = empty_cascade();
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = project_dlss(
        &before,
        &authority,
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: Some(build_nvidia_dlss_pe([3, 8, 0, 0])),
        },
        &claim(vec![live_hash]),
        &cascade,
        &mut accumulator,
    )
    .expect_err("wrong catalog generation should reject");
    assert!(error.to_string().contains("catalog"));
    assert!(accumulator.finalize().expect("effects").is_none());
}
