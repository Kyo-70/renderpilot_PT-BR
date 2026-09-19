//! Context-isolated deduplicated evidence collection.

use crate::addons::game_analysis::context::GameInstallationContext;
use crate::addons::game_analysis::evidence::ValidatedEvidence;

#[derive(Debug, PartialEq, Eq)]
pub enum EvidenceInsertionError {
    MismatchedInstallationContext,
}

/// Deduplicated collection of validated evidence isolated to one installation context.
#[derive(Debug)]
pub struct GameEvidenceSet<'game> {
    context: &'game GameInstallationContext,
    evidences: Vec<ValidatedEvidence<'game>>,
}

impl<'game> GameEvidenceSet<'game> {
    #[must_use]
    pub fn new(context: &'game GameInstallationContext) -> Self {
        Self {
            context,
            evidences: Vec::new(),
        }
    }

    pub fn insert(
        &mut self,
        evidence: ValidatedEvidence<'game>,
    ) -> Result<bool, EvidenceInsertionError> {
        if evidence.installation_id() != self.context.id() {
            return Err(EvidenceInsertionError::MismatchedInstallationContext);
        }

        let duplicate = self.evidences.iter().any(|e| {
            e.source() == evidence.source()
                && e.scope() == evidence.scope()
                && e.file_path() == evidence.file_path()
                && e.file_offset() == evidence.file_offset()
                && e.claim() == evidence.claim()
        });

        if duplicate {
            return Ok(false);
        }

        self.evidences.push(evidence);
        Ok(true)
    }

    #[must_use]
    pub fn evidences(&self) -> &[ValidatedEvidence<'game>] {
        &self.evidences
    }
}
