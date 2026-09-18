// -----------------------------------------------------------------------------
// Filter lists
// -----------------------------------------------------------------------------

/// Exact filename matches (case-insensitive, with or without `.exe`) that
/// are definitely not the main game binary. Launchers, support apps.
const NON_GAME_EXE_NAMES: &[&str] = &[
    "steam",
    "steamservice",
    "steamerrorreporter",
    "epicgameslauncher",
    "origin",
    "eadesktop",
    "ubisoftconnect",
    "gog galaxy",
    "galaxyclient",
    "battle.net",
    "rockstargameslauncher",
    "playnite",
    "setup",
    "unins000",
    "unins001",
    "eosbootstrapper",
    "easyanticheat",
    "easyanticheat_setup",
    "battleye",
    "anticheatexpert",
    "activationui",
    "touchup",
    "oalinst",
];

/// Filename suffixes (case-insensitive) that strongly imply a non-game
/// binary. Matched against the basename without the `.exe` extension.
const NON_GAME_EXE_SUFFIXES: &[&str] = &[
    "launcher",
    "setup",
    "install",
    "uninstall",
    "crashreport",
    "crashhandler",
    "updater",
    "update",
    "redist",
    "dxsetup",
    "vcredist",
    "configure",
    "settings",
    "benchmark",
    "server",
    "dedicated",
    "editor",
    "helper",
    "support",
    "tool",
    "anticheat",
    "bootstrapper",
    "prereqsetup",
    "diag",
    "reporter",
];

/// Substrings (case-insensitive) anywhere in the filename that imply
/// a non-game binary. Catches names like `CrashHandler_x64.exe`.
const NON_GAME_EXE_SUBSTRINGS: &[&str] = &[
    "crash",
    "report",
    "redist",
    "helper",
    "support",
    "config",
    "setup",
    "install",
    "uninstall",
    "launcher",
    "updater",
    "dxsetup",
    "vcredist",
    "anticheat",
    "battleye",
    "bootstrapper",
    "prereq",
    "cleanup",
];

/// Directory names (case-insensitive, exact segment match) that hold installer,
/// redistributable, or prerequisite binaries rather than the game itself. Any
/// executable located under one of these — at any depth — is not the game, no
/// matter what it is named (e.g. `__Installer/Cleanup.exe`, `_CommonRedist/.../
/// vc_redist.exe`). Matching the parent folder catches generically-named helpers
/// the filename filters miss.
const NON_GAME_DIR_SEGMENTS: &[&str] = &[
    "__installer",
    "_commonredist",
    "commonredist",
    "redist",
    "_redist",
    "redistributable",
    "redistributables",
    "directx",
    "vcredist",
    "dotnet",
    "prerequisites",
    "installers",
];

/// Articulates the specific heuristic rationale for segregating a `.exe` from the pool of
/// primary game candidates. This classification is preserved alongside the candidate record,
/// enabling the frontend UI to transparently justify the rejection and facilitate manual
/// override workflows should the heuristic prove overly aggressive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    /// Located under an entry in `NON_GAME_DIR_SEGMENTS` (installer/redist folder).
    NonGameLocation(String),
    /// Filename matched an entry in `NON_GAME_EXE_NAMES`.
    NonGameName(String),
    /// Filename ended with an entry in `NON_GAME_EXE_SUFFIXES`.
    NonGameSuffix(String),
    /// Filename contained an entry in `NON_GAME_EXE_SUBSTRINGS`.
    NonGameSubstring(String),
}

impl RejectionReason {
    /// Stable wire string for serialization to the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NonGameLocation(_) => "non_game_location",
            Self::NonGameName(_) => "non_game_name",
            Self::NonGameSuffix(_) => "non_game_suffix",
            Self::NonGameSubstring(_) => "non_game_substring",
        }
    }

    /// The exact filter token that matched.
    pub fn token(&self) -> &str {
        match self {
            Self::NonGameLocation(s)
            | Self::NonGameName(s)
            | Self::NonGameSuffix(s)
            | Self::NonGameSubstring(s) => s,
        }
    }
}

/// Returns the parent-directory segment of `relative_path` (forward-slash,
/// relative to the install root) that matches a known installer/redist folder,
/// case-insensitively. The final segment (the file name) is never considered.
fn non_game_dir_segment(relative_path: &str) -> Option<String> {
    let (parent, _) = relative_path.rsplit_once('/')?;
    parent.split('/').find_map(|segment| {
        NON_GAME_DIR_SEGMENTS
            .iter()
            .find(|&&dir| dir.eq_ignore_ascii_case(segment))
            .map(|&dir| dir.to_owned())
    })
}

pub(super) fn classify(
    relative_path: &str,
    name_no_ext: &str,
    full_name: &str,
) -> Option<RejectionReason> {
    // A parent installer/redist folder rejects the binary regardless of its name —
    // the strongest signal that an executable is not the game.
    if let Some(segment) = non_game_dir_segment(relative_path) {
        return Some(RejectionReason::NonGameLocation(segment));
    }

    let lower = name_no_ext.to_ascii_lowercase();

    for banned in NON_GAME_EXE_NAMES {
        if lower == *banned {
            return Some(RejectionReason::NonGameName((*banned).to_owned()));
        }
    }

    for suffix in NON_GAME_EXE_SUFFIXES {
        // Exact names were checked first, so those retain `NonGameName`;
        // this also rejects a bare suffix token as `NonGameSuffix`.
        if lower.ends_with(suffix) {
            return Some(RejectionReason::NonGameSuffix((*suffix).to_owned()));
        }
    }

    let lower_full = full_name.to_ascii_lowercase();
    for needle in NON_GAME_EXE_SUBSTRINGS {
        if lower_full.contains(needle) {
            return Some(RejectionReason::NonGameSubstring((*needle).to_owned()));
        }
    }

    None
}
