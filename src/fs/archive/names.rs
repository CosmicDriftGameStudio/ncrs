//! The one rule that decides whether an archive entry may exist at all.

/// The components of an entry name, or `None` when the entry must be refused.
///
/// Archives carry names written by someone else: an absolute path, a `..`, a
/// drive letter or a device name would make an unpack write outside its target.
/// `Some(vec![])` is the archive root (`./`), which callers ignore.
pub fn safe_components(raw: &str) -> Option<Vec<String>> {
    if raw.starts_with('/') || raw.starts_with('\\') {
        return None;
    }
    let mut components: Vec<String> = Vec::new();
    for part in raw.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains('\0') {
            return None;
        }
        if components.is_empty() && is_drive_prefix(part) {
            return None;
        }
        if cfg!(windows) && (part.contains(':') || is_device_name(part) || ends_unseen(part)) {
            return None;
        }
        components.push(part.to_owned());
    }
    Some(components)
}

/// macOS zips carry each file's resource fork as a `._name` sibling and a
/// `__MACOSX` folder of them. They mean nothing off a Mac, so listings and
/// unpacks leave them out. Only archive entries are judged: files on disk
/// stay visible.
pub fn is_apple_double(components: &[String]) -> bool {
    components.iter().any(|part| part == "__MACOSX")
        || components.last().is_some_and(|name| name.starts_with("._"))
}

fn is_drive_prefix(part: &str) -> bool {
    let mut chars = part.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), None) if letter.is_ascii_alphabetic()
    )
}

/// Win32 cuts trailing dots and spaces off a name, so `evil.exe.` would become
/// `evil.exe`.
fn ends_unseen(part: &str) -> bool {
    part.ends_with(['.', ' '])
}

/// Windows opens `CON`, `NUL` and friends as devices whatever the extension.
pub fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            upper
                .strip_prefix(prefix)
                .is_some_and(|digit| matches!(digit.as_bytes(), [b'1'..=b'9']))
        })
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn parts(raw: &str) -> Option<Vec<String>> {
        safe_components(raw)
    }

    fn some(items: &[&str]) -> Option<Vec<String>> {
        Some(items.iter().map(|item| (*item).to_owned()).collect())
    }

    #[test]
    fn apple_double_entries_are_recognised() {
        let apple = |raw: &str| is_apple_double(&safe_components(raw).unwrap());
        for raw in ["__MACOSX", "__MACOSX/a/._b", "dir/._x", "._x", "__MACOSX/"] {
            assert!(apple(raw), "{raw}");
        }
        for raw in ["a._b", ".hidden", "_MACOSX", "dir/__MACOSXfoo", "._x/y"] {
            assert!(!apple(raw), "{raw}");
        }
    }

    #[test]
    fn ordinary_names_split_into_their_parts() {
        let table: [(&str, Option<Vec<String>>); 8] = [
            ("a.txt", some(&["a.txt"])),
            ("a/b/c.txt", some(&["a", "b", "c.txt"])),
            ("a\\b\\c.txt", some(&["a", "b", "c.txt"])),
            ("./a//b/", some(&["a", "b"])),
            ("a/./b", some(&["a", "b"])),
            ("./", some(&[])),
            ("", some(&[])),
            ("v1.0..2/x", some(&["v1.0..2", "x"])),
        ];
        for (raw, expected) in table {
            assert_eq!(parts(raw), expected, "{raw:?}");
        }
    }

    #[test]
    fn names_that_could_leave_the_target_are_refused() {
        for raw in [
            "/abs.txt",
            "\\\\server\\share\\x",
            "\\abs.txt",
            "../evil.txt",
            "a/../../evil.txt",
            "a/..",
            "a\\..\\evil",
            "C:/evil.txt",
            "z:\\evil",
            "a/b\0c",
        ] {
            assert_eq!(parts(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn a_drive_letter_is_only_refused_in_front() {
        assert_eq!(
            parts("a/c:"),
            if cfg!(windows) {
                None
            } else {
                some(&["a", "c:"])
            }
        );
    }

    #[test]
    fn windows_devices_are_refused_on_windows_only() {
        for raw in [
            "con",
            "CON.txt",
            "dir/nul",
            "Aux.tar.gz",
            "COM1",
            "lpt9.x",
            "a/prn ",
        ] {
            let expected = if cfg!(windows) {
                None
            } else {
                parts_unchecked(raw)
            };
            assert_eq!(parts(raw), expected, "{raw:?}");
        }
        assert!(parts("com0").is_some());
        assert!(parts("console").is_some());
        assert!(parts("COM10").is_some());
    }

    #[test]
    fn names_ending_in_a_dot_or_space_are_refused_on_windows_only() {
        for raw in ["evil.exe.", "evil.exe ", "dir/run.bat..", "a/b. "] {
            let expected = if cfg!(windows) {
                None
            } else {
                parts_unchecked(raw)
            };
            assert_eq!(parts(raw), expected, "{raw:?}");
        }
        assert!(parts("a.b/c .d").is_some());
    }

    fn parts_unchecked(raw: &str) -> Option<Vec<String>> {
        Some(
            raw.split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    }

    #[test]
    fn device_names_are_recognised_with_any_extension_or_case() {
        for name in [
            "CON", "con", "Nul.txt", "AUX", "prn.a.b", "COM1", "com9", "LPT5.log",
        ] {
            assert!(is_device_name(name), "{name}");
        }
        for name in ["CONSOLE", "COM0", "COM10", "LPT", "nulx", "xcon"] {
            assert!(!is_device_name(name), "{name}");
        }
    }
}
