use super::*;
use tempfile::tempdir;

#[test]
fn drifted_owned_outer_is_rejected_before_mutation() {
    assert_drifted_outer_is_rejected(FileOwnership::Owned);
}

#[test]
fn drifted_reused_outer_is_rejected_before_mutation() {
    assert_drifted_outer_is_rejected(FileOwnership::Reused);
}

fn assert_drifted_outer_is_rejected(ownership: FileOwnership) {
    let root = tempdir().expect("root");
    let proxy = root.path().join("dxgi.dll");
    let downstream = root.path().join("ReShade64.dll");
    let outer = b"same bytes, new identity";
    std::fs::write(&proxy, outer).expect("old outer");
    std::fs::write(&downstream, b"peer").expect("downstream");
    let prior = exact_receipt_from_live(&proxy, ownership).expect("prior receipt");
    let downstream_receipt =
        exact_receipt_from_live(&downstream, FileOwnership::Reused).expect("peer receipt");
    let replacement = root.path().join("replacement.dll");
    std::fs::write(&replacement, outer).expect("new outer");
    std::fs::remove_file(&proxy).expect("replace outer");
    std::fs::rename(&replacement, &proxy).expect("install new outer");

    let topology = GameProxyTopology {
        id: "optiscaler:identity-drift".to_owned(),
        game_id: GameId::new("manual:identity-drift").expect("game id"),
        root_slot: path_ref(&proxy).expect("root path"),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: path_ref(&proxy).expect("proxy path"),
            receipt: prior,
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: path_ref(&downstream).expect("downstream path"),
            receipt: downstream_receipt,
        }),
        downstream_origin: Some(path_ref(&proxy).expect("origin path")),
        root_prestate: ProxyRootPrestate::RelocatedDownstream,
    };

    let error = revalidate_for_removal(&topology)
        .expect_err("same-bytes identity drift must fail closed before journal/IO");
    assert!(matches!(error, ServiceError::CommandFailed(_)));
    assert_eq!(std::fs::read(&proxy).expect("live outer"), outer);
}
