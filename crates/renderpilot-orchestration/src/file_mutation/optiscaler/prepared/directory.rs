use std::path::{Path, PathBuf};

use renderpilot_domain::{ArtifactSlot, DurableObservation, OptiScalerDirectoryReceipt, PathRef};

use super::super::{
    AppliedOperation, DiskObservation, DomainCreateDirectoryState, DomainOperationEffect,
    OptiScalerAction, create_private_artifact_directory, durable, endpoint_for, private_artifact,
    publish_from_private_artifact,
};
use super::PreparedFileMutation;
use crate::ServiceError;

impl PreparedFileMutation<'_> {
    pub(crate) fn created_directory_receipts(
        &self,
        root: &Path,
    ) -> Result<Vec<OptiScalerDirectoryReceipt>, ServiceError> {
        let mut receipts = Vec::new();
        for record in self.journal.operations() {
            let DomainOperationEffect::CreateDirectory(effect) = record.effect() else {
                continue;
            };
            if !crate::paths::is_within(Path::new(effect.endpoint().path()), root) {
                continue;
            }
            if let DomainCreateDirectoryState::Applied {
                live: DurableObservation::Directory { identity },
            } = effect.state()
            {
                receipts.push(OptiScalerDirectoryReceipt {
                    path: PathRef::new(effect.endpoint().path()).map_err(|error| {
                        crate::failed(format!("invalid directory path: {error}"))
                    })?,
                    identity: identity.clone(),
                });
            }
        }
        Ok(receipts)
    }

    pub(crate) fn create_directory(
        &mut self,
        path: &Path,
    ) -> Result<AppliedOperation, ServiceError> {
        let index = self.current_operation_index(path, OptiScalerAction::CreateDirectory)?;
        let before = {
            let endpoint = endpoint_for(self.journal.operations()[index].effect(), path)
                .ok_or_else(|| crate::failed("directory endpoint is not current"))?;
            self.resolve_before(index, endpoint)?
        };
        if before != DiskObservation::Absent
            || self.observe_endpoint(path)? != DiskObservation::Absent
        {
            return Err(crate::failed("directory target is not exactly absent"));
        }
        let stage = private_artifact(self, index, ArtifactSlot::Stage)?;
        self.set_directory_state(index, DomainCreateDirectoryState::StageIntent, None)?;
        let staged = create_private_artifact_directory(&stage)?;
        self.set_directory_state_with_artifact(
            index,
            DomainCreateDirectoryState::Staged {
                stage: durable(&staged),
            },
            None,
            ArtifactSlot::Stage,
            &staged,
        )?;
        self.set_directory_state(
            index,
            DomainCreateDirectoryState::PublishIntent {
                stage: durable(&staged),
            },
            None,
        )?;
        publish_from_private_artifact(&stage, path, &staged)?;
        let live = self.observe_endpoint(path)?;
        if !matches!(live, DiskObservation::Directory { .. }) {
            return Err(crate::failed(
                "created directory postimage is not a directory",
            ));
        }
        self.set_directory_state_with_artifact(
            index,
            DomainCreateDirectoryState::Applied {
                live: durable(&live),
            },
            Some(live),
            ArtifactSlot::Stage,
            &DiskObservation::Absent,
        )?;
        self.next_operation_id += 1;
        let ordinal =
            u32::try_from(index).map_err(|_| crate::failed("operation index overflow"))?;
        Ok(AppliedOperation { ordinal })
    }

    /// Materializes the current directory frontier when a topology action has
    /// finished releasing/relocating its file participants.  Directory
    /// creation remains an ordered journal action; this helper only consumes
    /// consecutive CreateDirectory operations and never searches by path.
    pub(crate) fn create_planned_directories(&mut self) -> Result<(), ServiceError> {
        loop {
            let Some(record) = self.journal.operations().get(self.next_operation_id) else {
                return Ok(());
            };
            let DomainOperationEffect::CreateDirectory(effect) = record.effect() else {
                return Ok(());
            };
            let path = PathBuf::from(effect.endpoint().path());
            self.create_directory(&path)?;
        }
    }
}
