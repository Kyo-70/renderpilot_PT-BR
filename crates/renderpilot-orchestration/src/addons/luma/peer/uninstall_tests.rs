use std::path::{Path, PathBuf};

use renderpilot_domain::{
    AddonKind, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, FileOwnership,
    FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime,
    InstalledAddon, Launcher, LibraryComponent, LibraryTechnology, ManagedAddonFile,
    ManagedFileBaseline, PathRef, PlannedGameProxyTopology, Platform, ProxyImplementation,
    ProxyLink, ProxyRootPrestate, Sha256Hash, Swappability,
};

use renderpilot_application::{ComponentRepository, GameRepository, InstalledAddonRepository};
use renderpilot_storage_sqlite::SqliteStorage;

use crate::Context;
use crate::addons::luma::dlss::PlannedDlss;
use crate::catalog::cascade::cascade_for_managed_paths;
use crate::coordinated_files::CoordinatedFilePlan;

use super::root_authority::LumaPeerRootAuthority;
use super::uninstall::{PlannedManagedDlssRelease, compose_active_uninstall};
use crate::peer_mutation_executor::{EndpointExpectation, EndpointPostcondition};

type UninstallCompositionParts = (
    crate::peer_mutation_executor::ExactEndpointProgram,
    Vec<Option<Vec<u8>>>,
    PlannedGameProxyTopology,
);

