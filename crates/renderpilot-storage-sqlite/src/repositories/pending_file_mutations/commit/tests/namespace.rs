use super::*;

#[test]
fn namespace_custody_accepts_the_materialized_disjoint_workspace_program() {
    validate_namespace_custody(&materialized_applied())
        .expect("materialized workspace must be disjoint from its endpoint");
}

#[test]
fn namespace_custody_requires_every_workspace_identity() {
    let journal = journal();
    assert!(validate_namespace_custody(&journal).is_err());
}

#[test]
fn namespace_custody_accepts_a_workspace_free_postcommit_program() {
    let capability = capability();
    let operation = applied_postcommit_operation(0, "game/obsolete-dir", "directory-id");
    let control = ControlNamespaceBinding::new(
        format!("game/control-postcommit-{}", capability.as_str()),
        None,
        capability,
    )
    .expect("control namespace");
    let mut journal = test_journal(vec!["game".to_owned()], control, vec![operation]);
    journal
        .control_namespace_mut()
        .set_identity("control-id")
        .expect("control identity");
    journal.set_materialization(MaterializationState::Ready);

    validate_namespace_custody(&journal)
        .expect("postcommit-only program must require no private workspace");
}
