//! Installation analysis budget controls.

pub const MAX_METADATA_FILE_BYTES: usize = 64 * 1024; // 64 KiB
pub const MAX_STREAM_READ_PER_INSTALLATION: u64 = 64 * 1024 * 1024; // 64 MiB

/// Installation analysis budget manager.
#[derive(Debug)]
pub struct AnalysisBudget {
    pub section_stream_bytes_remaining: u64,
    pub max_helpers: usize,
}

impl Default for AnalysisBudget {
    fn default() -> Self {
        Self {
            section_stream_bytes_remaining: MAX_STREAM_READ_PER_INSTALLATION,
            max_helpers: 3,
        }
    }
}
