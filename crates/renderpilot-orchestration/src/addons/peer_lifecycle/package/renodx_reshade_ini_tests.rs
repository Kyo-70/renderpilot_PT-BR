use super::*;
use renderpilot_domain::{LibraryComponent, RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature};

fn typed_authority(
    root: &std::path::Path,
    feature: RenoDxReshadeIniFeature,
) -> RenoDxReshadeIniAuthority {
    RenoDxReshadeIniAuthority::new(feature, path(root)).expect("authority")
}

#[test]
fn active_package_binds_typed_renodx_reshade_ini_to_the_sealed_game_root() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let canonical_root =
        crate::paths::canonicalize_existing(game.path()).expect("canonical game root");
    let ini = path(&canonical_root.join("ReShade.ini"));
    let addon_file = path(&game.path().join("renodx.addon"));
    let payload = b"[GENERAL]\nPerformanceMode=1\n".to_vec();
    let authority = typed_authority(&canonical_root, RenoDxReshadeIniFeature::Install);
    let after_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::RenoDx,
        addon_file.clone(),
    )
    .with_created_file(ini.clone());
    let program = ExactEndpointProgram::new(vec![
        crate::peer_mutation_executor::ExactEndpoint::new(
            addon_file,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"addon")),
        ),
        crate::peer_mutation_executor::ExactEndpoint::new(
            ini,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(&payload)),
        ),
    ])
    .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());

    let package = PeerMutationPackage::plan_active_with_renodx_reshade_ini(
        PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: None,
            after_peer: Some(&after_peer),
            before_topology: &topology,
            planned_after_topology: &planned,
            program,
            payloads: vec![Some(b"addon".to_vec()), Some(payload)],
            game_root: game.path().to_path_buf(),
            payload_root: None,
            component_set: None::<&[LibraryComponent]>,
            baseline_mutations: &[],
            catalog_claim: None,
        },
        authority,
    )
    .expect("typed package");

    assert_eq!(
        package
            .renodx_reshade_ini_authority()
            .expect("authority")
            .feature(),
        RenoDxReshadeIniFeature::Install
    );
    assert!(package.catalog_claim().is_none());
}

#[test]
fn untyped_active_package_rejects_typed_endpoint() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let ini = path(&game.path().join("ReShade.ini"));
    let after_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::RenoDx,
        path(&game.path().join("renodx.addon")),
    )
    .with_created_file(ini.clone());
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            ini,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"ini")),
        )])
        .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let error = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::RenoDx,
        before_peer: None,
        after_peer: Some(&after_peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![Some(b"ini".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    })
    .expect_err("untyped route cannot carry typed endpoint");
    assert!(error.to_string().contains("invalid endpoint role"));
}

#[test]
fn typed_package_rejects_wrong_root_authority() {
    let game = tempfile::tempdir().expect("game");
    let other = tempfile::tempdir().expect("other");
    let topology = outer_topology(game.path());
    let game_root = crate::paths::canonicalize_existing(game.path()).expect("game root");
    let ini = path(&game_root.join("ReShade.ini"));
    let after_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::RenoDx,
        path(&game.path().join("renodx.addon")),
    )
    .with_created_file(ini.clone());
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            ini,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"ini")),
        )])
        .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let error = PeerMutationPackage::plan_active_with_renodx_reshade_ini(
        PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: None,
            after_peer: Some(&after_peer),
            before_topology: &topology,
            planned_after_topology: &planned,
            program,
            payloads: vec![Some(b"ini".to_vec())],
            game_root: game.path().to_path_buf(),
            payload_root: None,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
        },
        typed_authority(other.path(), RenoDxReshadeIniFeature::Install),
    )
    .expect_err("wrong root must be rejected");
    assert!(
        error
            .to_string()
            .contains("differs from the sealed game root")
    );
}

#[test]
fn typed_package_rejects_missing_typed_endpoint() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let canonical_root = crate::paths::canonicalize_existing(game.path()).expect("game root");
    let addon = path(&game.path().join("renodx.addon"));
    let after_peer =
        InstalledAddon::new(topology.game_id.clone(), AddonKind::RenoDx, addon.clone());
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            addon,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"addon")),
        )])
        .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let error = PeerMutationPackage::plan_active_with_renodx_reshade_ini(
        PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: None,
            after_peer: Some(&after_peer),
            before_topology: &topology,
            planned_after_topology: &planned,
            program,
            payloads: vec![Some(b"addon".to_vec())],
            game_root: game.path().to_path_buf(),
            payload_root: None,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
        },
        typed_authority(&canonical_root, RenoDxReshadeIniFeature::Install),
    )
    .expect_err("missing typed endpoint must be rejected");
    assert!(error.to_string().contains("RenoDX ReShade.ini"));
}

#[test]
fn typed_package_rejects_duplicate_typed_endpoint_and_path_drift() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let canonical_root = crate::paths::canonicalize_existing(game.path()).expect("game root");
    let ini = path(&canonical_root.join("ReShade.ini"));
    let drift = path(&canonical_root.join("nested").join("ReShade.ini"));
    let after_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::RenoDx,
        path(&game.path().join("renodx.addon")),
    )
    .with_created_file(ini.clone());
    let program = ExactEndpointProgram::new(vec![
        crate::peer_mutation_executor::ExactEndpoint::new(
            ini,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"ini")),
        ),
        crate::peer_mutation_executor::ExactEndpoint::new(
            drift,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"drift")),
        ),
    ])
    .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let error = PeerMutationPackage::plan_active_with_renodx_reshade_ini(
        PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: None,
            after_peer: Some(&after_peer),
            before_topology: &topology,
            planned_after_topology: &planned,
            program,
            payloads: vec![Some(b"ini".to_vec()), Some(b"drift".to_vec())],
            game_root: game.path().to_path_buf(),
            payload_root: None,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
        },
        typed_authority(&canonical_root, RenoDxReshadeIniFeature::Install),
    )
    .expect_err("duplicate/path-drift typed endpoint must be rejected");
    assert!(error.to_string().contains("exactly one") || error.to_string().contains("ReShade.ini"));
}

#[test]
fn typed_package_rejects_catalog_projection() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let canonical_root = crate::paths::canonicalize_existing(game.path()).expect("game root");
    let ini = path(&canonical_root.join("ReShade.ini"));
    let authority = typed_authority(&canonical_root, RenoDxReshadeIniFeature::Install);
    let after_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::RenoDx,
        path(&game.path().join("renodx.addon")),
    )
    .with_created_file(ini.clone());
    let components: &[LibraryComponent] = &[];
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            ini,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"ini")),
        )])
        .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let error = PeerMutationPackage::plan_active_with_renodx_reshade_ini(
        PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: None,
            after_peer: Some(&after_peer),
            before_topology: &topology,
            planned_after_topology: &planned,
            program,
            payloads: vec![Some(b"ini".to_vec())],
            game_root: game.path().to_path_buf(),
            payload_root: None,
            component_set: Some(components),
            baseline_mutations: &[],
            catalog_claim: None,
        },
        authority,
    )
    .expect_err("typed route cannot carry catalog projection");
    assert!(error.to_string().contains("catalog projection"));
}
