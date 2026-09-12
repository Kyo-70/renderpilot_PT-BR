use super::*;
use crate::FileSafetyAuthority;
use crate::addons::optiscaler::types::OptiScalerArchiveMember;
use crate::addons::optiscaler::{archive, manifest_store};
use renderpilot_application::GameRepository;
use renderpilot_domain::{
    FileOwnership, GameIdentity, GameInstallation, GameRuntime, Launcher, ManagedAddonFile,
    Platform,
};
use std::path::Path;
use tempfile::tempdir;

mod peer;
mod retained_fsr;
mod roundtrip;
mod smoke;

#[test]
fn managed_descendant_allows_missing_child_but_not_a_missing_retained_root() {
    let root = tempdir().expect("root");
    let target = root.path().join("D3D12_Optiscaler").join("D3D12Core.dll");

    assert!(
        super::maybe_exact_managed_receipt_from_live(root.path(), &target, FileOwnership::Owned,)
            .expect("missing managed descendant")
            .is_none()
    );

    let missing_root = root.path().join("missing-root");
    assert!(
        super::maybe_exact_managed_receipt_from_live(
            &missing_root,
            &missing_root.join("D3D12Core.dll"),
            FileOwnership::Owned,
        )
        .is_err()
    );
}
