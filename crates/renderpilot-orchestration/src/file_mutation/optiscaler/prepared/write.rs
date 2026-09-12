use std::path::Path;

use renderpilot_domain::{ArtifactSlot, FileOwnership, FileReceipt, Sha256Hash};

use super::super::{
    AppliedOperation, DiskObservation, DomainWriteState, OptiScalerAction, artifact,
    disk_observation, durable, endpoint_for, hash_bytes, overwrite_file_exact, private_artifact,
    private_entry_observation, publish_from_private_artifact, read_verified_file,
    remove_private_artifact, token_drift, write_private_artifact,
};
use super::PreparedFileMutation;
use crate::ServiceError;

impl PreparedFileMutation<'_> {
    pub(crate) fn copy_before_state_atomically(
        &mut self,
        source: &Path,
        destination: &Path,
        expected: &FileReceipt,
    ) -> Result<AppliedOperation, ServiceError> {
        if expected.ownership() != FileOwnership::Owned {
            return Err(crate::failed("preservation requires Owned authority"));
        }
        let (bytes, observed) = read_verified_file(source)?;
        if observed.identity != expected.identity()
            || observed.digest.as_deref() != Some(expected.digest().as_str())
        {
            return Err(crate::failed("preservation source drifted"));
        }
        self.write_file(destination, &bytes)
    }

    pub(crate) fn copy_file_from_verified(
        &mut self,
        source: &Path,
        destination: &Path,
        expected_sha256: &Sha256Hash,
    ) -> Result<AppliedOperation, ServiceError> {
        let (bytes, _) = read_verified_file(source)?;
        if hash_bytes(&bytes)? != *expected_sha256 {
            return Err(crate::failed("copy source digest drifted"));
        }
        self.write_file(destination, &bytes)
    }

    pub(crate) fn write_file(
        &mut self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<AppliedOperation, ServiceError> {
        let index = self.current_operation_index(path, OptiScalerAction::Write)?;
        let before = {
            let endpoint = endpoint_for(self.journal.operations()[index].effect(), path)
                .ok_or_else(|| crate::failed("write endpoint is not current"))?;
            self.resolve_before(index, endpoint)?
        };
        let current = self.observe_endpoint(path)?;
        if current != before {
            return Err(token_drift(path, &before, &current));
        }
        let target_digest = hash_bytes(bytes)?;
        self.set_write_state(
            index,
            DomainWriteState::StageIntent {
                target_digest: target_digest.clone(),
            },
            None,
        )?;
        let staged = {
            let stage = private_artifact(self, index, ArtifactSlot::Stage)?;
            write_private_artifact(&stage, bytes)?
        };
        self.set_write_state_with_artifact(
            index,
            DomainWriteState::Staged {
                stage: durable(&staged),
            },
            None,
            ArtifactSlot::Stage,
            &staged,
        )?;
        self.set_write_state(
            index,
            DomainWriteState::CaptureIntent {
                stage: durable(&staged),
            },
            None,
        )?;
        if matches!(before, DiskObservation::File { .. }) {
            // Keep the live directory entry in place for an Owned rewrite.
            // The custody copy is an exact digest witness used for rollback;
            // publishing through the retained live handle preserves its
            // durable identity for the aggregate commit contract.
            let (bytes_before, observed) = read_verified_file(path)?;
            if disk_observation(&observed)? != before {
                return Err(crate::failed(
                    "write preimage changed while capturing custody",
                ));
            }
            let custody_observation = {
                let custody = private_artifact(self, index, ArtifactSlot::Custody)?;
                write_private_artifact(&custody, &bytes_before)?
            };
            self.set_write_state_with_artifact(
                index,
                DomainWriteState::Captured {
                    stage: durable(&staged),
                    custody: durable(&custody_observation),
                },
                None,
                ArtifactSlot::Custody,
                &custody_observation,
            )?;
        } else {
            self.set_write_state(
                index,
                DomainWriteState::Captured {
                    stage: durable(&staged),
                    custody: durable(&DiskObservation::Absent),
                },
                None,
            )?;
        }
        let expected = DiskObservation::File {
            identity: match (&before, &staged) {
                (DiskObservation::File { identity, .. }, _) => identity.clone(),
                (DiskObservation::Absent, DiskObservation::File { identity, .. }) => {
                    identity.clone()
                }
                _ => return Err(crate::failed("stage is not a regular file")),
            },
            digest: target_digest,
        };
        let custody = artifact(&self.journal.operations()[index], ArtifactSlot::Custody);
        self.set_write_state(
            index,
            DomainWriteState::PublishIntent {
                stage: durable(&staged),
                custody: durable(&custody),
            },
            None,
        )?;
        let live = if matches!(before, DiskObservation::File { .. }) {
            overwrite_file_exact(path, &before, bytes)?
        } else {
            let stage = private_artifact(self, index, ArtifactSlot::Stage)?;
            publish_from_private_artifact(&stage, path, &expected)?;
            self.observe_endpoint(path)?
        };
        if live != expected {
            return Err(token_drift(path, &expected, &live));
        }
        if matches!(before, DiskObservation::File { .. }) {
            let stage = private_artifact(self, index, ArtifactSlot::Stage)?;
            remove_private_artifact(&stage, &private_entry_observation(&staged)?)?;
        }
        self.set_write_state_with_artifact(
            index,
            DomainWriteState::Applied {
                live: durable(&live),
                custody: durable(&custody),
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
}
