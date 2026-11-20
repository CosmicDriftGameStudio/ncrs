use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// What was handed over and what to do with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A folder opens as the folder, a file as its folder with the cursor on
    /// the file.
    Open(PathBuf),
    /// Always the folder that holds the path, with the cursor on it: "Show in
    /// folder", also for a folder.
    Reveal(PathBuf),
}

/// What the panel is to show for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Folder(PathBuf),
    File { folder: PathBuf, name: OsString },
}

impl Request {
    pub fn path(&self) -> &Path {
        match self {
            Self::Open(path) | Self::Reveal(path) => path,
        }
    }

    /// `None` when the path does not exist (any more) or has no folder above.
    pub fn target(&self) -> Option<Target> {
        match self {
            Self::Open(path) if path.is_dir() => Some(Target::Folder(path.clone())),
            Self::Open(path) | Self::Reveal(path) => {
                let folder = path
                    .parent()
                    .filter(|folder| !folder.as_os_str().is_empty())?;
                let name = path.file_name()?;
                path.exists().then(|| Target::File {
                    folder: folder.to_path_buf(),
                    name: name.to_os_string(),
                })
            }
        }
    }
}

/// The path a command line argument names: a `file://` URI is decoded (the
/// Linux launchers pass `%U`), a relative path is taken from `cwd`.
pub fn path_from_argument(argument: &OsStr, cwd: &Path) -> PathBuf {
    let path = argument
        .to_str()
        .and_then(path_from_file_uri)
        .unwrap_or_else(|| PathBuf::from(argument));
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

/// The requests for the URIs of a D-Bus call; what is no local `file://` URI
/// is left out.
pub fn requests_from_uris(uris: &[String], reveal: bool) -> Vec<Request> {
    uris.iter()
        .filter_map(|uri| path_from_file_uri(uri))
        .map(|path| {
            if reveal {
                Request::Reveal(path)
            } else {
                Request::Open(path)
            }
        })
        .collect()
}

fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let (scheme, rest) = uri.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    if !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost")) || path.is_empty() {
        return None;
    }
    let without_query = path.split(['?', '#']).next().unwrap_or(path);
    let decoded = decode_percent(without_query);
    if decoded.contains(&0) {
        return None;
    }
    Some(path_of_bytes(decoded))
}

fn decode_percent(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match escaped {
            Some(value) => {
                decoded.push(value);
                at += 3;
            }
            None => {
                decoded.push(byte);
                at += 1;
            }
        }
    }
    decoded
}

#[cfg(unix)]
fn path_of_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;
    PathBuf::from(OsString::from_vec(bytes))
}

/// `/C:/Users/me` of a Windows URI is `C:/Users/me`.
#[cfg(not(unix))]
fn path_of_bytes(bytes: Vec<u8>) -> PathBuf {
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let drive_path = text
        .strip_prefix('/')
        .filter(|rest| rest.get(1..2) == Some(":"));
    PathBuf::from(drive_path.unwrap_or(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_uri_is_decoded_to_its_path() {
        assert_eq!(
            path_from_file_uri("file:///home/me/My%20Folder/a%C3%A4.txt"),
            Some(PathBuf::from("/home/me/My Folder/aä.txt"))
        );
        assert_eq!(
            path_from_file_uri("FILE://localhost/tmp/x"),
            Some(PathBuf::from("/tmp/x"))
        );
    }

    #[test]
    fn uris_of_other_hosts_and_other_schemes_are_no_local_path() {
        assert_eq!(path_from_file_uri("file://server/share/x"), None);
        assert_eq!(path_from_file_uri("https://example.org/x"), None);
        assert_eq!(path_from_file_uri("/plain/path"), None);
    }

    #[test]
    fn an_escaped_nul_makes_the_uri_no_path() {
        assert_eq!(path_from_file_uri("file:///tmp/a%00b"), None);
        let uris = vec!["file:///tmp/a%00b".to_owned()];
        assert!(requests_from_uris(&uris, false).is_empty());
    }

    #[test]
    fn a_broken_escape_stays_as_typed() {
        assert_eq!(
            path_from_file_uri("file:///tmp/100%/a%zz"),
            Some(PathBuf::from("/tmp/100%/a%zz"))
        );
    }

    #[test]
    fn a_relative_argument_is_taken_from_the_working_folder() {
        let cwd = Path::new("/work");
        assert_eq!(
            path_from_argument(OsStr::new("sub/a.txt"), cwd),
            Path::new("/work").join("sub/a.txt")
        );
        assert_eq!(
            path_from_argument(OsStr::new("file:///etc/hosts"), cwd),
            PathBuf::from("/etc/hosts")
        );
    }

    #[test]
    fn uris_become_open_or_reveal_requests_and_drop_what_is_no_file_uri() {
        let uris = vec!["file:///a".to_owned(), "http://x/y".to_owned()];
        assert_eq!(
            requests_from_uris(&uris, true),
            vec![Request::Reveal(PathBuf::from("/a"))]
        );
        assert_eq!(
            requests_from_uris(&uris, false),
            vec![Request::Open(PathBuf::from("/a"))]
        );
    }

    #[test]
    fn a_folder_opens_as_the_folder_and_a_file_as_its_folder_with_the_cursor_on_it() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let file = scratch.path().join("a.txt");
        std::fs::write(&file, "x").expect("file");
        assert_eq!(
            Request::Open(scratch.path().to_path_buf()).target(),
            Some(Target::Folder(scratch.path().to_path_buf()))
        );
        let expected = Some(Target::File {
            folder: scratch.path().to_path_buf(),
            name: OsString::from("a.txt"),
        });
        assert_eq!(Request::Open(file.clone()).target(), expected);
        assert_eq!(Request::Reveal(file).target(), expected);
    }

    #[test]
    fn revealing_a_folder_shows_its_parent_with_the_cursor_on_it() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let inner = scratch.path().join("inner");
        std::fs::create_dir(&inner).expect("folder");
        assert_eq!(
            Request::Reveal(inner).target(),
            Some(Target::File {
                folder: scratch.path().to_path_buf(),
                name: OsString::from("inner"),
            })
        );
    }

    #[test]
    fn a_path_that_does_not_exist_has_no_target() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        assert_eq!(
            Request::Open(scratch.path().join("gone.txt")).target(),
            None
        );
    }
}
