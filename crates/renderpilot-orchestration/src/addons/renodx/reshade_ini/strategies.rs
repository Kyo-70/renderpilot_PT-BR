//! Merge strategies for removing ReShade.ini sections and keys.

use crate::addons::engine::{IniSectionRemoval, MergeStrategy};
use crate::addons::reshade::ini_schema::{
    ADDON_PATH_KEY, ADDON_SECTION, DISABLED_ADDONS_KEY, DLSS_FIX_SECTION, LOAD_FROM_DLL_MAIN_KEY,
};

/// Builds the merge strategy to remove DLSS-Fix keys from `ReShade.ini`.
#[must_use]
pub(crate) fn ini_remove_dlss_fix_strategy() -> MergeStrategy {
    MergeStrategy::IniRemoveKeys {
        sections: vec![
            IniSectionRemoval {
                name: ADDON_SECTION.to_owned(),
                keys: vec![LOAD_FROM_DLL_MAIN_KEY.to_owned()],
            },
            IniSectionRemoval {
                name: DLSS_FIX_SECTION.to_owned(),
                keys: Vec::new(),
            },
        ],
    }
}

/// Builds the merge strategy an uninstall applies to a `ReShade.ini` RenoDX did
/// not create from scratch (so it is never blanket-deleted): removes exactly the
/// keys/sections RenoDX itself ever writes there — `[ADDON]` `DisabledAddons`,
/// `AddonPath`, and (when a DLSS-Fix companion was installed) `LoadFromDllMain`
/// plus the whole `[RENODX-DLSSFIX]` section — leaving every other key, section,
/// comment, and blank line (including the user's own settings) untouched.
#[must_use]
pub(crate) fn ini_remove_renodx_strategy() -> MergeStrategy {
    MergeStrategy::IniRemoveKeys {
        sections: vec![
            IniSectionRemoval {
                name: ADDON_SECTION.to_owned(),
                keys: vec![
                    DISABLED_ADDONS_KEY.to_owned(),
                    ADDON_PATH_KEY.to_owned(),
                    LOAD_FROM_DLL_MAIN_KEY.to_owned(),
                ],
            },
            IniSectionRemoval {
                name: DLSS_FIX_SECTION.to_owned(),
                keys: Vec::new(),
            },
        ],
    }
}
