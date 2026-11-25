//! Where a rendered target puts a file below its root.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The name the rules gave, with the extension of the file: a filing pattern
/// describes the name without it.
pub fn target_of(root: &Path, relative: &Path, source: &Path) -> PathBuf {
    let mut name: OsString = relative.as_os_str().to_owned();
    if let Some(extension) = source.extension() {
        let has_it = relative
            .extension()
            .is_some_and(|own| own.eq_ignore_ascii_case(extension));
        if !has_it {
            name.push(".");
            name.push(extension);
        }
    }
    root.join(name)
}
