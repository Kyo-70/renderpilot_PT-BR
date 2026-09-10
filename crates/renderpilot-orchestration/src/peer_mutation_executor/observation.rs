//! Retained-handle observations shared by peer apply and crash recovery.

use std::fs;
use std::path::Path;

use renderpilot_domain::PathRef;

use crate::{ServiceError, addons::errors};

use super::VerifiedPeerFile;

/// One no-follow observation of a peer endpoint.  A present file keeps the
/// verified parent and leaf used to obtain its observation, while an absent
/// leaf keeps the same authority whenever its parent already exists.  This
/// lets a caller perform an exact operation without falling back to a
/// pathname-only write.
pub(crate) enum PeerPathObservation {
    Absent {
        parent: Option<(crate::fs::VerifiedDir, crate::fs::LeafName)>,
    },
    File {
        parent: crate::fs::VerifiedDir,
        leaf: crate::fs::LeafName,
        bytes: Vec<u8>,
        observation: crate::fs::EntryObservation,
    },
}

/// Read-only projection of one retained-handle observation.
///
/// Unlike [`PeerPathObservation`], this type deliberately does not expose
/// directory or file handles.  A present file owns the bytes returned by the
/// same no-follow read and the exact identity/digest/length derived from that
/// read.  Callers can borrow the bytes, but cannot mutate the snapshot.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PeerPathSnapshot {
    Absent,
    File(PeerFileSnapshot),
}

/// Opaque present-file projection.  Its owned bytes and metadata are private;
/// callers can only borrow them through [`PeerPathSnapshot`] getters.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PeerFileSnapshot {
    file: VerifiedPeerFile,
    bytes: Vec<u8>,
}

impl PeerPathSnapshot {
    pub(crate) fn file(&self) -> Option<&VerifiedPeerFile> {
        match self {
            Self::Absent => None,
            Self::File(snapshot) => Some(&snapshot.file),
        }
    }

    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Absent => None,
            Self::File(snapshot) => Some(&snapshot.bytes),
        }
    }
}

pub(crate) fn observe_peer_path(path: &PathRef) -> Result<PeerPathObservation, ServiceError> {
    let display_path = Path::new(path.as_str());
    let (parent, leaf) = match crate::fs::verified_parent(display_path) {
        Ok(value) => value,
        Err(_error) if endpoint_parent_is_absent(display_path)? => {
            return Ok(PeerPathObservation::Absent { parent: None });
        }
        Err(error) => {
            return Err(errors::invalid(format!(
                "failed to observe peer path {}: {error}",
                path.as_str()
            )));
        }
    };
    let Some(observation) = parent.observe_leaf(&leaf)? else {
        return Ok(PeerPathObservation::Absent {
            parent: Some((parent, leaf)),
        });
    };
    if observation.kind != crate::fs::EntryKind::File {
        return Err(errors::invalid(format!(
            "peer path is not a regular file: {}",
            path.as_str()
        )));
    }
    let (bytes, observed) = parent.read_regular_file(&leaf, Some(&observation))?;
    if observed.kind != crate::fs::EntryKind::File || observed.digest.is_none() {
        return Err(errors::invalid(format!(
            "peer path did not yield a regular-file digest: {}",
            path.as_str()
        )));
    }
    Ok(PeerPathObservation::File {
        parent,
        leaf,
        bytes,
        observation: observed,
    })
}

/// Observe one peer path under an explicit lexical authority root.
///
/// The containment check is intentionally performed before invoking the
/// filesystem observer.  The observer itself remains the sole no-follow read
/// and hash operation; this function only projects its retained result.
pub(crate) fn observe_peer_path_snapshot(
    path: &PathRef,
    authorized_root: &PathRef,
) -> Result<PeerPathSnapshot, ServiceError> {
    let display_path = Path::new(path.as_str());
    let root = Path::new(authorized_root.as_str());
    if !crate::paths::is_within(display_path, root) {
        return Err(errors::invalid(format!(
            "peer path is outside the authorized root: {}",
            path.as_str()
        )));
    }

    match observe_peer_path(path)? {
        PeerPathObservation::Absent { .. } => Ok(PeerPathSnapshot::Absent),
        PeerPathObservation::File {
            bytes, observation, ..
        } => {
            let digest = observation.digest.ok_or_else(|| {
                errors::invalid(format!(
                    "peer path has no digest in its retained observation: {}",
                    path.as_str()
                ))
            })?;
            let length = u64::try_from(bytes.len()).map_err(|error| {
                errors::invalid(format!("peer endpoint length overflow: {error}"))
            })?;
            let digest = renderpilot_domain::Sha256Hash::new(digest)
                .map_err(|error| errors::invalid(error.to_string()))?;
            let file = VerifiedPeerFile::new_with_length(observation.identity, digest, length)
                .map_err(|error| errors::invalid(error.to_string()))?;
            Ok(PeerPathSnapshot::File(PeerFileSnapshot { file, bytes }))
        }
    }
}

