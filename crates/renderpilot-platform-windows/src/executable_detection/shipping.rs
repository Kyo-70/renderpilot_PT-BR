/// Strips `suffix` from `s` ignoring ASCII case, without allocations,
/// safely verifying UTF-8 character boundaries.
#[inline]
fn strip_ascii_case_suffix<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let suffix_bytes = suffix.as_bytes();
    let s_bytes = s.as_bytes();
    if s_bytes.len() >= suffix_bytes.len() {
        let split_idx = s_bytes.len() - suffix_bytes.len();
        if s_bytes[split_idx..].eq_ignore_ascii_case(suffix_bytes) && s.is_char_boundary(split_idx)
        {
            return Some(&s[..split_idx]);
        }
    }
    None
}

/// Strips `.exe` extension case-insensitively without allocations and safely with Unicode.
#[inline]
pub(super) fn strip_exe_suffix(s: &str) -> &str {
    strip_ascii_case_suffix(s, ".exe").unwrap_or(s)
}

/// Suffixes identifying production shipping configuration binaries.
const SHIPPING_SUFFIXES: [&str; 4] = [
    "-win64-shipping",
    "-win32-shipping",
    "-shipping",
    "_shipping",
];

/// Strips the production shipping suffix from an executable name or stem without allocation.
///
/// Returns `Some(base_stem)` when a recognized shipping suffix is present and the base stem
/// is non-empty, or `None` otherwise.
///
/// Recognized suffixes (case-insensitive):
/// - `-Win64-Shipping`
/// - `-Win32-Shipping`
/// - `-Shipping`
/// - `_Shipping`
#[must_use]
pub fn strip_shipping_suffix(name_or_stem: &str) -> Option<&str> {
    let stem = strip_exe_suffix(name_or_stem);
    for suffix in SHIPPING_SUFFIXES {
        if let Some(base) = strip_ascii_case_suffix(stem, suffix)
            && !base.is_empty()
        {
            return Some(base);
        }
    }
    None
}

/// Returns whether an executable name or stem carries a production shipping suffix.
///
/// Pure and allocation-free.
#[must_use]
pub fn is_shipping_binary_name(name_or_stem: &str) -> bool {
    strip_shipping_suffix(name_or_stem).is_some()
}

/// Returns whether `candidate_name` is a shipping configuration binary bound to `launcher_name`.
///
/// Binding requires:
/// 1. `candidate_name` carries a valid production shipping suffix (`-Win64-Shipping`, `-Win32-Shipping`, `-Shipping`, `_Shipping`).
/// 2. The stripped base stem of `candidate_name` matches the base stem of `launcher_name` case-insensitively.
///
/// For example:
/// - `FactoryGameSteam-Win64-Shipping.exe` bound to `FactoryGameSteam.exe` => `true`
/// - `CrashReportClient-Win64-Shipping.exe` bound to `FactoryGameSteam.exe` => `false`
/// - `Unrelated-Shipping.exe` bound to `Game.exe` => `false`
#[must_use]
pub fn is_bound_shipping_target(candidate_name: &str, launcher_name: &str) -> bool {
    let launcher_stem = strip_exe_suffix(launcher_name);
    if launcher_stem.is_empty() {
        return false;
    }
    if let Some(candidate_base) = strip_shipping_suffix(candidate_name) {
        candidate_base.eq_ignore_ascii_case(launcher_stem)
    } else {
        false
    }
}
