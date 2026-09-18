use super::scan::RawCandidate;

/// Bonus for executables in the install directory root (depth 0).
/// Root-level files are far more likely to be the main game binary.
const ROOT_DEPTH_BONUS: i32 = 20;

/// Bonus for executables one level below root (depth 1).
/// Paths like `bin/Game.exe` are still plausible primary targets.
const NEAR_ROOT_DEPTH_BONUS: i32 = 5;

/// Bonus when the executable stem matches the install directory name after
/// normalization (non-alphanumerics stripped, lowercased). E.g. `Cyberpunk2077.exe`
/// inside `Cyberpunk 2077/`. Tolerant of spacing/punctuation differences the way a
/// launcher names a folder vs the game binary.
const FOLDER_NAME_MATCH_BONUS: i32 = 30;

/// Weaker bonus when one normalized name merely *contains* the other (e.g. an
/// `re2.exe` inside `Resident Evil 2/`, or a `WitcherLauncher` folder). Lower than
/// an exact match so the precise binary still wins.
const FOLDER_NAME_PARTIAL_BONUS: i32 = 12;

const MEGABYTE: u64 = 1024 * 1024;

/// Size threshold for the large-binary bonus.
const LARGE_BINARY_BYTES: u64 = 100 * MEGABYTE;

/// Size threshold for the medium-binary bonus.
const MEDIUM_BINARY_BYTES: u64 = 10 * MEGABYTE;

/// Bonus for binaries larger than [`LARGE_BINARY_BYTES`].
/// Capped to avoid letting size dominate games with small engines.
const LARGE_BINARY_BONUS: i32 = 10;

/// Bonus for binaries larger than [`MEDIUM_BINARY_BYTES`] but not large.
const MEDIUM_BINARY_BONUS: i32 = 3;

/// Normalizes a name for tolerant comparison: keeps only ASCII alphanumerics,
/// lowercased. So `"Cyberpunk 2077"` and `"Cyberpunk2077"` compare equal — matching
/// how a launcher names an install folder versus the game's binary. Mirrors NVIDIA
/// Profile Inspector's `NormalizeName`.
fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// True when one normalized name contains the other and the contained name is
/// substantial enough to be a meaningful signal (avoids matching on a 2–3 char
/// fragment). Backs the partial folder-name bonus.
fn folder_name_overlaps(stem: &str, dir: &str) -> bool {
    const MIN_OVERLAP: usize = 4;
    (dir.len() >= MIN_OVERLAP && stem.contains(dir))
        || (stem.len() >= MIN_OVERLAP && dir.contains(stem))
}

pub(super) fn compute_rank_score(raw: &RawCandidate, install_dir_name: &str) -> i32 {
    let mut score: i32 = 0;

    if raw.depth == 0 {
        score += ROOT_DEPTH_BONUS;
    } else if raw.depth == 1 {
        score += NEAR_ROOT_DEPTH_BONUS;
    }

    let normalized_dir = normalize_name(install_dir_name);
    let normalized_stem = normalize_name(&raw.file_name_no_ext);
    if !normalized_dir.is_empty() && !normalized_stem.is_empty() {
        if normalized_stem == normalized_dir {
            score += FOLDER_NAME_MATCH_BONUS;
        } else if folder_name_overlaps(&normalized_stem, &normalized_dir) {
            score += FOLDER_NAME_PARTIAL_BONUS;
        }
    }

    if raw.size_bytes > LARGE_BINARY_BYTES {
        score += LARGE_BINARY_BONUS;
    } else if raw.size_bytes > MEDIUM_BINARY_BYTES {
        score += MEDIUM_BINARY_BONUS;
    }

    score
}