/// Missing parents are safe only when the entire missing suffix is below a
/// verified, existing directory.  Existing parents are opened component by
/// component without following links or reparses.
fn endpoint_parent_is_absent(path: &Path) -> Result<bool, ServiceError> {
    let mut current = path
        .parent()
        .ok_or_else(|| errors::invalid(format!("peer path has no parent: {}", path.display())))?;
    let mut missing = false;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(errors::invalid(format!(
                    "peer path parent is a link or reparse point: {}",
                    current.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(errors::invalid(format!(
                    "peer path parent is not a directory: {}",
                    current.display()
                )));
            }
            Ok(_) => {
                crate::fs::VerifiedDir::open_absolute_components(current, None).map_err(
                    |error| {
                        errors::invalid(format!(
                            "peer path parent is unsafe or unreadable: {}: {error}",
                            current.display()
                        ))
                    },
                )?;
                return Ok(missing);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
            }
            Err(error) => {
                return Err(errors::io("inspect peer endpoint parent", current, &error));
            }
        }
        let Some(parent) = current.parent() else {
            return Ok(missing);
        };
        if parent == current {
            return Ok(missing);
        }
        current = parent;
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{
        PeerPathObservation, PeerPathSnapshot, observe_peer_path, observe_peer_path_snapshot,
    };
    use renderpilot_domain::{PathRef, Sha256Hash};

    fn path_ref(path: &std::path::Path) -> PathRef {
        PathRef::new(path.to_string_lossy().into_owned()).expect("path")
    }

    #[test]
    fn regular_file_snapshot_keeps_coherent_bytes_identity_digest_and_length() {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("peer.dll");
        let bytes = b"peer snapshot bytes";
        std::fs::write(&path, bytes).expect("file");

        let snapshot =
            observe_peer_path_snapshot(&path_ref(&path), &path_ref(root.path())).expect("snapshot");
        let PeerPathSnapshot::File(_) = &snapshot else {
            panic!("regular file was absent")
        };
        let file = snapshot.file().expect("file metadata");
        assert_eq!(snapshot.bytes().expect("file bytes"), bytes);
        assert!(!file.identity().is_empty());
        assert_eq!(file.length(), bytes.len() as u64);
        let expected = Sha256Hash::new(hex::encode(Sha256::digest(bytes))).expect("digest");
        assert_eq!(file.digest(), &expected);
    }

    #[test]
    fn absent_leaf_and_missing_suffix_are_absent() {
        let root = tempfile::tempdir().expect("root");
        let direct = root.path().join("missing.dll");
        let nested = root.path().join("missing").join("nested.dll");
        let root_ref = path_ref(root.path());

        assert!(matches!(
            observe_peer_path_snapshot(&path_ref(&direct), &root_ref).expect("direct absence"),
            PeerPathSnapshot::Absent
        ));
        assert!(matches!(
            observe_peer_path_snapshot(&path_ref(&nested), &root_ref).expect("nested absence"),
            PeerPathSnapshot::Absent
        ));
    }

    #[test]
    fn absent_leaf_retains_existing_parent_authority() {
        let root = tempfile::tempdir().expect("root");
        let direct = root.path().join("missing.dll");
        let nested = root.path().join("missing").join("nested.dll");

        let PeerPathObservation::Absent {
            parent: Some((parent, leaf)),
        } = observe_peer_path(&path_ref(&direct)).expect("direct observation")
        else {
            panic!("existing parent authority was not retained")
        };
        assert!(parent.observe_leaf(&leaf).expect("direct leaf").is_none());

        let PeerPathObservation::Absent { parent: None } =
            observe_peer_path(&path_ref(&nested)).expect("nested observation")
        else {
            panic!("missing suffix unexpectedly retained a parent")
        };
    }

    #[test]
    fn outside_authorized_root_is_rejected_before_observation() {
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let path = outside.path().join("outside.dll");
        std::fs::write(&path, b"outside").expect("file");

        assert!(observe_peer_path_snapshot(&path_ref(&path), &path_ref(root.path())).is_err());
    }

    #[test]
    fn directory_leaf_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        let directory = root.path().join("directory.dll");
        std::fs::create_dir(&directory).expect("directory");

        assert!(observe_peer_path_snapshot(&path_ref(&directory), &path_ref(root.path())).is_err());
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

        assert!(observe_peer_path_snapshot(&path_ref(&link), &path_ref(root.path())).is_err());
    }
}
