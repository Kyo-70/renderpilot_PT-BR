use std::path::Path;

use renderpilot_domain::{FileOwnership, FileReceipt};

use super::super::{
    DiskObservation, DomainOperationEffect, DomainRelocateState, OptiScalerAction, durable,
    move_exact_no_replace,
};
use super::PreparedFileMutation;
use crate::ServiceError;

impl PreparedFileMutation<'_> {
    pub(crate) fn relocate_file_exact(
        &mut self,
        source: &Path,
        destination: &Path,
        expected: &FileReceipt,
        resulting_ownership: FileOwnership,
    ) -> Result<FileReceipt, ServiceError> {
        if expected.ownership() != resulting_ownership {
            return Err(crate::failed("relocation cannot change receipt ownership"));
        }
        let index = self.current_operation_index(source, OptiScalerAction::Relocate)?;
        let (before, destination_before) = {
            let record = &self.journal.operations()[index];
            let DomainOperationEffect::Relocate(effect) = record.effect() else {
                return Err(crate::failed("relocation plan has wrong action"));
            };
            if !crate::paths::same_path(Path::new(effect.destination().path()), destination) {
                return Err(crate::failed("relocation destination is not current"));
            }
            (
                self.resolve_before(index, effect.source())?,
                self.resolve_before(index, effect.destination())?,
            )
        };
        let expected_native = DiskObservation::File {
            identity: expected.identity().to_owned(),
            digest: expected.digest().clone(),
        };
        if before != expected_native
            || destination_before != DiskObservation::Absent
            || self.observe_endpoint(source)? != expected_native
            || self.observe_endpoint(destination)? != DiskObservation::Absent
        {
            return Err(crate::failed("relocation endpoints drifted"));
        }
        self.set_relocate_state(index, DomainRelocateState::MoveIntent, None, None)?;
        move_exact_no_replace(source, destination, &expected_native)?;
        let source_after = self.observe_endpoint(source)?;
        let destination_after = self.observe_endpoint(destination)?;
        if source_after != DiskObservation::Absent || destination_after != expected_native {
            self.set_relocate_state(index, DomainRelocateState::ReverseIntent, None, None)?;
            let _ = move_exact_no_replace(destination, source, &expected_native);
            return Err(crate::failed(
                "relocation postimage drift; reverse intent retained",
            ));
        }
        self.set_relocate_state(
            index,
            DomainRelocateState::Applied {
                source_after: durable(&source_after),
                destination_after: durable(&destination_after),
            },
            Some(source_after),
            Some(destination_after),
        )?;
        self.next_operation_id += 1;
        let ordinal =
            u32::try_from(index).map_err(|_| crate::failed("operation index overflow"))?;
        self.receipt_for_ordinal(ordinal, resulting_ownership)
    }
}
