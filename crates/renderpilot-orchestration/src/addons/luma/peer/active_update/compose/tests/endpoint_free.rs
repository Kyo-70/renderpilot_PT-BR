use std::path::Path;

use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, LibraryComponent,
    LibraryTechnology, ManagedAddonFile, PeerCatalogDeletedBaseline, PeerCatalogRollbackClaim,
    PlannedGameProxyTopology, Swappability,
};

use crate::{
    addons::luma::peer::active_update::{
        compose::{EndpointFreeRoute, select_endpoint_free_route},
        error::LumaActiveUpdateError,
        model::LumaActiveUpdateMtime,
    },
    catalog::cascade::{CascadeResult, ValidatedRollbackPlan},
};

use super::{game, hash, path, record, topology};

fn base() -> (
    renderpilot_domain::InstalledAddon,
    renderpilot_domain::GameProxyTopology,
) {
    let root = Path::new("C:/Games/Test");
    let game_id = game();
    let topology = topology(root, &game_id);
    (record(root, &game_id, Vec::new()), topology)
}

fn empty_cascade() -> CascadeResult {
    CascadeResult::empty_for_test()
}

fn select<'a>(
    before: &'a renderpilot_domain::InstalledAddon,
    topology: &'a renderpilot_domain::GameProxyTopology,
    after: &'a renderpilot_domain::InstalledAddon,
    planned: &'a PlannedGameProxyTopology,
    cascade: &'a CascadeResult,
    mtime: Option<&'a LumaActiveUpdateMtime>,
) -> Result<EndpointFreeRoute, LumaActiveUpdateError> {
    select_endpoint_free_route(before, topology, after, planned, cascade, mtime)
}

#[test]
fn exact_unchanged_record_is_noop() {
    let (before, topology) = base();
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    assert_eq!(
        select(
            &before,
            &topology,
            &before,
            &planned,
            &empty_cascade(),
            None,
        )
        .expect("noop"),
        EndpointFreeRoute::Noop
    );
}

#[test]
fn changed_metadata_with_exact_topology_is_metadata_route() {
    let (before, topology) = base();
    let after = before.clone().with_addon_version("refresh");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    assert_eq!(
        select(&before, &topology, &after, &planned, &empty_cascade(), None,).expect("metadata"),
        EndpointFreeRoute::Metadata
    );
}

#[test]
fn reused_membership_add_remove_and_replace_are_aggregate_routes() {
    let root = Path::new("C:/Games/Test");
    let game_id = game();
    let topology = topology(root, &game_id);
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let empty = record(root, &game_id, Vec::new());
    let reused_a = ManagedAddonFile::reused(path(root, "ReShade64.dll"), hash('b'));
    let reused_b = ManagedAddonFile::reused(path(root, "nvngx_dlss.dll"), hash('c'));
    let one = record(root, &game_id, vec![reused_a]);
    let two = record(root, &game_id, vec![reused_b]);

    assert_eq!(
        select(&empty, &topology, &one, &planned, &empty_cascade(), None).expect("add"),
        EndpointFreeRoute::AggregateMembership
    );
    assert_eq!(
        select(&one, &topology, &empty, &planned, &empty_cascade(), None).expect("remove"),
        EndpointFreeRoute::AggregateMembership
    );
    assert_eq!(
        select(&one, &topology, &two, &planned, &empty_cascade(), None)
            .expect("simultaneous remove/add"),
        EndpointFreeRoute::AggregateMembership
    );
}

#[test]
fn endpoint_free_route_rejects_topology_drift() {
    let (before, topology) = base();
    let mut changed = topology.clone();
    changed.outer.receipt =
        renderpilot_domain::FileReceipt::owned("other", hash('c')).expect("receipt");
    let planned = PlannedGameProxyTopology::Exact(changed);
    let error = select(
        &before,
        &topology,
        &before,
        &planned,
        &empty_cascade(),
        None,
    )
    .expect_err("topology drift");
    assert!(error.is_endpoint_free_topology());
}

#[test]
fn endpoint_free_route_rejects_mtime_without_effects() {
    let (before, topology) = base();
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let mtime = LumaActiveUpdateMtime::new(path(Path::new("C:/Games/Test"), "Luma.addon64"), None);
    let error = select(
        &before,
        &topology,
        &before,
        &planned,
        &empty_cascade(),
        Some(&mtime),
    )
    .expect_err("mtime");
    assert!(error.is_endpoint_free_mtime());
}

#[test]
fn endpoint_free_route_rejects_owned_managed_changes() {
    let (before, topology) = base();
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let after = record(
        Path::new("C:/Games/Test"),
        before.game_id(),
        vec![ManagedAddonFile::owned(
            path(Path::new("C:/Games/Test"), "owned.dll"),
            renderpilot_domain::ManagedFileBaseline::Absent,
            hash('d'),
        )],
    );
    let error = select(&before, &topology, &after, &planned, &empty_cascade(), None)
        .expect_err("owned membership");
    assert!(error.is_domain());
}

fn catalog_claim() -> PeerCatalogRollbackClaim {
    let game_id = game();
    let target = path(Path::new("C:/Games/Test"), "nvngx_dlss.dll");
    let id = ComponentId::new("catalog:dlss").expect("component id");
    let component = LibraryComponent::new(
        id.clone(),
        game_id,
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(target.clone()).with_sha256(hash('c')));
    let baseline =
        ComponentRollbackBaseline::new(vec![ComponentFile::new(target).with_sha256(hash('d'))]);
    PeerCatalogRollbackClaim::new(
        vec![component],
        vec![PeerCatalogDeletedBaseline::new(id, baseline)],
    )
    .expect("catalog claim")
}

#[test]
fn endpoint_free_route_rejects_catalog_claim_rollback_and_mutation_paths() {
    let (before, topology) = base();
    let planned = PlannedGameProxyTopology::Exact(topology.clone());

    let claim = catalog_claim();
    let claim_cascade = CascadeResult::from_parts_for_test(Vec::new(), Some(claim), Vec::new());
    let error = select(&before, &topology, &before, &planned, &claim_cascade, None)
        .expect_err("catalog claim");
    assert!(error.is_endpoint_free_cascade());

    let target = path(Path::new("C:/Games/Test"), "nvngx_dlss.dll");
    let component = LibraryComponent::new(
        ComponentId::new("catalog:rollback").expect("component id"),
        game(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(target.clone()).with_sha256(hash('c')));
    let baseline =
        ComponentRollbackBaseline::new(vec![ComponentFile::new(target).with_sha256(hash('d'))]);
    let rollback = CascadeResult::from_parts_for_test(
        vec![ValidatedRollbackPlan::for_test(component, baseline)],
        None,
        Vec::new(),
    );
    let error =
        select(&before, &topology, &before, &planned, &rollback, None).expect_err("rollback specs");
    assert!(error.is_endpoint_free_cascade());

    let mut paths = CascadeResult::empty_for_test();
    paths
        .mutation_paths
        .push(std::path::PathBuf::from("C:/Games/Test/nvngx_dlss.dll"));
    let error =
        select(&before, &topology, &before, &planned, &paths, None).expect_err("mutation paths");
    assert!(error.is_endpoint_free_cascade());
}