fn path(root: &Path, name: &str) -> PathRef {
    PathRef::new(root.join(name).to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn game_id(label: &str) -> GameId {
    GameId::new(format!("manual:luma-uninstall:{label}")).expect("game id")
}

fn empty_cascade(game_id: &GameId) -> crate::catalog::cascade::CascadeResult {
    let database = tempfile::tempdir().expect("database");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    cascade_for_managed_paths(context.storage(), game_id, &[]).expect("empty cascade")
}

fn authority(root: &Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(root, &root.join("dxgi.dll")).expect("authority")
}

fn outer_topology(root: &Path, game_id: &GameId) -> GameProxyTopology {
    let root_slot = path(root, "dxgi.dll");
    GameProxyTopology {
        id: "optiscaler:luma-uninstall".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn host_topology(
    root: &Path,
    game_id: &GameId,
    host: &PathRef,
    ownership: FileOwnership,
    host_bytes: &[u8],
) -> GameProxyTopology {
    let mut topology = outer_topology(root, game_id);
    topology.downstream = Some(ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path: host.clone(),
        receipt: match ownership {
            FileOwnership::Owned => {
                FileReceipt::owned("host", digest(host_bytes)).expect("receipt")
            }
            FileOwnership::Reused => {
                FileReceipt::reused("host", digest(host_bytes)).expect("receipt")
            }
        },
    });
    topology.downstream_origin = Some(topology.root_slot.clone());
    topology
}

fn active_topology(root: &Path, game_id: &GameId) -> GameProxyTopology {
    let host = path(root, "ReShade64.dll");
    host_topology(root, game_id, &host, FileOwnership::Reused, b"foreign host")
}

fn addon_record(root: &Path, game_id: &GameId) -> (InstalledAddon, PathRef) {
    let addon = path(root, "Luma.addon");
    std::fs::write(addon.as_str(), b"addon").expect("addon");
    (
        InstalledAddon::new(game_id.clone(), AddonKind::Luma, addon.clone()),
        addon,
    )
}

fn release(binding: &ManagedAddonFile, action: CoordinatedFilePlan) -> PlannedManagedDlssRelease {
    PlannedManagedDlssRelease::new(
        binding,
        PlannedDlss {
            action,
            binding: None,
        },
    )
}

fn remove_plan(path: &PathRef, installed: &[u8]) -> CoordinatedFilePlan {
    CoordinatedFilePlan::RemoveAndRelease {
        path: PathBuf::from(path.as_str()),
        expected_live: vec![digest(installed)],
    }
}

fn restore_plan(path: &PathRef, installed: &[u8], baseline: &[u8]) -> CoordinatedFilePlan {
    CoordinatedFilePlan::RestoreAndRelease {
        path: PathBuf::from(path.as_str()),
        baseline_sha256: digest(baseline),
        expected_live: vec![digest(installed)],
    }
}

fn compose(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    authority: &LumaPeerRootAuthority,
    game_id: &GameId,
    releases: &[PlannedManagedDlssRelease],
) -> Result<UninstallCompositionParts, crate::ServiceError> {
    let cascade = empty_cascade(game_id);
    compose_active_uninstall(record, topology, authority, &cascade, releases)
        .map(|composition| composition.into_parts())
}

#[test]
fn owned_host_absent_removes_host_and_clears_topology() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("owned-host-absent");
    let host_path = path(root.path(), "ReShade64.dll");
    std::fs::write(host_path.as_str(), b"host").expect("host");
    let claim = ManagedAddonFile::owned(
        host_path.clone(),
        ManagedFileBaseline::Absent,
        digest(b"host"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record.try_with_managed_files(vec![claim]).expect("record");
    let topology = host_topology(root.path(), &id, &host_path, FileOwnership::Owned, b"host");

    let (program, payloads, planned) =
        compose(&record, &topology, &authority(root.path()), &id, &[]).expect("compose");
    assert_eq!(program.endpoints().len(), 2);
    assert_eq!(payloads, vec![None, None]);
    let PlannedGameProxyTopology::Exact(after) = planned else {
        panic!("expected exact topology")
    };
    assert!(after.downstream.is_none());
    assert!(after.downstream_origin.is_none());
    assert_eq!(after.root_prestate, ProxyRootPrestate::Absent);
    let host_endpoint = program
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.path() == &host_path)
        .expect("host endpoint");
    assert!(matches!(
        host_endpoint.after(),
        EndpointPostcondition::Absent
    ));
}

#[test]
fn owned_host_present_restores_baseline_and_clears_topology() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("owned-host-present");
    let host_path = path(root.path(), "ReShade64.dll");
    std::fs::write(host_path.as_str(), b"managed host").expect("host");
    std::fs::write(format!("{}.bak", host_path.as_str()), b"foreign host").expect("sidecar");
    let claim = ManagedAddonFile::owned(
        host_path.clone(),
        ManagedFileBaseline::Present {
            sha256: digest(b"foreign host"),
        },
        digest(b"managed host"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record.try_with_managed_files(vec![claim]).expect("record");
    let topology = host_topology(
        root.path(),
        &id,
        &host_path,
        FileOwnership::Owned,
        b"managed host",
    );

    let (program, payloads, planned) =
        compose(&record, &topology, &authority(root.path()), &id, &[]).expect("compose");
    assert_eq!(program.endpoints().len(), 3);
    assert_eq!(payloads.len(), 3);
    let host = program
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.path() == &host_path)
        .expect("host endpoint");
    assert!(
        matches!(host.after(), EndpointPostcondition::File(hash) if hash == &digest(b"foreign host"))
    );
    let sidecar = program
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.path().as_str().ends_with("ReShade64.dll.bak"))
        .expect("sidecar endpoint");
    assert!(matches!(sidecar.after(), EndpointPostcondition::Absent));
    assert!(
        matches!(planned, PlannedGameProxyTopology::Exact(after) if after.downstream.is_none())
    );
}

#[test]
fn reused_host_and_unclaimed_host_keep_topology_without_host_endpoint() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("reused-host");
    let host_path = path(root.path(), "ReShade64.dll");
    std::fs::write(host_path.as_str(), b"foreign host").expect("host");
    let (record, addon_path) = addon_record(root.path(), &id);
    let topology = host_topology(
        root.path(),
        &id,
        &host_path,
        FileOwnership::Reused,
        b"foreign host",
    );

    let reused = ManagedAddonFile::reused(host_path.clone(), digest(b"foreign host"));
    let record = record.try_with_managed_files(vec![reused]).expect("record");
    let (program, _, planned) =
        compose(&record, &topology, &authority(root.path()), &id, &[]).expect("reused compose");
    assert_eq!(program.endpoints().len(), 1);
    assert_eq!(program.endpoints()[0].path(), &addon_path);
    assert_eq!(planned, PlannedGameProxyTopology::Exact(topology));

    let (unclaimed, _) = addon_record(root.path(), &game_id("unclaimed-host"));
    let unclaimed_id = unclaimed.game_id().clone();
    let unclaimed_topology = host_topology(
        root.path(),
        &unclaimed_id,
        &host_path,
        FileOwnership::Reused,
        b"foreign host",
    );
    let (program, _, planned) = compose(
        &unclaimed,
        &unclaimed_topology,
        &authority(root.path()),
        &unclaimed_id,
        &[],
    )
    .expect("unclaimed compose");
    assert_eq!(program.endpoints().len(), 1);
    assert_eq!(planned, PlannedGameProxyTopology::Exact(unclaimed_topology));
}

#[test]
fn external_addon_path_generic_engine_file_succeeds() {
    let root = tempfile::tempdir().expect("root");
    let payload = tempfile::tempdir().expect("payload");
    std::fs::write(
        root.path().join("ReShade.ini"),
        format!("[ADDON]\r\nAddonPath={}\r\n", payload.path().display()),
    )
    .expect("ini");
    let id = game_id("external-payload");
    let addon = path(payload.path(), "Luma.addon");
    std::fs::write(addon.as_str(), b"addon").expect("addon");
    let record = InstalledAddon::new(id.clone(), AddonKind::Luma, addon.clone());
    let topology = active_topology(root.path(), &id);

    let (program, payloads, _) =
        compose(&record, &topology, &authority(root.path()), &id, &[]).expect("compose");
    assert_eq!(program.endpoints().len(), 1);
    assert_eq!(program.endpoints()[0].path(), &addon);
    assert_eq!(payloads, vec![None]);
}

#[test]
fn owned_dlss_remove_and_restore_are_lowered_directly() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("dlss");
    let dlss_path = path(root.path(), "nvngx_dlss.dll");
    std::fs::write(dlss_path.as_str(), b"managed dlss").expect("dlss");
    let claim = ManagedAddonFile::owned(
        dlss_path.clone(),
        ManagedFileBaseline::Absent,
        digest(b"managed dlss"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record
        .try_with_managed_files(vec![claim.clone()])
        .expect("record");
    let topology = active_topology(root.path(), &id);
    let remove = release(&claim, remove_plan(&dlss_path, b"managed dlss"));
    let (program, payloads, _) =
        compose(&record, &topology, &authority(root.path()), &id, &[remove])
            .expect("remove compose");
    assert_eq!(program.endpoints().len(), 2);
    let dlss = program
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.path() == &dlss_path)
        .expect("dlss endpoint");
    assert!(matches!(dlss.after(), EndpointPostcondition::Absent));
    assert!(payloads.iter().all(Option::is_none));

    let dlss_path = path(root.path(), "nvngx_dlss.dll");
    std::fs::write(dlss_path.as_str(), b"managed dlss").expect("dlss");
    std::fs::write(format!("{}.bak", dlss_path.as_str()), b"foreign dlss").expect("sidecar");
    let claim = ManagedAddonFile::owned(
        dlss_path.clone(),
        ManagedFileBaseline::Present {
            sha256: digest(b"foreign dlss"),
        },
        digest(b"managed dlss"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record
        .try_with_managed_files(vec![claim.clone()])
        .expect("record");
    let restore = release(
        &claim,
        restore_plan(&dlss_path, b"managed dlss", b"foreign dlss"),
    );
    let (program, _, _) = compose(&record, &topology, &authority(root.path()), &id, &[restore])
        .expect("restore compose");
    assert_eq!(program.endpoints().len(), 3);
    let dlss = program
        .endpoints()
        .iter()
        .find(|endpoint| endpoint.path() == &dlss_path)
        .expect("dlss endpoint");
    assert!(
        matches!(dlss.after(), EndpointPostcondition::File(hash) if hash == &digest(b"foreign dlss"))
    );
}

#[test]
fn reused_dlss_keep_is_a_noop() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("reused-dlss");
    let dlss_path = path(root.path(), "nvngx_dlss.dll");
    std::fs::write(dlss_path.as_str(), b"foreign dlss").expect("dlss");
    let claim = ManagedAddonFile::reused(dlss_path, digest(b"foreign dlss"));
    let (record, addon) = addon_record(root.path(), &id);
    let record = record
        .try_with_managed_files(vec![claim.clone()])
        .expect("record");
    let keep = release(&claim, CoordinatedFilePlan::Keep);

    let (program, payloads, _) = compose(
        &record,
        &active_topology(root.path(), &id),
        &authority(root.path()),
        &id,
        &[keep],
    )
    .expect("compose");
    assert_eq!(program.endpoints().len(), 1);
    assert_eq!(program.endpoints()[0].path(), &addon);
    assert_eq!(payloads, vec![None]);
}

#[test]
fn release_coverage_rejects_missing_duplicate_extraneous_and_mismatched_claims() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("coverage");
    let dlss_path = path(root.path(), "nvngx_dlss.dll");
    std::fs::write(dlss_path.as_str(), b"managed").expect("dlss");
    let claim = ManagedAddonFile::owned(
        dlss_path.clone(),
        ManagedFileBaseline::Absent,
        digest(b"managed"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record
        .try_with_managed_files(vec![claim.clone()])
        .expect("record");
    let topology = active_topology(root.path(), &id);
    let valid = release(&claim, remove_plan(&dlss_path, b"managed"));

    assert!(compose(&record, &topology, &authority(root.path()), &id, &[]).is_err());
    assert!(
        compose(
            &record,
            &topology,
            &authority(root.path()),
            &id,
            &[valid.clone(), valid],
        )
        .is_err()
    );

    let other_path = path(root.path(), "other");
    let other = ManagedAddonFile::owned(
        other_path.clone(),
        ManagedFileBaseline::Absent,
        digest(b"other"),
    );
    let extraneous = release(&other, remove_plan(&other_path, b"other"));
    assert!(
        compose(
            &record,
            &topology,
            &authority(root.path()),
            &id,
            &[extraneous],
        )
        .is_err()
    );

    let mismatched = ManagedAddonFile::owned(
        dlss_path.clone(),
        ManagedFileBaseline::Absent,
        digest(b"different"),
    );
    let mismatched = release(&mismatched, remove_plan(&dlss_path, b"different"));
    assert!(
        compose(
            &record,
            &topology,
            &authority(root.path()),
            &id,
            &[mismatched],
        )
        .is_err()
    );
}

#[test]
fn topology_overlap_and_host_digest_mismatch_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("overlap");
    let root_slot = path(root.path(), "dxgi.dll");
    std::fs::write(root_slot.as_str(), b"addon").expect("root slot");
    let record = InstalledAddon::new(id.clone(), AddonKind::Luma, root_slot);
    let topology = active_topology(root.path(), &id);
    assert!(compose(&record, &topology, &authority(root.path()), &id, &[]).is_err());

    let host_path = path(root.path(), "ReShade64.dll");
    std::fs::write(host_path.as_str(), b"host").expect("host");
    let claim = ManagedAddonFile::owned(
        host_path.clone(),
        ManagedFileBaseline::Absent,
        digest(b"host"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record.try_with_managed_files(vec![claim]).expect("record");
    let mut topology = host_topology(root.path(), &id, &host_path, FileOwnership::Owned, b"host");
    topology.downstream.as_mut().expect("downstream").receipt =
        FileReceipt::owned("host", digest(b"wrong")).expect("receipt");
    assert!(compose(&record, &topology, &authority(root.path()), &id, &[]).is_err());
}

#[test]
fn inactive_luma_topology_is_rejected_before_lowering() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("inactive-topology");
    let (record, _) = addon_record(root.path(), &id);
    let authority = authority(root.path());

    assert!(
        compose(
            &record,
            &outer_topology(root.path(), &id),
            &authority,
            &id,
            &[],
        )
        .is_err()
    );

    let mut non_optiscaler = active_topology(root.path(), &id);
    non_optiscaler.outer.implementation = ProxyImplementation::ReShade;
    assert!(compose(&record, &non_optiscaler, &authority, &id, &[]).is_err());

    let mut non_reshade = active_topology(root.path(), &id);
    non_reshade
        .downstream
        .as_mut()
        .expect("downstream")
        .implementation = ProxyImplementation::SpecialK;
    assert!(compose(&record, &non_reshade, &authority, &id, &[]).is_err());
}

#[test]
fn finalized_program_has_deterministic_endpoint_and_payload_cardinality() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("cardinality");
    let (record, addon) = addon_record(root.path(), &id);
    let topology = active_topology(root.path(), &id);
    let (program, payloads, _) =
        compose(&record, &topology, &authority(root.path()), &id, &[]).expect("compose");
    assert_eq!(program.endpoints().len(), payloads.len());
    assert_eq!(program.endpoints()[0].path(), &addon);
    assert!(matches!(
        program.endpoints()[0].before(),
        EndpointExpectation::File(_)
    ));
}

#[test]
fn catalog_consumed_owned_dlss_requires_keep_and_skips_direct_release() {
    let root = tempfile::tempdir().expect("root");
    let id = game_id("catalog-consumed");
    let dlss_path = path(root.path(), "nvngx_dlss.dll");
    std::fs::write(dlss_path.as_str(), b"managed dlss").expect("dlss");
    std::fs::write(format!("{}.bak", dlss_path.as_str()), b"foreign dlss").expect("sidecar");
    let claim = ManagedAddonFile::owned(
        dlss_path.clone(),
        ManagedFileBaseline::Present {
            sha256: digest(b"foreign dlss"),
        },
        digest(b"managed dlss"),
    );
    let (record, _) = addon_record(root.path(), &id);
    let record = record
        .try_with_managed_files(vec![claim.clone()])
        .expect("record");

    let storage = SqliteStorage::in_memory().expect("storage");
    storage
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(id.clone(), "catalog game", Launcher::Manual).expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            path(root.path(), ""),
        ))
        .expect("game");
    storage.upsert_installed_addon(&record).expect("addon");
    let component_id = ComponentId::new("component:catalog-consumed").expect("component id");
    let component = LibraryComponent::new(
        component_id.clone(),
        id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(dlss_path.clone()).with_sha256(digest(b"managed dlss")));
    storage
        .replace_components_for_game(&id, std::slice::from_ref(&component))
        .expect("component");
    storage
        .recover_component_rollback_baseline(
            &id,
            &component_id,
            &ComponentRollbackBaseline::new(vec![
                ComponentFile::new(dlss_path.clone()).with_sha256(digest(b"foreign dlss")),
            ]),
        )
        .expect("baseline");
    let cascade = cascade_for_managed_paths(&storage, &id, &[PathBuf::from(dlss_path.as_str())])
        .expect("cascade");
    assert!(cascade.catalog_claim().is_some());

    let release = release(&claim, CoordinatedFilePlan::Keep);
    let composition = compose_active_uninstall(
        &record,
        &active_topology(root.path(), &id),
        &authority(root.path()),
        &cascade,
        vec![release],
    )
    .expect("catalog-consumed compose");
    let (program, payloads, _) = composition.into_parts();
    assert_eq!(program.endpoints().len(), 3);
    assert_eq!(program.endpoints().len(), payloads.len());
    assert_eq!(
        program
            .endpoints()
            .iter()
            .filter(|endpoint| endpoint.path() == &dlss_path)
            .count(),
        1
    );
}
