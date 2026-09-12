use super::*;

#[test]
fn leaf_name_rejects_non_leaf_components() {
    assert!(LeafName::parse(OsStr::new("")).is_err());
    assert!(LeafName::parse(OsStr::new(".")).is_err());
    assert!(LeafName::parse(OsStr::new("..")).is_err());
    assert!(LeafName::parse(OsStr::new("a/b")).is_err());
    assert!(LeafName::parse(OsStr::new("a\\b")).is_err());
    assert!(LeafName::parse(OsStr::new("safe")).is_ok());
}

#[test]
fn hostile_same_uid_preflight_is_side_effect_free() {
    assert!(AuthorityMode::HostileSameUid.preflight().is_err());
}

fn capability() -> [u8; 32] {
    [0x5a; 32]
}

#[test]
fn private_leaf_binding_is_exact() {
    let capability = capability();
    let leaf = LeafName::from_capability("private", &capability).expect("leaf");
    assert!(leaf.is_bound_to_capability(&capability));
    assert!(!leaf.is_bound_to_capability(&[0x6b; 32]));
    assert!(
        !LeafName::parse(OsStr::new("private5a"))
            .expect("leaf")
            .is_bound_to_capability(&capability)
    );
    let uppercase = format!("private-{}", hex::encode(capability).to_uppercase());
    assert!(
        !LeafName::parse(OsStr::new(&uppercase))
            .expect("leaf")
            .is_bound_to_capability(&capability)
    );
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn retained_file_read_returns_same_handle_observation() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::parse(OsStr::new("stage")).expect("stage leaf");
    assert!(
        authority
            .create_file_no_replace(&name, b"must not be created", AuthorityMode::HostileSameUid)
            .is_err()
    );
    assert!(!root.path().join(name.as_os_str()).exists());
    let result = authority
        .create_file_no_replace(&name, b"retained bytes", AuthorityMode::CooperativeSameUid)
        .expect("create retained file");
    let CreateFileNoReplace::Created {
        _entry: entry,
        observation,
    } = result
    else {
        panic!("new file unexpectedly reported as occupied");
    };
    let (bytes, reread) = entry
        .read_regular_file(Some(&observation))
        .expect("read retained file");
    assert_eq!(bytes, b"retained bytes");
    assert_eq!(reread, observation);
    assert!(matches!(
        authority.create_file_no_replace(
            &name,
            b"replacement must not win",
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(CreateFileNoReplace::Occupied)
    ));
    let reserved = authority
        .enumerate_reserved_children(std::slice::from_ref(&name))
        .expect("enumerate reserved child");
    assert_eq!(reserved.len(), 1);
    assert_eq!(reserved[0].name, name);
    assert_eq!(reserved[0].observation, observation);
    drop(entry);
    authority
        .remove_exact(&name, &observation, AuthorityMode::CooperativeSameUid)
        .expect("cleanup retained file");
}

#[cfg(target_os = "linux")]
#[test]
fn retained_file_read_and_hash_preserve_shared_file_offset() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::parse(OsStr::new("cursor-preservation")).expect("leaf");
    let initial_data = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let CreateFileNoReplace::Created {
        _entry: entry,
        observation,
    } = authority
        .create_file_no_replace(&name, initial_data, AuthorityMode::CooperativeSameUid)
        .expect("create retained file")
    else {
        panic!("file unexpectedly reported as occupied");
    };

    let initial_offset = 10_u64;
    let seek_result = rustix::fs::seek(&entry.fd, rustix::fs::SeekFrom::Start(initial_offset))
        .expect("seek retained fd");
    assert_eq!(seek_result, initial_offset);

    let digest = super::platform::linux::hash_fd(&entry.fd).expect("hash_fd");
    assert_eq!(digest, hex::encode(sha2::Sha256::digest(initial_data)));

    let offset_after_hash = rustix::fs::seek(&entry.fd, rustix::fs::SeekFrom::Current(0))
        .expect("tell cursor after hash");
    assert_eq!(offset_after_hash, initial_offset);

    let (bytes, reread) = entry
        .read_regular_file(Some(&observation))
        .expect("read_regular_file");
    assert_eq!(bytes, initial_data);
    assert_eq!(reread, observation);

    let offset_after_read = rustix::fs::seek(&entry.fd, rustix::fs::SeekFrom::Current(0))
        .expect("tell cursor after read");
    assert_eq!(offset_after_read, initial_offset);

    drop(entry);
    authority
        .remove_exact(&name, &observation, AuthorityMode::CooperativeSameUid)
        .expect("cleanup retained file");
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn bounded_retained_file_read_accepts_limit_and_rejects_oversize() {
    const MAX_BYTES: usize = 16 * 1024 * 1024;

    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");

    let exact_name = LeafName::parse(OsStr::new("exact-boundary")).expect("exact leaf");
    let exact_bytes = vec![0xA5; MAX_BYTES];
    let CreateFileNoReplace::Created {
        _entry: exact_entry,
        observation: exact_observation,
    } = authority
        .create_file_no_replace(&exact_name, &exact_bytes, AuthorityMode::CooperativeSameUid)
        .expect("create exact-boundary file")
    else {
        panic!("exact-boundary file unexpectedly existed");
    };
    let (read_exact, observed_exact) = exact_entry
        .read_regular_file_bounded(Some(&exact_observation), MAX_BYTES)
        .expect("read exact-boundary file");
    assert_eq!(read_exact, exact_bytes);
    assert_eq!(observed_exact, exact_observation);

    let oversized_name = LeafName::parse(OsStr::new("oversized")).expect("oversized leaf");
    let oversized_bytes = vec![0x5A; MAX_BYTES + 1];
    let CreateFileNoReplace::Created {
        _entry: oversized_entry,
        observation: oversized_observation,
    } = authority
        .create_file_no_replace(
            &oversized_name,
            &oversized_bytes,
            AuthorityMode::CooperativeSameUid,
        )
        .expect("create oversized file")
    else {
        panic!("oversized file unexpectedly existed");
    };
    let error = oversized_entry
        .read_regular_file_bounded(Some(&oversized_observation), MAX_BYTES)
        .expect_err("oversized file must fail before allocation");
    assert!(error.to_string().contains("bounded read limit"));

    drop(exact_entry);
    drop(oversized_entry);
    authority
        .remove_exact(
            &exact_name,
            &exact_observation,
            AuthorityMode::CooperativeSameUid,
        )
        .expect("cleanup exact-boundary file");
    authority
        .remove_exact(
            &oversized_name,
            &oversized_observation,
            AuthorityMode::CooperativeSameUid,
        )
        .expect("cleanup oversized file");
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn retained_overwrite_reapplies_after_same_identity_digest_advance() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::parse(OsStr::new("retained")).expect("retained leaf");
    let CreateFileNoReplace::Created { observation, .. } = authority
        .create_file_no_replace(&name, b"S", AuthorityMode::CooperativeSameUid)
        .expect("create retained file")
    else {
        panic!("retained file unexpectedly existed");
    };
    let advanced = authority
        .overwrite_regular_file(&name, &observation, b"X")
        .expect("advance retained digest");
    assert_eq!(advanced.identity, observation.identity);
    let repaired = authority
        .overwrite_regular_file_with_stable_identity(&name, &advanced, &observation.identity, b"L")
        .expect("reapply through retained identity");
    assert_eq!(repaired.identity, observation.identity);
    assert_eq!(
        std::fs::read(root.path().join(name.as_os_str())).expect("repaired bytes"),
        b"L"
    );
    assert!(
        authority
            .overwrite_regular_file_with_stable_identity(
                &name,
                &repaired,
                "foreign-identity",
                b"must not write",
            )
            .is_err()
    );
    assert_eq!(
        std::fs::read(root.path().join(name.as_os_str())).expect("stable bytes"),
        b"L"
    );
    authority
        .remove_exact(&name, &repaired, AuthorityMode::CooperativeSameUid)
        .expect("cleanup retained file");
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn relative_directory_creation_is_exact_and_no_replace() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::parse(OsStr::new("candidate")).expect("candidate leaf");

    let hostile = authority.create_directory_no_replace(&name, AuthorityMode::HostileSameUid);
    assert!(hostile.is_err());
    assert!(
        authority
            .observe_leaf(&name)
            .expect("hostile observation")
            .is_none()
    );

    let foreign_name = LeafName::parse(OsStr::new("foreign")).expect("foreign leaf");
    let foreign_path = root.path().join(foreign_name.as_os_str());
    std::fs::write(&foreign_path, b"preserve this entry").expect("foreign file");
    assert!(
        authority
            .create_directory_no_replace(&foreign_name, AuthorityMode::CooperativeSameUid)
            .is_err()
    );
    assert_eq!(
        std::fs::read(&foreign_path).expect("foreign bytes"),
        b"preserve this entry"
    );

    let directory = authority
        .create_directory_no_replace(&name, AuthorityMode::CooperativeSameUid)
        .expect("create directory");
    let created = authority
        .observe_leaf(&name)
        .expect("created observation")
        .expect("created directory");
    assert!(created.is_directory());
    assert_eq!(created.identity, directory.identity());

    assert!(
        authority
            .create_directory_no_replace(&name, AuthorityMode::CooperativeSameUid)
            .is_err()
    );
    let after_occupied = authority
        .observe_leaf(&name)
        .expect("occupied observation")
        .expect("occupied directory");
    assert_eq!(after_occupied.identity, directory.identity());
    let identity = directory.identity().to_owned();
    drop(directory);

    assert!(matches!(
        authority
            .remove_empty_dir(&name, &identity, AuthorityMode::CooperativeSameUid,)
            .expect("cleanup directory"),
        RemoveEmptyDir::Removed
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn relative_directory_creation_is_owner_only_and_effective_uid_owned() {
    use std::os::unix::fs::MetadataExt;

    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::parse(OsStr::new("permissions")).expect("permissions leaf");
    let directory = authority
        .create_directory_no_replace(&name, AuthorityMode::CooperativeSameUid)
        .expect("create directory");
    let metadata = std::fs::symlink_metadata(directory.metadata_path()).expect("metadata");
    assert_eq!(metadata.mode() & 0o777, 0o700);
    assert_eq!(metadata.uid(), linux_effective_uid());
    assert!(linux_identity_is_stable(metadata.dev(), metadata.ino()));
    assert_eq!(
        directory.identity(),
        linux_identity(metadata.dev(), metadata.ino())
    );
    assert!(matches!(
        authority
            .remove_empty_dir(
                &name,
                directory.identity(),
                AuthorityMode::CooperativeSameUid,
            )
            .expect("cleanup directory"),
        RemoveEmptyDir::Removed
    ));
}

#[cfg(windows)]
#[test]
fn relative_directory_creation_installs_protected_owner_only_security() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::parse(OsStr::new("security")).expect("security leaf");
    let directory = authority
        .create_directory_no_replace(&name, AuthorityMode::CooperativeSameUid)
        .expect("create directory");
    windows_verify_private_security(&directory.handle).expect("owner-only protected DACL");
    let identity = directory.identity().to_owned();
    drop(directory);
    assert!(matches!(
        authority
            .remove_empty_dir(&name, &identity, AuthorityMode::CooperativeSameUid,)
            .expect("cleanup directory"),
        RemoveEmptyDir::Removed
    ));
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn control_namespace_leaf_binds_transaction_and_capability() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let cap = capability();
    let namespace = ControlNamespace::create(
        &authority,
        "transaction-42",
        &cap,
        AuthorityMode::CooperativeSameUid,
    )
    .expect("control namespace");
    let identity = namespace.identity().to_owned();
    assert_eq!(namespace.transaction_id(), "transaction-42");
    assert_eq!(namespace.capability(), &cap);
    assert!(
        ControlNamespace::reopen(&authority, "transaction-42", &[0x6b; 32], &identity,).is_err()
    );
    assert!(
        ControlNamespace::reopen(&authority, "transaction-42", &cap, "wrong-identity").is_err()
    );
    drop(namespace);
    let reopened = ControlNamespace::reopen(&authority, "transaction-42", &cap, &identity)
        .expect("reopen control namespace");
    reopened
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("cleanup control namespace");
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn reserved_enumeration_rejects_unknown_direct_children() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let cap = capability();
    let namespace = ControlNamespace::create(
        &authority,
        "enumeration",
        &cap,
        AuthorityMode::CooperativeSameUid,
    )
    .expect("control namespace");
    std::fs::write(namespace.metadata_path().join("unknown"), b"foreign").expect("foreign child");
    let reserved = LeafName::parse(OsStr::new("allowed")).expect("reserved leaf");
    assert!(
        namespace
            .enumerate_reserved_children(std::slice::from_ref(&reserved))
            .is_err()
    );
    std::fs::remove_file(namespace.metadata_path().join("unknown")).expect("foreign cleanup");
    namespace
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("cleanup control namespace");
}

#[test]
fn absolute_reopen_requires_the_exact_final_identity() {
    let root = tempfile::tempdir().expect("temporary root");
    assert!(VerifiedDir::open_absolute_components(root.path(), Some("wrong")).is_err());
}

#[cfg(windows)]
#[test]
fn private_namespace_windows_creation_is_atomic_and_owner_only() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::from_capability("private", &capability()).expect("leaf");
    assert!(
            authority
                .create_private_namespace_with_mode(
                    &name,
                    &capability(),
                    AuthorityMode::HostileSameUid,
                )
                .is_err()
        );
    assert!(!root.path().join(name.as_os_str()).exists());
    let namespace = authority
        .create_private_namespace(&name, &capability())
        .expect("private namespace");
    windows_verify_private_security(&namespace.dir.handle).expect("owner-only DACL");
    namespace
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("cleanup");
}

#[cfg(windows)]
#[test]
fn windows_staged_publication_inherits_the_destination_parent_dacl() {
    let private_root = tempfile::tempdir().expect("private root");
    let public_root = tempfile::tempdir().expect("public root");
    let private_parent = VerifiedDir::open(private_root.path()).expect("private parent");
    let public_parent = VerifiedDir::open(public_root.path()).expect("public parent");
    let namespace_leaf = LeafName::from_capability("stage", &capability()).expect("namespace leaf");
    let namespace = private_parent
        .create_private_namespace(&namespace_leaf, &capability())
        .expect("private namespace");
    let staged_leaf = LeafName::parse(OsStr::new("staged.dll")).expect("staged leaf");
    let CreateFileNoReplace::Created {
        _entry: staged_entry,
        observation,
    } = namespace
        .directory()
        .create_file_no_replace(
            &staged_leaf,
            b"published bytes",
            AuthorityMode::CooperativeSameUid,
        )
        .expect("stage file")
    else {
        panic!("fresh stage leaf was occupied");
    };
    drop(staged_entry);
    let destination_leaf = LeafName::parse(OsStr::new("published.dll")).expect("destination leaf");
    assert!(matches!(
        namespace.directory().publish_staged_no_replace(
            &staged_leaf,
            &public_parent,
            &destination_leaf,
            &observation,
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(RenameNoReplace::Moved)
    ));
    let published = public_parent
        .observe_leaf(&destination_leaf)
        .expect("observe published")
        .expect("published entry");
    assert_eq!(published.digest.as_deref(), observation.digest.as_deref());
    let published_path = public_root.path().join(destination_leaf.as_os_str());
    let published_handle = std::fs::File::open(&published_path).expect("open published file");
    assert!(windows_verify_private_security(&published_handle).is_err());
    drop(published_handle);
    std::fs::write(&published_path, b"ordinary rewrite").expect("ordinary rewrite published");
    public_parent
        .remove_exact(
            &destination_leaf,
            &public_parent
                .observe_leaf(&destination_leaf)
                .expect("observe rewritten published")
                .expect("rewritten published entry"),
            AuthorityMode::CooperativeSameUid,
        )
        .expect("remove published");

    let staged_directory_leaf =
        LeafName::parse(OsStr::new("staged-directory")).expect("staged directory leaf");
    let staged_directory = namespace
        .directory()
        .create_directory_no_replace(&staged_directory_leaf, AuthorityMode::CooperativeSameUid)
        .expect("stage directory");
    drop(staged_directory);
    let staged_directory_observation = namespace
        .directory()
        .observe_leaf(&staged_directory_leaf)
        .expect("observe staged directory")
        .expect("staged directory entry");
    assert!(staged_directory_observation.is_directory());
    let published_directory_leaf =
        LeafName::parse(OsStr::new("published-directory")).expect("published directory leaf");
    assert!(matches!(
        namespace.directory().publish_staged_no_replace(
            &staged_directory_leaf,
            &public_parent,
            &published_directory_leaf,
            &staged_directory_observation,
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(RenameNoReplace::Moved)
    ));
    let published_directory = public_root
        .path()
        .join(published_directory_leaf.as_os_str());
    let ordinary_child = published_directory.join("ordinary-child.txt");
    std::fs::write(&ordinary_child, b"ordinary child").expect("ordinary child create");
    std::fs::remove_file(&ordinary_child).expect("ordinary child remove");
    std::fs::remove_dir(&published_directory).expect("ordinary directory remove");
    namespace
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("remove private namespace");
}

#[cfg(windows)]
#[test]
fn windows_exact_tokens_and_native_empty_check_preserve_replacements() {
    let root = tempfile::tempdir().expect("temporary root");
    let destination = tempfile::tempdir().expect("temporary destination");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let destination_authority =
        VerifiedDir::open(destination.path()).expect("destination authority");

    let live = root.path().join("live");
    let replacement = root.path().join("replacement");
    std::fs::write(&live, b"same bytes").expect("live");
    std::fs::write(&replacement, b"same bytes").expect("replacement");
    let live_leaf = LeafName::parse(OsStr::new("live")).expect("live leaf");
    let before = authority
        .observe_leaf(&live_leaf)
        .expect("observation")
        .expect("live entry");
    std::fs::remove_file(&live).expect("remove live");
    std::fs::rename(&replacement, &live).expect("replace live");
    assert!(
        authority
            .remove_exact(&live_leaf, &before, AuthorityMode::CooperativeSameUid)
            .is_err()
    );
    assert!(live.exists());

    let source = root.path().join("source");
    let target = destination.path().join("target");
    std::fs::write(&source, b"source").expect("source");
    std::fs::write(&target, b"target").expect("target");
    let source_leaf = LeafName::parse(OsStr::new("source")).expect("source leaf");
    let target_leaf = LeafName::parse(OsStr::new("target")).expect("target leaf");
    assert!(matches!(
        authority.rename_no_replace(
            &source_leaf,
            &destination_authority,
            &target_leaf,
            None,
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(RenameNoReplace::Occupied)
    ));
    assert!(source.exists());
    assert!(target.exists());

    let directory = root.path().join("nonempty");
    std::fs::create_dir(&directory).expect("directory");
    std::fs::write(directory.join("child"), b"child").expect("child");
    let directory_leaf = LeafName::parse(OsStr::new("nonempty")).expect("directory leaf");
    let observation = authority
        .observe_leaf(&directory_leaf)
        .expect("directory observation")
        .expect("directory");
    assert!(matches!(
        authority.remove_empty_dir(
            &directory_leaf,
            &observation.identity,
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(RemoveEmptyDir::NotEmpty)
    ));
    assert!(directory.join("child").exists());
}

#[cfg(windows)]
#[test]
fn windows_retained_parent_blocks_external_namespace_mutation_until_drop() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let retained_path = root.path().to_owned();
    let renamed = retained_path.with_file_name("retained-parent-renamed");
    assert!(std::fs::rename(&retained_path, &renamed).is_err());
    assert!(std::fs::remove_dir(&retained_path).is_err());
    drop(authority);
    std::fs::rename(&retained_path, &renamed).expect("rename after parent handle drop");
    std::fs::remove_dir(renamed).expect("cleanup");
}

#[cfg(windows)]
#[test]
fn windows_created_directory_blocks_external_namespace_mutation_until_drop() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let leaf = LeafName::parse(OsStr::new("created")).expect("created leaf");
    let retained = authority
        .create_directory_no_replace(&leaf, AuthorityMode::CooperativeSameUid)
        .expect("created directory");
    let created_path = root.path().join(leaf.as_os_str());
    let renamed = root.path().join("created-renamed");

    assert!(std::fs::rename(&created_path, &renamed).is_err());
    assert!(std::fs::remove_dir(&created_path).is_err());

    drop(retained);
    std::fs::rename(&created_path, &renamed).expect("rename after child handle drop");
    std::fs::remove_dir(renamed).expect("cleanup");
}

#[cfg(target_os = "linux")]
#[test]
fn ancestor_symlink_is_rejected_before_capability_acquisition() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("temporary root");
    let real = root.path().join("real");
    std::fs::create_dir(&real).expect("real directory");
    let link = root.path().join("link");
    symlink(&real, &link).expect("ancestor symlink");
    assert!(VerifiedDir::open_absolute_components(&link, None).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn private_namespace_is_owner_only_and_hostile_preflight_has_no_artifact() {
    use std::os::unix::fs::MetadataExt;

    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let name = LeafName::from_capability("private", &capability()).expect("leaf");
    let hostile = authority.create_private_namespace_with_mode(
        &name,
        &capability(),
        AuthorityMode::HostileSameUid,
    );
    assert!(hostile.is_err());
    assert!(!root.path().join(name.as_os_str()).exists());

    let namespace = authority
        .create_private_namespace(&name, &capability())
        .expect("private namespace");
    let metadata = std::fs::symlink_metadata(namespace.metadata_path()).expect("metadata");
    assert_eq!(metadata.mode() & 0o777, 0o700);
    assert_eq!(metadata.uid(), linux_effective_uid());
    namespace
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("cleanup");
}

#[cfg(target_os = "linux")]
#[test]
fn wrong_identity_reopen_and_equal_bytes_replacement_are_rejected() {
    let root = tempfile::tempdir().expect("temporary root");
    let authority = VerifiedDir::open(root.path()).expect("root authority");
    let first = LeafName::from_capability("first", &capability()).expect("first leaf");
    let second = LeafName::from_capability("second", &capability()).expect("second leaf");
    let namespace = authority
        .create_private_namespace(&first, &capability())
        .expect("private namespace");
    assert!(PrivateNamespace::reopen(&authority, &first, "unix:wrong").is_err());
    let second_namespace = authority
        .create_private_namespace(&second, &capability())
        .expect("second namespace");
    second_namespace
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("second cleanup");
    namespace
        .remove_empty(AuthorityMode::CooperativeSameUid)
        .expect("first cleanup");

    let live = root.path().join("live");
    let replacement = root.path().join("replacement");
    std::fs::write(&live, b"same bytes").expect("live");
    std::fs::write(&replacement, b"same bytes").expect("replacement");
    let leaf = LeafName::parse(OsStr::new("live")).expect("live leaf");
    let before = authority
        .observe_leaf(&leaf)
        .expect("observation")
        .expect("live entry");
    std::fs::remove_file(&live).expect("remove live");
    std::fs::rename(&replacement, &live).expect("replace live");
    assert!(
        authority
            .remove_exact(&leaf, &before, AuthorityMode::CooperativeSameUid)
            .is_err()
    );
    assert!(live.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn occupied_rename_preserves_both_entries_and_nonempty_remove_is_authoritative() {
    let root = tempfile::tempdir().expect("temporary root");
    let destination = tempfile::tempdir().expect("temporary destination");
    let source_authority = VerifiedDir::open(root.path()).expect("source authority");
    let destination_authority =
        VerifiedDir::open(destination.path()).expect("destination authority");
    let source = root.path().join("source");
    let target = destination.path().join("target");
    std::fs::write(&source, b"source").expect("source");
    std::fs::write(&target, b"target").expect("target");
    let source_leaf = LeafName::parse(OsStr::new("source")).expect("source leaf");
    let target_leaf = LeafName::parse(OsStr::new("target")).expect("target leaf");
    assert!(matches!(
        source_authority.rename_no_replace(
            &source_leaf,
            &destination_authority,
            &target_leaf,
            None,
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(RenameNoReplace::Occupied)
    ));
    assert!(source.exists());
    assert!(target.exists());

    let directory = root.path().join("nonempty");
    std::fs::create_dir(&directory).expect("directory");
    std::fs::write(directory.join("child"), b"child").expect("child");
    let directory_leaf = LeafName::parse(OsStr::new("nonempty")).expect("directory leaf");
    let observation = source_authority
        .observe_leaf(&directory_leaf)
        .expect("directory observation")
        .expect("directory");
    assert!(matches!(
        source_authority.remove_empty_dir(
            &directory_leaf,
            &observation.identity,
            AuthorityMode::CooperativeSameUid,
        ),
        Ok(RemoveEmptyDir::NotEmpty)
    ));
    assert!(directory.join("child").exists());
}
