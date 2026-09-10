use super::*;
use crate::peer_mutation_executor::{ExactEndpoint, VerifiedPeerFile};
use renderpilot_domain::{PathRef, PeerEndpointRole, Sha256Hash};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn digest() -> Sha256Hash {
    Sha256Hash::new("a".repeat(64)).expect("digest")
}

fn path_ref(path: &Path) -> PathRef {
    let path = path.to_string_lossy().replace('\\', "/");
    PathRef::new(&path).expect("path")
}

fn create(path: &Path) -> (ExactEndpoint, EndpointObservation) {
    (
        ExactEndpoint::new(
            path_ref(path),
            PeerEndpointRole::Disjoint,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(digest()),
        ),
        EndpointObservation::Absent,
    )
}

fn existing_file() -> VerifiedPeerFile {
    VerifiedPeerFile::new_with_length("existing".to_owned(), digest(), 1).expect("file")
}

fn replace(path: &Path, remove: bool) -> (ExactEndpoint, EndpointObservation) {
    (
        ExactEndpoint::new(
            path_ref(path),
            PeerEndpointRole::Disjoint,
            EndpointExpectation::File(existing_file()),
            if remove {
                EndpointPostcondition::Absent
            } else {
                EndpointPostcondition::File(digest())
            },
        ),
        EndpointObservation::File(existing_file()),
    )
}

fn make_program(
    endpoints: Vec<(ExactEndpoint, EndpointObservation)>,
) -> (ExactEndpointProgram, Vec<EndpointObservation>) {
    let before = endpoints.iter().map(|(_, before)| before.clone()).collect();
    let program = ExactEndpointProgram::new(
        endpoints
            .into_iter()
            .map(|(endpoint, _)| endpoint)
            .collect(),
    )
    .expect("program");
    (program, before)
}

#[test]
fn nested_shared_create_chain_is_deduplicated_and_parent_first() {
    let root = tempdir().expect("root");
    let first = root.path().join("nested/deeper/one.dll");
    let second = root.path().join("nested/deeper/two.dll");
    let (program, before) = make_program(vec![create(&first), create(&second)]);
    let plan =
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]).expect("plan");

    assert_eq!(plan.ancestors().len(), 2);
    assert_eq!(plan.ancestors()[0].path(), root.path().join("nested"));
    assert_eq!(
        plan.ancestors()[1].path(),
        root.path().join("nested/deeper")
    );
    assert_eq!(plan.ancestors()[1].consumers(), &[0, 1]);
    assert_eq!(plan.endpoint_chain(0), Some(&[0, 1][..]));
    assert_eq!(plan.endpoint_chain(1), Some(&[0, 1][..]));
    let entries = plan.manifest_entries();
    assert_eq!(
        entries[0].path(),
        path_ref(&root.path().join("nested")).as_str()
    );
    assert_eq!(entries[0].consumer_ordinals(), &[0, 1]);
    assert_eq!(
        entries[1].path(),
        path_ref(&root.path().join("nested").join("deeper")).as_str()
    );
    assert_eq!(entries[1].consumer_ordinals(), &[0, 1]);
    assert!(!root.path().join("nested").exists());
}

#[test]
fn roots_are_ordered_before_depth_and_key() {
    let first_root = tempdir().expect("first root");
    let second_root = tempdir().expect("second root");
    let (program, before) = make_program(vec![
        create(&second_root.path().join("new/file.dll")),
        create(&first_root.path().join("new/deeper/file.dll")),
    ]);
    let plan = PeerAncestorPlan::derive(
        &program,
        &before,
        &[first_root.path().to_owned(), second_root.path().to_owned()],
    )
    .expect("plan");
    assert_eq!(plan.ancestors()[0].path(), first_root.path().join("new"));
    assert_eq!(
        plan.ancestors()[1].path(),
        first_root.path().join("new/deeper")
    );
    assert_eq!(plan.ancestors()[2].path(), second_root.path().join("new"));
}

#[test]
fn depth_order_uses_the_declared_root_even_after_existing_parents() {
    let root = tempdir().expect("root");
    std::fs::create_dir(root.path().join("existing")).expect("existing");
    let deep = root.path().join("existing/missing/deeper/file.dll");
    let shallow = root.path().join("shallow/file.dll");
    let (program, before) = make_program(vec![create(&deep), create(&shallow)]);
    let plan =
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]).expect("plan");
    assert_eq!(plan.ancestors()[0].path(), root.path().join("shallow"));
    assert_eq!(
        plan.ancestors()[1].path(),
        root.path().join("existing/missing")
    );
    assert_eq!(
        plan.ancestors()[2].path(),
        root.path().join("existing/missing/deeper")
    );
}

