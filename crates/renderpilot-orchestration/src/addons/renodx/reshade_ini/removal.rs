//! Tolerant RenoDX configuration removal for uninstall.

use renderpilot_domain::{
    NormalizedPathRelation, PathRef, RenoDxConfigReceipt, normalized_path_relation,
};

use super::document::{IniDocument, RemovalPolicy};
use super::{RenoDxConfigError, RenoDxConfigReconcile};

/// Removes a receipt's keys during uninstall with per-key CAS semantics.
///
/// Unlike update/reconcile, uninstall is deliberately tolerant: a key that is
/// missing or no longer equals RenderPilot's last-written value is left alone,
/// while all other receipt-owned keys continue through restoration/removal.
pub(crate) fn plan_config_removal(
    ini_path: &PathRef,
    before: Option<&[u8]>,
    receipt: &RenoDxConfigReceipt,
) -> Result<RenoDxConfigReconcile, RenoDxConfigError> {
    if !receipt.is_supported()
        || !matches!(
            normalized_path_relation(receipt.ini_path.as_str(), ini_path.as_str()),
            NormalizedPathRelation::Equal
        )
    {
        return Err(RenoDxConfigError::InvalidReceipt);
    }
    let Some(before) = before else {
        return Ok(RenoDxConfigReconcile {
            after: None,
            receipt: None,
            changed: false,
        });
    };

    let mut doc = IniDocument::parse(before)?;
    let prior = receipt.managed_entries();
    let mut changed = false;
    for entry in prior.iter() {
        changed |= doc.remove_owned_key_with_policy(
            entry,
            receipt.section_preexisted,
            RemovalPolicy::Tolerant,
        )?;
    }

    if changed {
        changed |= doc.restore_newline_anchor(receipt.newline_anchor.as_deref());
    }
    let after = if changed {
        doc.to_bytes()
    } else {
        before.to_vec()
    };
    Ok(RenoDxConfigReconcile {
        after: Some(after),
        receipt: None,
        changed,
    })
}
