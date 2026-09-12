use super::*;

pub(in crate::addons::optiscaler) fn now_ms() -> Result<i64, ServiceError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .map_err(|error| failed(format!("system clock is before the Unix epoch: {error}")))?;
    i64::try_from(millis)
        .map_err(|_| failed("system clock does not fit an i64 millisecond timestamp"))
}

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler) struct ConfigPreservationPlan {
    pub(in crate::addons::optiscaler) source: PathBuf,
    pub(in crate::addons::optiscaler) destination: PathBuf,
    pub(in crate::addons::optiscaler) expected_sha256: Sha256Hash,
    pub(in crate::addons::optiscaler) source_current: FileReceipt,
    pub(in crate::addons::optiscaler) remove_source: bool,
}

pub(in crate::addons::optiscaler) struct ConfigPreservationExecution {
    pub(in crate::addons::optiscaler) destination: PathBuf,
    pub(in crate::addons::optiscaler) destination_receipt: FileReceipt,
}

impl ConfigPreservationPlan {
    pub(in crate::addons::optiscaler) fn for_game(
        game_id: &GameId,
        source: &Path,
        expected_sha256: Sha256Hash,
        remove_source: bool,
    ) -> Result<Self, ServiceError> {
        let expected_receipt = exact_receipt_from_live(source, FileOwnership::Owned)?;
        Self::for_game_with_receipt(
            game_id,
            source,
            expected_sha256,
            expected_receipt,
            remove_source,
        )
    }

    pub(in crate::addons::optiscaler) fn for_game_with_receipt(
        game_id: &GameId,
        source: &Path,
        expected_sha256: Sha256Hash,
        expected_receipt: FileReceipt,
        remove_source: bool,
    ) -> Result<Self, ServiceError> {
        if expected_receipt.ownership() != FileOwnership::Owned
            || expected_receipt.digest() != &expected_sha256
        {
            return Err(failed(format!(
                "OptiScaler configuration preservation receipt does not match source digest: {}",
                source.display()
            )));
        }
        let destination = crate::app_dir::app_dir()?
            .join("recovery")
            .join("optiscaler")
            .join(safe_recovery_game_id(game_id))
            .join(format!(
                "{}-{}-OptiScaler.ini",
                now_ms()?,
                ulid::Ulid::generate()
            ));
        Ok(Self {
            source: source.to_path_buf(),
            destination,
            expected_sha256,
            source_current: expected_receipt,
            remove_source,
        })
    }

    pub(in crate::addons::optiscaler) fn scope_root(&self) -> Result<PathBuf, ServiceError> {
        let mut root = self
            .destination
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                failed(format!(
                    "OptiScaler recovery destination has no parent: {}",
                    self.destination.display()
                ))
            })?;
        while !root.exists() {
            root = root.parent().map(Path::to_path_buf).ok_or_else(|| {
                failed(format!(
                    "OptiScaler recovery destination has no existing ancestor: {}",
                    self.destination.display()
                ))
            })?;
        }
        Ok(root)
    }

    pub(in crate::addons::optiscaler) fn source_target(&self) -> MutationTarget {
        MutationTarget::quarantine(&self.source, Some(self.expected_sha256.clone()))
    }

    pub(in crate::addons::optiscaler) fn owned_source_receipt(&self) -> &FileReceipt {
        &self.source_current
    }

    pub(in crate::addons::optiscaler) fn execute(
        &self,
        mutation: &mut PreparedFileMutation<'_>,
        remove_source: bool,
    ) -> Result<ConfigPreservationExecution, ServiceError> {
        let source_receipt = self.owned_source_receipt();
        let applied = mutation.copy_before_state_atomically(
            &self.source,
            &self.destination,
            source_receipt,
        )?;
        let destination_receipt =
            mutation.receipt_for_ordinal(applied.ordinal(), FileOwnership::Owned)?;
        if remove_source {
            if !self.remove_source {
                return Ok(ConfigPreservationExecution {
                    destination: self.destination.clone(),
                    destination_receipt,
                });
            }
            let owned_receipt = self.owned_source_receipt();
            mutation.delete_file_exact(&self.source, owned_receipt)?;
        }
        Ok(ConfigPreservationExecution {
            destination: self.destination.clone(),
            destination_receipt,
        })
    }
}

pub(in crate::addons::optiscaler) fn safe_recovery_game_id(game_id: &GameId) -> String {
    let value = game_id
        .as_str()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(96)
        .collect::<String>();
    if value.is_empty() {
        "game".to_owned()
    } else {
        value
    }
}
