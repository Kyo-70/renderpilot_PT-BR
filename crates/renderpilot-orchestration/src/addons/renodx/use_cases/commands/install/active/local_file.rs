use std::path::Path;
use std::time::SystemTime;

use crate::ServiceError;
use crate::addons::renodx::errors;

/// Keep manually selected payloads bounded before passing them into the fetch
/// layer.  This is the same input contract as the inactive route.
const MAX_ADDON_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn read_addon_file(
    file_path: &str,
) -> Result<(Vec<u8>, Option<SystemTime>), ServiceError> {
    let path = Path::new(file_path);
    let metadata =
        std::fs::metadata(path).map_err(|error| errors::io("read add-on file", path, &error))?;
    if !metadata.is_file() {
        return Err(errors::invalid(
            "the selected add-on path is not a file".to_owned(),
        ));
    }
    if metadata.len() > MAX_ADDON_FILE_BYTES {
        return Err(errors::invalid(format!(
            "add-on file is too large (maximum {} MB)",
            MAX_ADDON_FILE_BYTES / (1024 * 1024)
        )));
    }
    let source_mtime = metadata.modified().ok();
    let bytes =
        std::fs::read(path).map_err(|error| errors::io("read add-on file", path, &error))?;
    Ok((bytes, source_mtime))
}
