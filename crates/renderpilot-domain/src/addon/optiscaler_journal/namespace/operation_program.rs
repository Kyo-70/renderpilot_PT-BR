fn normalized_component_key(path: &str) -> String {
    let mut key = normalized_path_key(path);
    while key.len() > 1
        && key.ends_with('/')
        && !(key.len() == 3 && key.as_bytes().get(1) == Some(&b':'))
    {
        key.pop();
    }
    key
}

fn canonical_namespace_path(
    field: &'static str,
    path: String,
) -> Result<String, OptiScalerJournalError> {
    if validate_canonical_namespace_path(field, &path).is_ok() {
        return Ok(path);
    }
    validate_nonempty(field, &path)?;
    let path = path.trim();
    if path.contains('\0') {
        return Err(OptiScalerJournalError::Invalid(
            "namespace path must not contain NUL",
        ));
    }
    let normalized = normalized_component_key(path);
    validate_namespace_components(&normalized)?;
    Ok(normalized)
}

fn validate_canonical_namespace_path(
    field: &'static str,
    path: &str,
) -> Result<(), OptiScalerJournalError> {
    validate_nonempty(field, path)?;
    if path.contains('\0') {
        return Err(OptiScalerJournalError::Invalid(
            "namespace path must not contain NUL",
        ));
    }
    if path.trim() != path
        || path.starts_with("//?/")
        || path.ends_with('/')
        || path
            .bytes()
            .any(|byte| byte == b'\\' || byte.is_ascii_uppercase())
    {
        return Err(OptiScalerJournalError::Invalid(
            "namespace path must use canonical durable spelling",
        ));
    }
    validate_namespace_components(path)
}

fn validate_namespace_components(path: &str) -> Result<(), OptiScalerJournalError> {
    if path.is_empty() || path == "." || path == ".." {
        return Err(OptiScalerJournalError::Invalid(
            "namespace path must be normalized and escape-free",
        ));
    }
    let body = if let Some(rest) = path.strip_prefix("//") {
        rest
    } else if let Some(rest) = path.strip_prefix('/') {
        rest
    } else {
        path
    };
    if body.is_empty()
        || body == "."
        || body == ".."
        || body
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(OptiScalerJournalError::Invalid(
            "namespace path must be normalized and escape-free",
        ));
    }
    Ok(())
}

fn direct_parent_key(path: &str) -> Option<String> {
    let key = normalized_component_key(path);
    let slash = key.rfind('/')?;
    if slash == 0 {
        return Some("/".to_owned());
    }
    if slash == 2 && key.as_bytes().get(1) == Some(&b':') {
        return Some(key[..3].to_owned());
    }
    Some(normalized_component_key(&key[..slash]))
}

fn validate_operation_program(
    operations: &[OperationRecord],
) -> Result<(), OptiScalerJournalError> {
    let mut last_touch: HashMap<String, (u32, Endpoint)> = HashMap::new();

    for operation in operations {
        for endpoint in operation.effect().endpoints() {
            let key = normalized_path_key(endpoint.path());
            match endpoint.preimage() {
                Preimage::Initial { .. } => {
                    if last_touch.contains_key(&key) {
                        return Err(OptiScalerJournalError::Invalid(
                            "repeated path touch must reference its prior postimage",
                        ));
                    }
                }
                Preimage::PriorPostimage {
                    operation_id,
                    endpoint: prior_endpoint,
                } => {
                    if *operation_id >= operation.operation_id() {
                        return Err(OptiScalerJournalError::Invalid(
                            "prior postimage must precede the current operation",
                        ));
                    }
                    let Some((last_operation_id, last_endpoint)) = last_touch.get(&key) else {
                        return Err(OptiScalerJournalError::Invalid(
                            "prior postimage cannot be the first path touch",
                        ));
                    };
                    if *last_operation_id != *operation_id || *last_endpoint != *prior_endpoint {
                        return Err(OptiScalerJournalError::Invalid(
                            "prior postimage must reference the immediate path producer",
                        ));
                    }
                    let prior_index = usize::try_from(*operation_id).map_err(|_| {
                        OptiScalerJournalError::Invalid("prior postimage ordinal overflows usize")
                    })?;
                    let Some(prior_operation) = operations.get(prior_index) else {
                        return Err(OptiScalerJournalError::Invalid(
                            "prior postimage operation does not exist",
                        ));
                    };
                    let Some(prior_recorded_endpoint) = prior_operation
                        .effect()
                        .endpoints()
                        .into_iter()
                        .find(|candidate| candidate.endpoint() == *prior_endpoint)
                    else {
                        return Err(OptiScalerJournalError::Invalid(
                            "prior postimage endpoint does not exist",
                        ));
                    };
                    if normalized_path_key(prior_recorded_endpoint.path()) != key {
                        return Err(OptiScalerJournalError::Invalid(
                            "prior postimage endpoint path does not match",
                        ));
                    }
                }
            }
            last_touch.insert(key, (operation.operation_id(), endpoint.endpoint()));
        }
    }
    Ok(())
}

fn validate_parent_dependencies(
    operations: &[OperationRecord],
) -> Result<(), OptiScalerJournalError> {
    for operation in operations {
        for dependency in operation.parent_dependencies() {
            let dependency_index = usize::try_from(*dependency).map_err(|_| {
                OptiScalerJournalError::Invalid("parent dependency ordinal overflows usize")
            })?;
            let Some(parent) = operations.get(dependency_index) else {
                return Err(OptiScalerJournalError::Invalid(
                    "parent dependency operation does not exist",
                ));
            };
            if parent.operation_id() >= operation.operation_id()
                || !matches!(parent.effect(), OperationEffect::CreateDirectory(_))
            {
                return Err(OptiScalerJournalError::Invalid(
                    "parent dependency must reference a previous directory creation",
                ));
            }
            let parent_key = parent
                .effect()
                .endpoints()
                .into_iter()
                .next()
                .map(|endpoint| normalized_component_key(endpoint.path()))
                .ok_or(OptiScalerJournalError::Invalid(
                    "directory dependency has no endpoint",
                ))?;
            let covers_endpoint = operation.effect().endpoints().into_iter().any(|endpoint| {
                direct_parent_key(endpoint.path()).as_deref() == Some(parent_key.as_str())
            });
            if !covers_endpoint {
                return Err(OptiScalerJournalError::Invalid(
                    "parent dependency is not a direct endpoint parent",
                ));
            }
        }
    }
    Ok(())
}
