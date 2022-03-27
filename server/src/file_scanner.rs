use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
#[allow(unused_imports)]
use tracing::{debug, error, info, log, trace, warn};
use walkdir::{DirEntry, WalkDir};

fn is_interesting(entry: &DirEntry) -> bool {
    let ext = match entry.path().extension() {
        None => return false,
        Some(ext) => ext,
    };
    matches!(
        // "" will never match anything we're interested in.
        ext.to_str().unwrap_or(""),
        "mkv" | "mp4" | "avi" | "ogm" | "wmv" | "m4v" | "rmvb" | "flv" | "mov" | "mpg"
    )
}

pub fn scan(path: &Path) -> impl Iterator<Item = String> {
    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name().as_bytes()[0] == b'.'
    }

    let base = Arc::new(path.to_path_buf());
    WalkDir::new(path)
        .max_open(20)
        .same_file_system(true)
        .into_iter()
        .filter_entry(|entry: &DirEntry| {
            // filter_entry can only be used once (practically).
            // https://github.com/BurntSushi/walkdir/issues/130

            if is_hidden(entry) && entry.depth() > 0 {
                // explicitly allow passing hidden entries as the argument (because "." counts as hidden), but skip ones found in directory listings.
                return false;
            }
            if entry.file_name().to_str().is_none() {
                warn!(
                    message = "ignoring non-UTF-8 filename",
                    filename = ?entry.path()
                );
                return false;
            }
            true
        })
        .filter_map(|result| match result {
            Err(error) => {
                let is_lost_found = error
                    .path()
                    .and_then(|p| p.file_name())
                    .map(|f| f == "lost+found")
                    .unwrap_or(false);
                let is_eperm = error
                    .io_error()
                    .map(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
                    .unwrap_or(false);
                if is_lost_found && is_eperm {
                    // Hide errors about not being able to read directory `lost+found`, it's a special case for ext4 and we're likely to encounter it when media is stored on a separate "data disk".
                    return None;
                }

                warn!(message = "file scanning error", ?error);
                None
            }
            Ok(entry) => Some(entry),
        })
        .filter(|entry| {
            let t = entry.file_type();
            if !t.is_file() && !t.is_symlink() {
                return false;
            }
            if !is_interesting(entry) {
                return false;
            }
            true
        })
        .filter_map({
            move |entry| match entry.path().strip_prefix(&*base) {
                Err(error) => {
                    warn!(
                        message = "file scanning found file in wrong subtree",
                        ?error
                    );
                    None
                }
                Ok(relative) => {
                    // we filtered out non-UTF-8 entries earlier
                    let p = relative.to_string_lossy().to_string();
                    Some(p)
                }
            }
        })
}