#[test]
fn replace_and_remove_reject_missing_parent() {
    let root = tempdir().expect("root");
    for remove in [false, true] {
        let target = root.path().join("missing/file.dll");
        let (program, before) = make_program(vec![replace(&target, remove)]);
        assert!(matches!(
            PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]),
            Err(PeerAncestorPlanError::MissingParent(_))
        ));
    }
}

#[test]
fn cardinality_and_exact_o1_identity_are_required() {
    let root = tempdir().expect("root");
    let (endpoint, _) = create(&root.path().join("file.dll"));
    let program = ExactEndpointProgram::new(vec![endpoint]).expect("program");
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &[], &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::EvidenceCardinality {
            expected: 1,
            actual: 0
        })
    ));

    let (endpoint, _) = create(&root.path().join("file.dll"));
    let program = ExactEndpointProgram::new(vec![endpoint]).expect("program");
    let wrong = EndpointObservation::File(existing_file());
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &[wrong], &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::BeforeObservationMismatch(_))
    ));
}

#[test]
fn roots_must_be_unique_absolute_and_reachable() {
    let root = tempdir().expect("root");
    let (program, before) = make_program(vec![create(&root.path().join("new/file.dll"))]);
    assert!(matches!(
        PeerAncestorPlan::derive(
            &program,
            &before,
            &[root.path().to_owned(), root.path().to_owned()]
        ),
        Err(PeerAncestorPlanError::DuplicateRoot(_))
    ));
    let equivalent_root = PathBuf::from(format!(
        "{}{}",
        root.path().display(),
        std::path::MAIN_SEPARATOR
    ));
    assert!(matches!(
        PeerAncestorPlan::derive(
            &program,
            &before,
            &[root.path().to_owned(), equivalent_root]
        ),
        Err(PeerAncestorPlanError::DuplicateRoot(_))
    ));
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[PathBuf::from("relative")]),
        Err(PeerAncestorPlanError::InvalidRoot(_))
    ));
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[root.path().join("does-not-exist")]),
        Err(PeerAncestorPlanError::UnreachableRoot(_))
    ));
}

#[test]
fn outside_root_overlap_and_nondirectory_are_rejected() {
    let root = tempdir().expect("root");
    let outside = tempdir().expect("outside");
    let (program, before) = make_program(vec![create(&outside.path().join("file.dll"))]);
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::OutsideAuthorizedRoot(_))
    ));

    let parent = root.path().join("overlap");
    let (first, first_before) = create(&parent.join("file.dll"));
    let (second, second_before) = create(&parent);
    let (program, before) = make_program(vec![(first, first_before), (second, second_before)]);
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::EndpointAncestorOverlap(_))
    ));

    let existing = root.path().join("existing");
    std::fs::create_dir(&existing).expect("directory");
    let (first, first_before) = create(&existing);
    let (second, second_before) = create(&existing.join("nested/file.dll"));
    let (program, before) = make_program(vec![(first, first_before), (second, second_before)]);
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::EndpointAncestorOverlap(_))
    ));

    std::fs::write(root.path().join("occupied"), b"file").expect("file");
    let (program, before) = make_program(vec![create(&root.path().join("occupied/file.dll"))]);
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::UnsafeParent(_))
    ));
}

#[test]
fn overlapping_roots_use_the_first_authorized_root_deterministically() {
    let root = tempdir().expect("root");
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).expect("nested root");
    let endpoint = nested.join("deeper/file.dll");
    let (program, before) = make_program(vec![create(&endpoint)]);
    let plan =
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned(), nested.clone()])
            .expect("plan");
    assert_eq!(plan.ancestors()[0].path(), nested.join("deeper"));
    assert_eq!(plan.ancestors()[0].consumers(), &[0]);
}

#[cfg(unix)]
#[test]
fn symlink_parent_is_rejected_without_following() {
    use std::os::unix::fs::symlink;
    let root = tempdir().expect("root");
    let real = tempdir().expect("real");
    symlink(real.path(), root.path().join("link")).expect("symlink");
    let (program, before) = make_program(vec![create(&root.path().join("link/file.dll"))]);
    assert!(matches!(
        PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()]),
        Err(PeerAncestorPlanError::UnsafeParent(_))
    ));
}
