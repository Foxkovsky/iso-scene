//! File access with an optional in-memory tree for WebAssembly.
//!
//! Native commands leave the tree empty and delegate to `std::fs`. The WASM
//! host fills it with the selected `.scene`, models, materials and textures;
//! every read in the compiler then goes through this module, so no filesystem
//! is required in the browser.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;

static FILES: RwLock<Option<BTreeMap<PathBuf, Vec<u8>>>> = RwLock::new(None);

/// Add or replace one in-memory file and switch to virtual reads.
///
/// # Errors
///
/// Returns an error for an empty path or a poisoned lock.
pub fn set_file(path: &str, bytes: Vec<u8>) -> Result<(), String> {
    let key = normalize(Path::new(path));
    if key.as_os_str().is_empty() {
        return Err("file path must not be empty".to_owned());
    }
    let mut slot = FILES
        .write()
        .map_err(|_| "virtual filesystem lock is poisoned".to_owned())?;
    slot.get_or_insert_with(BTreeMap::new).insert(key, bytes);
    Ok(())
}

/// Drop the in-memory tree and return to native reads.
pub fn clear_files() {
    if let Ok(mut slot) = FILES.write() {
        *slot = None;
    }
}

/// Whether reads currently go to the in-memory tree.
#[must_use]
pub fn is_virtual() -> bool {
    FILES.read().is_ok_and(|slot| slot.is_some())
}

/// Canonicalize a native path, or lexically normalize a virtual one.
///
/// # Errors
///
/// Returns the native `canonicalize` error; virtual paths don't touch disk.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    if is_virtual() {
        Ok(normalize(path))
    } else {
        path.canonicalize()
    }
}

/// Byte length of a file.
///
/// # Errors
///
/// Returns the native metadata error or a missing virtual file.
pub fn len(path: &Path) -> io::Result<u64> {
    if is_virtual() {
        let key = normalize(path);
        let slot = FILES
            .read()
            .map_err(|_| io::Error::other("virtual filesystem lock is poisoned"))?;
        let files = slot
            .as_ref()
            .ok_or_else(|| io::Error::other("virtual filesystem is empty"))?;
        return files
            .get(&key)
            .map(|bytes| bytes.len() as u64)
            .ok_or_else(|| missing(&key));
    }
    std::fs::metadata(path).map(|metadata| metadata.len())
}

/// Read one file.
///
/// # Errors
///
/// Returns the native read error or a missing virtual file.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    if is_virtual() {
        let key = normalize(path);
        let slot = FILES
            .read()
            .map_err(|_| io::Error::other("virtual filesystem lock is poisoned"))?;
        let files = slot
            .as_ref()
            .ok_or_else(|| io::Error::other("virtual filesystem is empty"))?;
        return files.get(&key).cloned().ok_or_else(|| missing(&key));
    }
    std::fs::read(path)
}

/// Read one file as UTF-8.
///
/// # Errors
///
/// Returns the read error or invalid UTF-8.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn missing(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("virtual file `{}` is missing", path.display()),
    )
}

/// Lexically collapse `.` and `..`; virtual keys are always relative.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => result.push(part),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{clear_files, is_virtual, read_to_string, set_file};

    #[test]
    fn a_virtual_file_wins_and_is_normalized() {
        clear_files();
        set_file("scenes/../model.model", b"version 1;".to_vec()).expect("set");
        assert!(is_virtual());
        assert_eq!(
            read_to_string(Path::new("./model.model")).expect("read"),
            "version 1;"
        );
        assert!(read_to_string(Path::new("missing.model")).is_err());
        clear_files();
        assert!(!is_virtual());
    }

    use std::path::Path;
}
