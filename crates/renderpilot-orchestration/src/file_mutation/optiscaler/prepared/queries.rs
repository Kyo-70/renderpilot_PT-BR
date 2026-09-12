use renderpilot_domain::{
    ArtifactSlot, FileOwnership, FileReceipt, OptiScalerConfigurationBaseline,
};

use super::super::{
    AppliedOperation, DiskObservation, DomainOperationEffect, MAX_OPTISCALER_CONFIGURATION_BYTES,
    artifact, disk_observation, expected_native, operation_is_terminal, private_artifact,
    read_private_artifact,
};
use super::PreparedFileMutation;
use crate::ServiceError;

impl PreparedFileMutation<'_> {
    /// Receipt evidence comes from the requested ordinal's applied endpoint.
    /// No path is searched or used as an operation selector.
    pub(crate) fn receipt_for_ordinal(
        &self,
        operation_id: u32,
        ownership: FileOwnership,
    ) -> Result<FileReceipt, ServiceError> {
        let record = self
            .journal
            .operations()
            .get(
                usize::try_from(operation_id)
                    .map_err(|_| crate::failed("operation id overflow"))?,
            )
            .ok_or_else(|| crate::failed("receipt ordinal is outside the journal"))?;
        if !operation_is_terminal(record) {
            return Err(crate::failed(
                "receipt requested before operation was applied",
            ));
        }
        let endpoint = match record.effect() {
            DomainOperationEffect::Relocate(effect) => effect.destination(),
            effect => effect
                .endpoints()
                .into_iter()
                .next()
                .ok_or_else(|| crate::failed("operation has no endpoint"))?,
        };
        let DiskObservation::File { identity, digest } = expected_native(endpoint) else {
            return Err(crate::failed("operation postimage is not an exact file"));
        };
        match ownership {
            FileOwnership::Owned => FileReceipt::owned(identity, digest),
            FileOwnership::Reused => FileReceipt::reused(identity, digest),
        }
        .map_err(|error| crate::failed(format!("invalid OptiScaler receipt: {error}")))
    }

    /// Captures the configuration preimage retained by an already-applied
    /// journal write. This never rereads the live target after publication.
    pub(crate) fn configuration_baseline_for(
        &self,
        applied: AppliedOperation,
    ) -> Result<OptiScalerConfigurationBaseline, ServiceError> {
        let index = usize::try_from(applied.ordinal())
            .map_err(|_| crate::failed("configuration operation id overflow"))?;
        let record = self
            .journal
            .operations()
            .get(index)
            .ok_or_else(|| crate::failed("configuration operation is outside the journal"))?;
        let DomainOperationEffect::Write(effect) = record.effect() else {
            return Err(crate::failed(
                "configuration baseline requires a write operation",
            ));
        };
        let before = self.resolve_before(index, effect.endpoint())?;
        match before {
            DiskObservation::Absent => Ok(OptiScalerConfigurationBaseline::Absent),
            DiskObservation::File { identity, digest } => {
                let custody = private_artifact(self, index, ArtifactSlot::Custody)?;
                let (bytes, observed) =
                    read_private_artifact(&custody, Some(MAX_OPTISCALER_CONFIGURATION_BYTES))?;
                if disk_observation(&observed)? != artifact(record, ArtifactSlot::Custody) {
                    return Err(crate::failed(
                        "configuration custody artifact changed after journal write",
                    ));
                }
                let receipt = FileReceipt::reused(identity, digest).map_err(|error| {
                    crate::failed(format!("invalid configuration baseline: {error}"))
                })?;
                OptiScalerConfigurationBaseline::present(receipt, bytes).map_err(|error| {
                    crate::failed(format!("invalid configuration baseline: {error}"))
                })
            }
            _ => Err(crate::failed(
                "configuration write preimage is not an exact file",
            )),
        }
    }
}
