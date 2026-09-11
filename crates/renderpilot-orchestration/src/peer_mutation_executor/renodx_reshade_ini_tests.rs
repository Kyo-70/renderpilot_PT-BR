use renderpilot_domain::{
    AddonKind, InstalledAddon, RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature,
};

use super::timing_tests::{Fixture, assert_prepared, fixture, hash, path_ref};
use super::{EndpointExpectation, EndpointPostcondition, ExactEndpoint, ExactEndpointProgram};
use crate::addons::peer_lifecycle::package::{PeerMutationPackage, PeerMutationRequest};
use crate::game_mutation_lock;

fn typed_renodx_after_peer(fixture: &Fixture) -> InstalledAddon {
    InstalledAddon::new(
        fixture.game_id.clone(),
        AddonKind::RenoDx,
        path_ref(&fixture.game_root.join("renodx.addon64")),
    )
    .with_created_file(path_ref(&fixture.game_root.join("ReShade.ini")))
}

fn typed_renodx_package<'a>(
    fixture: &'a Fixture,
    after_peer: &'a InstalledAddon,
) -> PeerMutationPackage<'a> {
    let addon = path_ref(&fixture.game_root.join("renodx.addon64"));
    let ini = path_ref(&fixture.game_root.join("ReShade.ini"));
    let program = ExactEndpointProgram::new(vec![
        ExactEndpoint::new(
            addon,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"renodx-addon")),
        ),
        ExactEndpoint::new(
            ini,
            renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"renodx-ini")),
        ),
    ])
    .expect("typed RenoDX program");
    let authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        path_ref(&fixture.game_root),
    )
    .expect("typed RenoDX authority");
    PeerMutationPackage::plan_active_with_renodx_reshade_ini(
        PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: None,
            after_peer: Some(after_peer),
            before_topology: &fixture.topology,
            planned_after_topology: &fixture.planned,
            program,
            payloads: vec![Some(b"renodx-addon".to_vec()), Some(b"renodx-ini".to_vec())],
            game_root: fixture.game_root.clone(),
            payload_root: None,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
        },
        authority,
    )
    .expect("typed RenoDX package")
}

#[test]
fn ordinary_typed_renodx_feature_mismatch_fails_before_reservation() {
    let fixture = fixture();
    let after_peer = typed_renodx_after_peer(&fixture);
    let package = typed_renodx_package(&fixture, &after_peer);
    let guard = game_mutation_lock::try_lock(&fixture.game_id).expect("guard");

    let result = fixture
        .context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer(
            &fixture.context,
            &guard,
            renderpilot_domain::mutation_features::LUMA_INSTALL,
            None,
            package,
        );
    let error = result
        .err()
        .expect("feature mismatch must fail before reservation");

    assert!(error.to_string().contains("does not match"));
    assert!(
        fixture
            .context
            .storage()
            .pending_file_mutations_for_game(&fixture.game_id)
            .expect("pending rows")
            .is_empty()
    );
}

#[test]
fn ordinary_typed_renodx_forwards_authority_to_storage_preflight() {
    let fixture = fixture();
    let after_peer = typed_renodx_after_peer(&fixture);
    let package = typed_renodx_package(&fixture, &after_peer);
    let guard = game_mutation_lock::try_lock(&fixture.game_id).expect("guard");

    let prepared = fixture
        .context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer(
            &fixture.context,
            &guard,
            renderpilot_domain::mutation_features::RENODX_INSTALL,
            None,
            package,
        )
        .expect("typed ordinary preparation");

    assert_prepared(&fixture);
    let manifest = fixture
        .context
        .storage()
        .pending_file_mutations_for_game(&fixture.game_id)
        .expect("pending rows")
        .into_iter()
        .next()
        .expect("prepared row")
        .manifest_json;
    assert!(manifest.contains(r#""role":"renodx_reshade_ini""#));
    drop(prepared);
}
