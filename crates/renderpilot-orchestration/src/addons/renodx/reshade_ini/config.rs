//! Multi-key RenoDX configuration planning and CAS reconciliation.

use renderpilot_domain::{
    NormalizedPathRelation, PathRef, RenoDxConfigEntry, RenoDxConfigReceipt,
    RenoDxManagedConfigKey, RenoDxSetPathValue, normalized_path_relation,
};

use crate::addons::renodx::types::{RenoDxConfig, RenoDxConfigSetting};

use super::document::{IniDocument, RemovalPolicy};
use super::{RenoDxConfigError, RenoDxConfigMutation, RenoDxConfigReconcile, SET_PATH_KEY};

fn config_settings(config: Option<&RenoDxConfig>) -> &[RenoDxConfigSetting] {
    config.map_or(&[], |config| config.settings.as_slice())
}

fn validate_desired_config(
    set_path: Option<RenoDxSetPathValue>,
    config: Option<&RenoDxConfig>,
) -> Result<usize, RenoDxConfigError> {
    let settings = config_settings(config);
    for (index, setting) in settings.iter().enumerate() {
        let key = setting.key.managed_key();
        let valid = key != RenoDxManagedConfigKey::SetPath && key.accepts_value(setting.value);
        if !valid {
            return Err(RenoDxConfigError::Ambiguous(
                "unsupported RenoDX configuration key or value",
            ));
        }
        if settings[..index]
            .iter()
            .any(|prior| prior.key == setting.key)
        {
            return Err(RenoDxConfigError::Ambiguous("duplicate configuration keys"));
        }
    }
    Ok(usize::from(set_path.is_some()) + settings.len())
}

fn for_each_desired_config(
    set_path: Option<RenoDxSetPathValue>,
    config: Option<&RenoDxConfig>,
    mut visit: impl FnMut(&str, i32) -> Result<(), RenoDxConfigError>,
) -> Result<(), RenoDxConfigError> {
    if let Some(value) = set_path {
        visit(SET_PATH_KEY, value.as_i32())?;
    }
    for setting in config_settings(config) {
        visit(setting.key.as_str(), setting.value)?;
    }
    Ok(())
}

fn desired_contains(
    set_path: Option<RenoDxSetPathValue>,
    config: Option<&RenoDxConfig>,
    key: &str,
) -> bool {
    set_path.is_some_and(|_| key.eq_ignore_ascii_case(SET_PATH_KEY))
        || config_settings(config)
            .iter()
            .any(|setting| setting.key.as_str().eq_ignore_ascii_case(key))
}

/// Plans one atomic multi-key `[renodx]` mutation. The input list is already
/// closed/validated by the catalogue parser; this function still rejects
/// duplicate keys and unsafe syntax before producing a postimage.
pub(crate) fn plan_config(
    ini_path: PathRef,
    before: &[u8],
    set_path: Option<RenoDxSetPathValue>,
    config: Option<&RenoDxConfig>,
) -> Result<RenoDxConfigMutation, RenoDxConfigError> {
    let desired_count = validate_desired_config(set_path, config)?;
    if desired_count == 0 {
        return Err(RenoDxConfigError::Ambiguous("empty RenoDX configuration"));
    }
    let mut doc = IniDocument::parse(before)?;
    let section_preexisted = doc.section_exists()?;
    let mut entries = Vec::with_capacity(desired_count);
    let mut newline_anchor = None;
    let mut changed = false;
    for_each_desired_config(set_path, config, |key, numeric| {
        let planned = doc.plan_key(key, numeric)?;
        if newline_anchor.is_none() {
            newline_anchor = planned.newline_anchor;
        }
        changed |= planned.changed;
        entries.push(RenoDxConfigEntry {
            key: key.to_owned(),
            baseline: planned.baseline,
            last_written: numeric,
        });
        Ok(())
    })?;
    Ok(RenoDxConfigMutation {
        after: doc.to_bytes(),
        receipt: RenoDxConfigReceipt::from_entries(
            ini_path,
            section_preexisted,
            newline_anchor,
            entries,
        ),
        changed,
    })
}

/// Reconciles all keys previously owned by RenderPilot and the new desired
/// set in one postimage. Every prior key is a CAS: external edits fail closed.
pub(crate) fn plan_config_reconcile(
    ini_path: PathRef,
    before: Option<&[u8]>,
    set_path: Option<RenoDxSetPathValue>,
    config: Option<&RenoDxConfig>,
    receipt: Option<&RenoDxConfigReceipt>,
) -> Result<RenoDxConfigReconcile, RenoDxConfigError> {
    let Some(receipt) = receipt else {
        let planned = plan_config(ini_path, before.unwrap_or_default(), set_path, config)?;
        return Ok(RenoDxConfigReconcile {
            after: Some(planned.after),
            receipt: Some(planned.receipt),
            changed: planned.changed || before.is_none(),
        });
    };
    if !receipt.is_supported()
        || !matches!(
            normalized_path_relation(receipt.ini_path.as_str(), ini_path.as_str()),
            NormalizedPathRelation::Equal
        )
    {
        return Err(RenoDxConfigError::InvalidReceipt);
    }
    let desired_count = validate_desired_config(set_path, config)?;
    let Some(before) = before else {
        if desired_count != 0 {
            return Err(RenoDxConfigError::Ambiguous(
                "receipt-owned ReShade.ini is missing",
            ));
        }
        return Ok(RenoDxConfigReconcile {
            after: None,
            receipt: None,
            changed: false,
        });
    };
    let mut doc = IniDocument::parse(before)?;
    let prior = receipt.managed_entries();
    doc.verify_owned_entries(prior.as_ref())?;
    let mut entries = Vec::new();
    let mut changed = false;
    let mut anchor = receipt.newline_anchor.clone();
    for_each_desired_config(set_path, config, |key, numeric| {
        let prior_entry = prior
            .iter()
            .find(|entry| entry.key.eq_ignore_ascii_case(key));
        let planned = doc.plan_key(key, numeric)?;
        changed |= planned.changed;
        if anchor.is_none() {
            anchor = planned.newline_anchor;
        }
        entries.push(RenoDxConfigEntry {
            key: key.to_owned(),
            baseline: prior_entry
                .map(|entry| entry.baseline.clone())
                .unwrap_or(planned.baseline),
            last_written: numeric,
        });
        Ok(())
    })?;

    for old in prior.iter() {
        if desired_contains(set_path, config, &old.key) {
            continue;
        }
        let removed = doc.remove_owned_key_with_policy(
            old,
            receipt.section_preexisted,
            RemovalPolicy::Strict,
        )?;
        changed |= removed;
    }
    if entries.is_empty() && changed {
        let anchor_changed = doc.restore_newline_anchor(receipt.newline_anchor.as_deref());
        changed |= anchor_changed;
    }
    let next_receipt = if entries.is_empty() {
        None
    } else {
        Some(RenoDxConfigReceipt::from_entries(
            ini_path,
            receipt.section_preexisted,
            anchor,
            entries,
        ))
    };
    Ok(RenoDxConfigReconcile {
        after: Some(doc.to_bytes()),
        receipt: next_receipt,
        changed,
    })
}
