use std::path::{Path, PathBuf};

use renderpilot_detection::{InstallTreeCompleteness, InstallTreeWalker, WalkDiagnosticKind};

/// Pre-ranking record gathered during the directory walk.
pub(super) struct RawCandidate {
    pub(super) absolute_path: PathBuf,
    pub(super) relative_path: String,
    pub(super) file_name: String,
    pub(super) file_name_no_ext: String,
    pub(super) size_bytes: u64,
    pub(super) depth: u32,
}

pub(super) fn collect_raw_candidates(
    root: &Path,
    walker: InstallTreeWalker,
    is_cancelled: impl Fn() -> bool,
) -> (
    Vec<RawCandidate>,
    Vec<PathBuf>,
    InstallTreeCompleteness,
    Vec<String>,
    usize,
) {
    let mut out = Vec::new();
    let mut structural_files = Vec::new();
    let Ok(report) = walker.walk_filtered_cancellable(
        root,
        |file_name| {
            let lower = file_name.to_ascii_lowercase();
            Path::new(file_name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                || is_structural_file_name(&lower)
        },
        is_cancelled,
    ) else {
        return (
            out,
            structural_files,
            InstallTreeCompleteness::Incomplete,
            vec![format!("could not inspect {}", root.display())],
            0,
        );
    };
    let completeness = report.completeness();
    let visited_entries = report.visited_entries();
    let diagnostics = report
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.kind() != WalkDiagnosticKind::DepthLimit)
        .map(|diagnostic| format!("{}: {}", diagnostic.path().display(), diagnostic.message()))
        .collect();

    for path in report.files() {
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_structural_file_name(&file_name.to_ascii_lowercase()) {
            structural_files.push(path.clone());
            continue;
        }
        let size_bytes = path.metadata().map_or(0, |m| m.len());
        let relative_path = relative_path_from(root, path);
        let depth = Path::new(&relative_path)
            .parent()
            .and_then(|parent| u32::try_from(parent.components().count()).ok())
            .unwrap_or(0);
        let file_name_no_ext = file_name
            .rsplit_once('.')
            .map_or_else(|| file_name.to_owned(), |(stem, _)| stem.to_owned());

        out.push(RawCandidate {
            absolute_path: path.clone(),
            relative_path,
            file_name: file_name.to_owned(),
            file_name_no_ext,
            size_bytes,
            depth,
        });
    }
    (
        out,
        structural_files,
        completeness,
        diagnostics,
        visited_entries,
    )
}

fn is_structural_file_name(lower_file_name: &str) -> bool {
    [".dll", ".pak", ".utoc", ".ucas", ".archive", ".bundle"]
        .iter()
        .any(|extension| lower_file_name.ends_with(extension))
}

fn relative_path_from(root: &Path, full: &Path) -> String {
    full.strip_prefix(root)
        .ok()
        .and_then(|rel| rel.to_str())
        .map_or_else(
            || {
                full.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned()
            },
            |s| s.replace('\\', "/"),
        )
}
