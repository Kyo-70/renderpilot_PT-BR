use std::path::Path;

use renderpilot_domain::{FileOwnership, FileReceipt, Sha256Hash};

use crate::file_mutation::optiscaler::PreparedFileMutation;
use crate::{ServiceError, failed};

pub(super) fn exact_receipt_from_live(
    path: &Path,
    ownership: FileOwnership,
) -> Result<FileReceipt, ServiceError> {
    let (parent, leaf) = crate::fs::verified_parent(path)
        .map_err(|error| failed(format!("failed to acquire proxy authority: {error}")))?;
    let observation = parent
        .observe_leaf(&leaf)
        .map_err(|error| failed(format!("failed to observe proxy participant: {error}")))?
        .ok_or_else(|| failed(format!("proxy participant is absent: {}", path.display())))?;
    if observation.kind != crate::fs::EntryKind::File {
        return Err(failed(format!(
            "proxy participant is not a file: {}",
            path.display()
        )));
    }
    let digest = observation.digest.ok_or_else(|| {
        failed(format!(
            "proxy participant digest is unavailable: {}",
            path.display()
        ))
    })?;
    let digest = Sha256Hash::new(digest)
        .map_err(|error| failed(format!("invalid proxy participant digest: {error}")))?;
    let receipt = match ownership {
        FileOwnership::Owned => FileReceipt::owned(observation.identity, digest),
        FileOwnership::Reused => FileReceipt::reused(observation.identity, digest),
    }
    .map_err(|error| failed(format!("invalid proxy participant receipt: {error}")))?;
    receipt
        .validate()
        .map_err(|error| failed(format!("invalid proxy participant receipt: {error}")))?;
    Ok(receipt)
}

pub(super) fn publish_outer(
    path: &Path,
    prior: Option<&FileReceipt>,
    expected_digest: &Sha256Hash,
    bytes: &[u8],
    mutation: &mut PreparedFileMutation<'_>,
    changed: &mut Vec<String>,
) -> Result<FileReceipt, ServiceError> {
    let live = maybe_exact_receipt_from_live(path, FileOwnership::Reused)?;
    if let Some(live) = &live {
        if live.digest() == expected_digest {
            if let Some(prior) = prior {
                if prior.identity() != live.identity() {
                    return Err(failed(format!(
                        "OptiScaler proxy identity changed at {}; preserving the different file",
                        path.display()
                    )));
                }
                let applied = mutation.verify_unchanged(path)?;
                return mutation.receipt_for_ordinal(applied.ordinal(), prior.ownership());
            }
            let applied = mutation.verify_unchanged(path)?;
            return mutation.receipt_for_ordinal(applied.ordinal(), FileOwnership::Reused);
        }
        if let Some(prior) = prior
            && prior.identity() == live.identity()
            && prior.digest() == live.digest()
        {
            // The aggregate identifies this path as the OptiScaler outer.
            // The journal still binds the exact live identity and digest, so
            // an adopted outer can be acquired without accepting drift.
            let applied = mutation.write_file(path, bytes)?;
            changed.push(path.to_string_lossy().into_owned());
            return mutation.receipt_for_ordinal(applied.ordinal(), FileOwnership::Owned);
        }
        let reason = match prior {
            Some(prior) if prior.identity() == live.identity() => {
                "the file drifted and requires an explicit repair decision"
            }
            Some(_) => "the existing file is foreign or identity-drifted",
            None => "the existing file is foreign",
        };
        return Err(failed(format!(
            "refusing to replace OptiScaler proxy {}; {reason}",
            path.display()
        )));
    }
    let applied = mutation.write_file(path, bytes)?;
    changed.push(path.to_string_lossy().into_owned());
    mutation.receipt_for_ordinal(applied.ordinal(), FileOwnership::Owned)
}

pub(super) fn maybe_exact_receipt_from_live(
    path: &Path,
    ownership: FileOwnership,
) -> Result<Option<FileReceipt>, ServiceError> {
    let (parent, leaf) = crate::fs::verified_parent(path)
        .map_err(|error| failed(format!("failed to acquire proxy authority: {error}")))?;
    let Some(observation) = parent
        .observe_leaf(&leaf)
        .map_err(|error| failed(format!("failed to observe proxy participant: {error}")))?
    else {
        return Ok(None);
    };
    if observation.kind != crate::fs::EntryKind::File {
        return Err(failed(format!(
            "proxy participant is not a file: {}",
            path.display()
        )));
    }
    let digest = observation.digest.ok_or_else(|| {
        failed(format!(
            "proxy participant digest is unavailable: {}",
            path.display()
        ))
    })?;
    let digest = Sha256Hash::new(digest)
        .map_err(|error| failed(format!("invalid proxy participant digest: {error}")))?;
    let receipt = match ownership {
        FileOwnership::Owned => FileReceipt::owned(observation.identity, digest),
        FileOwnership::Reused => FileReceipt::reused(observation.identity, digest),
    }
    .map_err(|error| failed(format!("invalid proxy participant receipt: {error}")))?;
    Ok(Some(receipt))
}
