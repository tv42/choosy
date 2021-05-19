use choosy_protocol as proto;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use tide::log;
use walkdir::{DirEntry, WalkDir};

fn is_interesting(entry: &DirEntry) -> bool {
    let ext = match entry.path().extension() {
        None => return false,
        Some(ext) => ext,
    };
    // rust is being painful, Path::extension() is a Cow
    match &*ext.to_string_lossy() {
        "mkv" | "mp4" | "avi" | "ogm" | "wmv" | "m4v" | "rmvb" | "flv" | "mov" | "mpg" => true,
        _ => false,
    }
}

pub fn scan(path: &Path) -> impl Iterator<Item = proto::FileChange> {
    fn is_hidden(entry: &DirEntry) -> bool {
        entry.file_name().as_bytes()[0] == b'.'
    }

    let walker = WalkDir::new(path)
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
                log::warn!("ignoring non-UTF-8 filename: {:?}", entry.path());
                return false;
            }
            true
        })
        .filter_map(|result| match result {
            Err(error) => {
                log::warn!("file scanning error: {}", error);
                None
            }
            Ok(entry) => Some(entry),
        })
        .filter(|entry| {
            let t = entry.file_type();
            if !t.is_file() && !t.is_symlink() {
                return false;
            }
            if !is_interesting(&entry) {
                return false;
            }
            true
        })
        .map(|entry| {
            // we filtered out non-UTF-8 entries earlier
            let p = entry.path().to_string_lossy();
            proto::FileChange::Add {
                name: p.to_string(),
            }
        });
    walker
}
