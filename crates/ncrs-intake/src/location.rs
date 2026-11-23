use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::IntakeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocationMode {
    /// Documents are taken in and later filed away.
    Inbox,
    /// Indexed where they are. Read-only.
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// Canonical, see `Location::new`.
    pub path: PathBuf,
    pub mode: LocationMode,
    pub images: bool,
    /// Hermes files what the rules leave open by itself, so such a file gets
    /// no review entry.
    pub continuous: bool,
}

/// The path as the rules see it: relative to the innermost location root it
/// lies under, so folders above the root take no part in a folder condition.
/// A path under no root stays as it is.
pub fn relative_to_location<'a>(path: &'a Path, roots: &[PathBuf]) -> &'a Path {
    roots
        .iter()
        .filter_map(|root| path.strip_prefix(root).ok())
        .min_by_key(|relative| relative.components().count())
        .unwrap_or(path)
}

/// The innermost location root the path lies under.
pub fn location_of<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a Path> {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .map(PathBuf::as_path)
}

/// `canonicalize` returns `\\?\C:\...` or `\\?\UNC\server\...` on Windows, while
/// panels and `current_exe` show `C:\...` and `\\server\...`; without this the
/// same folder would count as two. The prefix stays where Win32 would read the
/// plain form differently: a name ending in a dot or a space, a device name
/// like `NUL`, a `/` in a name, or a path of `MAX_PATH` characters or more.
pub fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let plain = path.to_str().and_then(|text| {
        let rest = text.strip_prefix(r"\\?\")?;
        let plain = if let Some(share) = rest.strip_prefix(r"UNC\") {
            format!(r"\\{share}")
        } else {
            let mut chars = rest.chars();
            let is_drive = chars
                .next()
                .is_some_and(|letter| letter.is_ascii_alphabetic())
                && chars.next() == Some(':');
            if !is_drive {
                return None;
            }
            rest.to_owned()
        };
        is_same_when_plain(&plain).then(|| PathBuf::from(plain))
    });
    plain.unwrap_or(path)
}

const MAX_PLAIN_PATH_CHARS: usize = 260;

fn is_same_when_plain(plain: &str) -> bool {
    plain.chars().count() < MAX_PLAIN_PATH_CHARS
        && !plain.contains('/')
        && plain
            .split('\\')
            .all(|name| !name.ends_with(['.', ' ']) && !is_device_name(name))
}

/// Win32 reads `NUL`, `COM1`, `con.txt` and the like as devices whatever the
/// extension.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
        })
}

pub fn canonical_path(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path).map(without_verbatim_prefix)
}

impl Location {
    /// The path is canonicalised and must be an existing directory. Walk and
    /// watcher both work on this root: on macOS FSEvents reports
    /// `/private/var/...` for a directory that is spelled `/var/...`, and two
    /// spellings of one file would be recorded as a fake copy.
    pub fn new(path: &Path, mode: LocationMode, images: bool) -> Result<Self, IntakeError> {
        let canonical = canonical_path(path).map_err(IntakeError::io(path))?;
        if !canonical.is_dir() {
            return Err(IntakeError::io(path)(io::Error::new(
                io::ErrorKind::NotADirectory,
                "not a directory",
            )));
        }
        Ok(Self {
            path: canonical,
            mode,
            images,
            continuous: false,
        })
    }

    #[must_use]
    pub fn with_continuous(mut self, continuous: bool) -> Self {
        self.continuous = continuous;
        self
    }
}

#[cfg(test)]
mod relative_tests {
    use super::*;

    #[test]
    fn the_part_above_the_innermost_root_is_dropped() {
        let roots = [PathBuf::from("/a/Steuer"), PathBuf::from("/a/Steuer/Inbox")];
        let path = Path::new("/a/Steuer/Inbox/new/x.pdf");
        assert_eq!(relative_to_location(path, &roots), Path::new("new/x.pdf"));
        assert_eq!(relative_to_location(path, &[]), path);
    }
}

#[cfg(test)]
mod verbatim_tests {
    use super::*;

    #[test]
    fn the_verbatim_prefix_is_dropped_only_in_the_drive_and_share_forms() {
        let cases = [
            (r"\\?\C:\a", r"C:\a"),
            (r"\\?\c:\a", r"c:\a"),
            (r"\\?\UNC\s\x", r"\\s\x"),
            (r"\\?\Volume{1}\x", r"\\?\Volume{1}\x"),
            (r"\\?\C:\a.\x", r"\\?\C:\a.\x"),
            (r"\\?\C:\a \x", r"\\?\C:\a \x"),
            (r"\\?\C:\NUL\x", r"\\?\C:\NUL\x"),
            (r"\\?\C:\com1.txt", r"\\?\C:\com1.txt"),
            (r"\\?\UNC\s\aux", r"\\?\UNC\s\aux"),
            (r"\\?\C:\a.b\COM10", r"C:\a.b\COM10"),
            (r"C:\a", r"C:\a"),
            ("/data", "/data"),
        ];
        for (given, expected) in cases {
            assert_eq!(
                without_verbatim_prefix(PathBuf::from(given)),
                PathBuf::from(expected),
                "{given}"
            );
        }
    }
}
