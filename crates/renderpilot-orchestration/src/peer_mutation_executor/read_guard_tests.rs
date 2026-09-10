use std::path::Path;

use renderpilot_domain::PathRef;

use super::{EndpointObservation, observe_peer_path};

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
}

#[test]
fn absent_leaf_and_missing_parent_are_absent() {
    let root = tempfile::tempdir().expect("root");
    let direct = root.path().join("missing.dll");
    let nested = root.path().join("missing").join("nested.dll");

    assert_eq!(
        observe_peer_path(&path_ref(&direct)).expect("direct absence"),
        EndpointObservation::Absent
    );
    assert_eq!(
        observe_peer_path(&path_ref(&nested)).expect("missing parent absence"),
        EndpointObservation::Absent
    );
}

#[test]
fn present_file_uses_the_verified_read_observation_for_identity_digest_and_length() {
    let root = tempfile::tempdir().expect("root");
    let file = root.path().join("stable.dll");
    let bytes = b"stable peer bytes";
    std::fs::write(&file, bytes).expect("write");

    let first = observe_peer_path(&path_ref(&file)).expect("first observation");
    let second = observe_peer_path(&path_ref(&file)).expect("second observation");
    let EndpointObservation::File(first) = first else {
        panic!("present file was absent")
    };
    let EndpointObservation::File(second) = second else {
        panic!("present file was absent")
    };
    assert_eq!(first.length(), bytes.len() as u64);
    assert_eq!(first.identity(), second.identity());
    assert_eq!(first.digest(), second.digest());
}

#[test]
fn directory_leaf_is_rejected() {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("directory.dll");
    std::fs::create_dir(&directory).expect("directory");

    assert!(observe_peer_path(&path_ref(&directory)).is_err());
}

#[test]
fn symlink_leaf_is_rejected_without_following_it() {
    let root = tempfile::tempdir().expect("root");
    let target = root.path().join("target.dll");
    let link = root.path().join("link.dll");
    std::fs::write(&target, b"target").expect("target");

    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&target, &link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&target, &link);
    if result.is_err() {
        // Windows CI may not grant the unprivileged symlink capability.
        return;
    }

    assert!(observe_peer_path(&path_ref(&link)).is_err());
}
