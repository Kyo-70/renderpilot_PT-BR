use super::*;
use renderpilot_domain::FileOwnership;

#[test]
fn native_runtime_is_always_private_to_optiscaler() {
    let target = Path::new(r"C:\Games\Example");
    let path = native_module_download_target("nvidia_sr", target, "nvngx_dlss.dll")
        .expect("reviewed native module placement");
    assert_eq!(path, target.join("OptiScaler").join("nvngx_dlss.dll"));
}

#[test]
fn unknown_native_module_has_no_implicit_install_path() {
    assert!(native_module_download_target("unknown", Path::new("game"), "x.dll").is_err());
}

#[test]
fn chained_update_reuses_committed_downstream_without_rediscovering_source() {
    let downstream = PathBuf::from("game").join("ReShade64.dll");
    let proxy = EvaluatedProxyPlan {
        slot: PathBuf::from("game").join("dxgi.dll"),
        chain_reshade: true,
        downstream_path: Some(downstream.clone()),
        conflict: None,
        reshade_source_path: None,
        reshade_source_sha256: None,
    };

    assert!(matches!(
        evaluated_downstream_install_plan(&proxy, true, FileOwnership::Reused)
            .expect("existing chain"),
        Some(crate::addons::proxy_chain::DownstreamInstallPlan::Existing {
            destination_path,
        }) if destination_path == downstream
    ));
    assert!(evaluated_downstream_install_plan(&proxy, false, FileOwnership::Reused).is_err());
}
