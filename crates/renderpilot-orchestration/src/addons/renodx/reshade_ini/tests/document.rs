use renderpilot_domain::RenoDxSetPathValue;

use crate::addons::renodx::reshade_ini::{
    current_set_path_value, plan_set_path, plan_set_path_removal,
};

use super::ini_path;

#[test]
fn utf8_bom_is_preserved_and_does_not_mask_first_renodx_section() {
    let path = ini_path();
    let mut before = crate::addons::UTF8_BOM.to_vec();
    before.extend_from_slice(b"[renodx]\r\nSet_Path=0\r\n");

    // Value inquiry recognizes [renodx] even with BOM at line 1.
    assert_eq!(
        current_set_path_value(&before).expect("query"),
        Some("0".to_owned())
    );

    // Planning mutation modifies existing section instead of appending duplicate [renodx],
    // and preserves the BOM in the output.
    let planned = plan_set_path(path, &before, RenoDxSetPathValue::One).expect("plan");
    assert!(planned.changed);
    assert!(planned.after.starts_with(crate::addons::UTF8_BOM));
    let text = std::str::from_utf8(&planned.after[crate::addons::UTF8_BOM.len()..]).expect("utf8");
    assert_eq!(text, "[renodx]\r\nSet_Path=1\r\n");

    // Uninstall preserves BOM on roundtrip.
    let removed = plan_set_path_removal(&planned.after, &planned.receipt).expect("remove");
    assert!(removed.changed);
    let removed_bytes = removed.after.expect("removed bytes");
    assert_eq!(removed_bytes, before);
}

#[test]
fn utf8_bom_without_eof_newline_is_preserved() {
    let path = ini_path();
    let mut before = crate::addons::UTF8_BOM.to_vec();
    before.extend_from_slice(b"[General]\r\nfoo=bar");

    let planned = plan_set_path(path, &before, RenoDxSetPathValue::Zero).expect("plan");
    assert!(planned.after.starts_with(crate::addons::UTF8_BOM));

    let removed = plan_set_path_removal(&planned.after, &planned.receipt).expect("remove");
    assert_eq!(removed.after.as_deref(), Some(before.as_slice()));
}

#[test]
fn byte_reversibility_restores_eof_newline_only_when_anchor_is_unchanged() {
    let path = ini_path();
    let before_no_newline = b"[General]\nfoo=bar";
    let installed =
        plan_set_path(path, before_no_newline, RenoDxSetPathValue::Zero).expect("install");
    assert_eq!(installed.receipt.newline_anchor.as_deref(), Some("foo=bar"));

    // Exact uninstall: anchor matches, trailing newline is stripped, exact byte match.
    let uninstalled =
        plan_set_path_removal(&installed.after, &installed.receipt).expect("uninstall");
    assert_eq!(
        uninstalled.after.as_deref(),
        Some(before_no_newline.as_slice())
    );

    // User inserted a setting after install:
    let user_modified = b"[General]\nfoo=bar\nUserSetting=123\n[renodx]\nSet_Path=0\n".to_vec();
    let removed_user = plan_set_path_removal(&user_modified, &installed.receipt)
        .expect("uninstall with user line");
    // UserSetting=123 must NOT lose its newline because it does not match anchor "foo=bar".
    assert_eq!(
        removed_user.after.as_deref(),
        Some(b"[General]\nfoo=bar\nUserSetting=123\n".as_slice())
    );
}
