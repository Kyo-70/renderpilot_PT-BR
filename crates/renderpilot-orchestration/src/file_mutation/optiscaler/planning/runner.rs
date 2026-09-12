pub(crate) fn run_optiscaler_mutation<'a, T, E: Into<ServiceError>>(
    mutation: &OptiScalerMutation<'a>,
    work: impl FnOnce(&mut PreparedFileMutation<'a>) -> Result<T, E>,
    commit: impl FnOnce(&mut PreparedFileMutation<'a>, &T) -> Result<(), ServiceError>,
    on_committed: impl FnOnce(&T),
    on_rolled_back: impl FnOnce(),
) -> Result<T, ServiceError> {
    let mut prepared = prepare_optiscaler(mutation)?;
    let value = match work(&mut prepared) {
        Ok(value) => value,
        Err(error) => return rollback(prepared, error.into(), on_rolled_back),
    };
    if let Err(error) = prepared.complete_unmodified_operations() {
        return rollback(prepared, error, on_rolled_back);
    }
    if let Err(error) = prepared.finish_preparation() {
        return rollback(prepared, error, on_rolled_back);
    }
    if let Err(error) = commit(&mut prepared, &value) {
        return rollback(prepared, error, on_rolled_back);
    }
    prepared.apply_post_commit_directory_cleanup()?;
    on_committed(&value);
    cleanup_committed(&mut prepared)?;
    Ok(value)
}

fn rollback<T>(
    prepared: PreparedFileMutation<'_>,
    primary: ServiceError,
    on_rolled_back: impl FnOnce(),
) -> Result<T, ServiceError> {
    match rollback_prepared(prepared) {
        Ok(()) => {
            on_rolled_back();
            Err(primary)
        }
        Err(error) => Err(super::transaction::combine_rollback_error(&primary, &error)),
    }
}
