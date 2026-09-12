use std::path::Path;

use renderpilot_domain::{ArtifactSlot, FileOwnership, FileReceipt};

use super::super::{
    AppliedOperation, DiskObservation, DomainDeleteState, DomainPreimage, OptiScalerAction, action,
    durable, endpoint_for, move_into_private_artifact, observe_private_artifact, private_artifact,
};
use super::PreparedFileMutation;
use crate::ServiceError;

impl PreparedFileMutation<'_> {
    pub(crate) fn delete_file_exact(
        &mut self,
        path: &Path,
        expected: &FileReceipt,
    ) -> Result<AppliedOperation, ServiceError> {
        let index = self.current_operation_index(path, OptiScalerAction::Delete)?;
        let (before, journal_authorizes_reused) = {
            let endpoint = endpoint_for(self.journal.operations()[index].effect(), path)
                .ok_or_else(|| crate::failed("delete endpoint is not current"))?;
            let before = self.resolve_before(index, endpoint)?;
            // The planner admits a Reused delete only for its closed
            // `OptiScalerArtifact` role. That marker is intentionally not a
            // durable protocol field; the persisted endpoint instead carries the
            // exact initial Reused receipt and storage independently derives the
            // role from aggregate before/after state. Require that same receipt
            // here, so callers cannot turn an arbitrary live Reused file into a
            // delete target by merely supplying a matching value.
            let journal_authorizes_reused = matches!(
                endpoint.preimage(),
                DomainPreimage::Initial {
                    receipt: Some(receipt),
                    owned_basis: None,
                    ..
                } if receipt == expected && receipt.ownership() == FileOwnership::Reused
            );
            (before, journal_authorizes_reused)
        };
        let expected_before = DiskObservation::File {
            identity: expected.identity().to_owned(),
            digest: expected.digest().clone(),
        };
        if (expected.ownership() != FileOwnership::Owned && !journal_authorizes_reused)
            || before != expected_before
            || self.observe_endpoint(path)? != expected_before
        {
            return Err(crate::failed(
                "delete receipt does not match an exact authorized preimage",
            ));
        }
        let custody = private_artifact(self, index, ArtifactSlot::Custody)?;
        self.set_delete_state(index, DomainDeleteState::CaptureIntent, None)?;
        move_into_private_artifact(path, &custody, &before)?;
        let custody_observation = observe_private_artifact(self, index, ArtifactSlot::Custody)?;
        self.set_delete_state_with_artifact(
            index,
            DomainDeleteState::Captured {
                custody: durable(&custody_observation),
            },
            None,
            ArtifactSlot::Custody,
            &custody_observation,
        )?;
        self.set_delete_state(
            index,
            DomainDeleteState::Applied {
                custody: durable(&custody_observation),
            },
            Some(DiskObservation::Absent),
        )?;
        self.next_operation_id += 1;
        let ordinal =
            u32::try_from(index).map_err(|_| crate::failed("operation index overflow"))?;
        Ok(AppliedOperation { ordinal })
    }

    /// Consumes the planned release of an active retained-FSR target. A
    /// manually removed OptiScaler DLL is a sealed Verify(Absent), while a
    /// present DLL is a sealed exact Delete; both must precede restoring the
    /// immutable original backup.
    pub(crate) fn delete_or_verify_exact(
        &mut self,
        path: &Path,
        expected: &FileReceipt,
    ) -> Result<AppliedOperation, ServiceError> {
        let record = self
            .journal
            .operations()
            .get(self.next_operation_id)
            .ok_or_else(|| {
                crate::failed(format!(
                    "no current OptiScaler operation for {}",
                    path.display()
                ))
            })?;
        match action(record.effect()) {
            OptiScalerAction::Delete => self.delete_file_exact(path, expected),
            OptiScalerAction::Verify => self.verify_unchanged(path),
            actual => Err(crate::failed(format!(
                "retained FSR release expects Delete or Verify, not {actual:?}"
            ))),
        }
    }
}
