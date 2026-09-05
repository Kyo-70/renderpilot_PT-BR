use super::parts::OptiScalerInstallStateParts;
use super::{
    OptiScalerConfigurationBaseline, OptiScalerInstallState, OptiScalerLifecycleContext,
    OptiScalerStateError, validate_configuration_lifecycle,
};

/// Creates a state for a physical RenderPilot install.
pub fn from_new_install(
    parts: OptiScalerInstallStateParts,
    configuration_baseline: OptiScalerConfigurationBaseline,
) -> Result<OptiScalerInstallState, OptiScalerStateError> {
    let state = parts.build(configuration_baseline);
    state.validate()?;
    validate_configuration_lifecycle(&state, OptiScalerLifecycleContext::NewInstall)?;
    Ok(state)
}

/// Creates a state for exact adoption of an unmanaged installation.
pub fn from_new_adoption(
    parts: OptiScalerInstallStateParts,
    configuration_baseline: OptiScalerConfigurationBaseline,
) -> Result<OptiScalerInstallState, OptiScalerStateError> {
    let state = parts.build(configuration_baseline);
    state.validate()?;
    validate_configuration_lifecycle(&state, OptiScalerLifecycleContext::NewAdoption)?;
    Ok(state)
}

/// Rehydrates a structurally valid persisted state.
pub fn from_persisted(
    parts: OptiScalerInstallStateParts,
    configuration_baseline: OptiScalerConfigurationBaseline,
) -> Result<OptiScalerInstallState, OptiScalerStateError> {
    let state = parts.build(configuration_baseline);
    state.validate()?;
    Ok(state)
}

/// Produces an update state while copying the immutable original baseline.
pub(super) fn from_existing(
    previous: &OptiScalerInstallState,
    parts: OptiScalerInstallStateParts,
) -> Result<OptiScalerInstallState, OptiScalerStateError> {
    previous.validate()?;
    let previous_lifecycle_is_canonical =
        validate_configuration_lifecycle(previous, OptiScalerLifecycleContext::Existing).is_ok();
    let state = parts.build(previous.configuration_baseline().clone());
    state.validate()?;
    validate_configuration_lifecycle(&state, OptiScalerLifecycleContext::Existing)?;

    if state.prerequisite_binding != previous.prerequisite_binding {
        return Err(OptiScalerStateError::PrerequisiteBindingTransition);
    }

    if previous_lifecycle_is_canonical {
        let previous_configuration = super::configuration_receipt(previous)?;
        let candidate_configuration = super::configuration_receipt(&state)?;
        if previous_configuration.cleanup != candidate_configuration.cleanup {
            return Err(OptiScalerStateError::ConfigurationBaselineTransition(
                "existing configuration cleanup policy cannot drift",
            ));
        }
        if let (
            OptiScalerConfigurationBaseline::Present {
                receipt: previous_receipt,
                ..
            },
            OptiScalerConfigurationBaseline::Present {
                receipt: candidate_receipt,
                ..
            },
        ) = (
            previous.configuration_baseline(),
            state.configuration_baseline(),
        ) && previous_receipt.identity() != candidate_receipt.identity()
        {
            return Err(OptiScalerStateError::ConfigurationBaselineTransition(
                "existing configuration baseline identity cannot drift",
            ));
        }
    }

    Ok(state)
}

/// Produces the first managed successor of a state whose configuration was
/// previously only observed as Reused. The supplied baseline is the exact
/// user-owned file observed immediately before the first OptiScaler write,
/// or the immutable baseline retained by an absent-file repair.
pub(super) fn from_existing_with_configuration_acquisition(
    previous: &OptiScalerInstallState,
    parts: OptiScalerInstallStateParts,
    configuration_baseline: OptiScalerConfigurationBaseline,
) -> Result<OptiScalerInstallState, OptiScalerStateError> {
    previous.validate()?;
    validate_configuration_lifecycle(previous, OptiScalerLifecycleContext::Existing)?;
    let previous_configuration = super::configuration_receipt(previous)?;
    if previous_configuration.installed.ownership() != super::FileOwnership::Reused
        || previous_configuration.cleanup != super::OptiScalerFileCleanup::PreserveUnchanged
    {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "configuration acquisition requires a canonical Reused predecessor",
        ));
    }
    let state = parts.build(configuration_baseline);
    state.validate()?;
    validate_configuration_lifecycle(&state, OptiScalerLifecycleContext::Existing)?;
    if state.prerequisite_binding != previous.prerequisite_binding {
        return Err(OptiScalerStateError::PrerequisiteBindingTransition);
    }
    let candidate_configuration = super::configuration_receipt(&state)?;
    if state.configuration_baseline().receipt().is_none() {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "configuration acquisition requires an exact present baseline",
        ));
    }
    if candidate_configuration.installed.ownership() != super::FileOwnership::Owned
        || candidate_configuration.cleanup
            != super::OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline
    {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "configuration acquisition must retain the immutable present baseline",
        ));
    }
    Ok(state)
}

/// Produces a successor that leaves an adopted Reused configuration at its
/// former target untouched and creates a new Owned configuration at a new
/// canonical target. This is not an in-place acquisition: the successor has
/// no claim to the old user file, so its cleanup baseline is Absent.
pub(super) fn from_existing_with_configuration_retarget(
    previous: &OptiScalerInstallState,
    parts: OptiScalerInstallStateParts,
) -> Result<OptiScalerInstallState, OptiScalerStateError> {
    previous.validate()?;
    validate_configuration_lifecycle(previous, OptiScalerLifecycleContext::Existing)?;
    let previous_configuration = super::configuration_receipt(previous)?;
    if previous_configuration.installed.ownership() != super::FileOwnership::Reused
        || previous_configuration.cleanup != super::OptiScalerFileCleanup::PreserveUnchanged
    {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "configuration retarget requires a canonical Reused predecessor",
        ));
    }

    let state = parts.build(OptiScalerConfigurationBaseline::Absent);
    state.validate()?;
    validate_configuration_lifecycle(&state, OptiScalerLifecycleContext::Existing)?;
    if state.prerequisite_binding != previous.prerequisite_binding {
        return Err(OptiScalerStateError::PrerequisiteBindingTransition);
    }
    let candidate_configuration = super::configuration_receipt(&state)?;
    if candidate_configuration.path == previous_configuration.path
        || candidate_configuration.installed.ownership() != super::FileOwnership::Owned
        || candidate_configuration.cleanup != super::OptiScalerFileCleanup::RemoveIfUnchanged
    {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "configuration retarget must create an Owned absent-baseline successor at a new path",
        ));
    }
    Ok(state)
}
