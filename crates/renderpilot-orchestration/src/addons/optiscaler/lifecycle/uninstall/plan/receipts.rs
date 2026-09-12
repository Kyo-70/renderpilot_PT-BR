use super::*;

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler::lifecycle::uninstall) enum ReceiptObservation {
    Exact(FileReceipt),
    Absent,
}

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn observe_receipt(
    path: &Path,
    expected: &FileReceipt,
) -> Result<ReceiptObservation, ServiceError> {
    let Some(live) = maybe_exact_receipt_from_live(path, expected.ownership())? else {
        return Ok(ReceiptObservation::Absent);
    };
    if live.identity() != expected.identity() || live.digest() != expected.digest() {
        return Err(failed(format!(
            "OptiScaler uninstall found a drifted file at {}; repair/reinstall before retry",
            path.display()
        )));
    }
    Ok(ReceiptObservation::Exact(live))
}

/// Observes a receipt that belongs to the persisted OptiScaler release tree.
/// Missing descendants are repairable absence; the retained root and every
/// real ancestor remain strict authority boundaries.
pub(in crate::addons::optiscaler::lifecycle::uninstall) fn observe_managed_receipt(
    managed_root: &Path,
    path: &Path,
    expected: &FileReceipt,
) -> Result<ReceiptObservation, ServiceError> {
    let Some(live) = super::super::super::maybe_exact_managed_receipt_from_live(
        managed_root,
        path,
        expected.ownership(),
    )?
    else {
        return Ok(ReceiptObservation::Absent);
    };
    if live.identity() != expected.identity() || live.digest() != expected.digest() {
        return Err(failed(format!(
            "OptiScaler uninstall found a drifted managed file at {}; repair/reinstall before retry",
            path.display()
        )));
    }
    Ok(ReceiptObservation::Exact(live))
}

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn observe_managed_reused_configuration(
    managed_root: &Path,
    path: &Path,
) -> Result<ReceiptObservation, ServiceError> {
    Ok(super::super::super::maybe_exact_managed_receipt_from_live(
        managed_root,
        path,
        FileOwnership::Reused,
    )?
    .map_or(ReceiptObservation::Absent, ReceiptObservation::Exact))
}

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn require_receipt(
    path: &Path,
    expected: &FileReceipt,
) -> Result<FileReceipt, ServiceError> {
    match observe_receipt(path, expected)? {
        ReceiptObservation::Exact(live) => Ok(live),
        ReceiptObservation::Absent => Err(failed(format!(
            "OptiScaler uninstall expected a present file at {}",
            path.display()
        ))),
    }
}

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn missing_recovery_directories(
    destination: &Path,
) -> Result<Vec<PathBuf>, ServiceError> {
    let mut cursor = destination
        .parent()
        .ok_or_else(|| failed("OptiScaler recovery destination has no parent"))?;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(failed(format!(
                    "OptiScaler recovery parent is not a directory: {}",
                    cursor.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.to_path_buf());
                cursor = cursor.parent().ok_or_else(|| {
                    failed("OptiScaler recovery destination has no reachable parent")
                })?;
            }
            Err(error) => {
                return Err(failed(format!(
                    "failed to inspect OptiScaler recovery parent {}: {error}",
                    cursor.display()
                )));
            }
        }
    }
    missing.reverse();
    Ok(missing)
}
