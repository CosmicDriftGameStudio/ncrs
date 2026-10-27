use std::fmt;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{
    NSData, NSError, NSString, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSURL,
};

/// The Cocoa error domain and the code of `NSFileReadCorruptFileError`, which
/// is what resolving bytes that are no bookmark answers with.
const COCOA_ERROR_DOMAIN: &str = "NSCocoaErrorDomain";
const FILE_READ_CORRUPT: isize = 259;

/// Why a bookmark could not be made or resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkError {
    /// The bytes are no bookmark, now or ever; the file that holds them is
    /// worth nothing.
    Corrupt(String),
    /// Not right now: the volume is unplugged, the folder is gone for the
    /// moment, or the system refused. The bookmark may work again later.
    Unavailable(String),
}

impl fmt::Display for BookmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(reason) | Self::Unavailable(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for BookmarkError {}

fn classify(error: &NSError) -> BookmarkError {
    let reason = error.localizedDescription().to_string();
    if error.domain().to_string() == COCOA_ERROR_DOMAIN && error.code() == FILE_READ_CORRUPT {
        BookmarkError::Corrupt(reason)
    } else {
        BookmarkError::Unavailable(reason)
    }
}

/// A bookmark that keeps the access the user gave `path` in the open panel.
///
/// Call it right after the choice: the access the panel handed out is what
/// allows the bookmark to be made in the sandbox.
pub fn create_bookmark(path: &Path) -> Result<Vec<u8>, BookmarkError> {
    let text = path
        .to_str()
        .ok_or_else(|| BookmarkError::Unavailable("the path is not valid UTF-8".to_owned()))?;
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(text), path.is_dir());
    url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
        NSURLBookmarkCreationOptions::WithSecurityScope,
        None,
        None,
    )
    .map(|data| data.to_vec())
    .map_err(|error| classify(&error))
}

/// What a bookmark resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub path: PathBuf,
    /// The folder moved or was renamed; make a new bookmark from `path`.
    pub stale: bool,
    /// Whether the access to `path` is now open. Without it the folder stays
    /// out of reach.
    pub access_started: bool,
}

/// Resolves `bytes` and opens the access to what they name, without asking
/// the user and without mounting anything. The access stays open until the
/// process ends.
pub fn resolve_bookmark(bytes: &[u8]) -> Result<Resolved, BookmarkError> {
    let data = NSData::with_bytes(bytes);
    let mut stale = Bool::NO;
    let options = NSURLBookmarkResolutionOptions::WithSecurityScope
        | NSURLBookmarkResolutionOptions::WithoutUI
        | NSURLBookmarkResolutionOptions::WithoutMounting;
    let url = resolve(&data, options, &mut stale).map_err(|error| classify(&error))?;
    let access_started = start_access(&url);
    let path = url
        .path()
        .map(|text| PathBuf::from(text.to_string()))
        .ok_or_else(|| BookmarkError::Unavailable("the bookmark names no path".to_owned()))?;
    // Not released on purpose: dropping the URL would not end the access, but
    // the access is wanted for the whole run and there is no matching stop.
    let _kept = Retained::into_raw(url);
    Ok(Resolved {
        path,
        stale: stale.as_bool(),
        access_started,
    })
}

/// The folder a bookmark names, found without opening any access: no security
/// scope is requested, so nothing is started and nothing is kept. For comparing
/// a bookmark with a folder, as opposed to using it.
pub fn bookmark_path(bytes: &[u8]) -> Result<PathBuf, BookmarkError> {
    let data = NSData::with_bytes(bytes);
    let mut stale = Bool::NO;
    let options =
        NSURLBookmarkResolutionOptions::WithoutUI | NSURLBookmarkResolutionOptions::WithoutMounting;
    let url = resolve(&data, options, &mut stale).map_err(|error| classify(&error))?;
    url.path()
        .map(|text| PathBuf::from(text.to_string()))
        .ok_or_else(|| BookmarkError::Unavailable("the bookmark names no path".to_owned()))
}

#[allow(unsafe_code)]
fn resolve(
    data: &NSData,
    options: NSURLBookmarkResolutionOptions,
    stale: &mut Bool,
) -> Result<Retained<NSURL>, Retained<NSError>> {
    // SAFETY: `data` is a live NSData, `stale` points to a valid Bool for the
    // duration of the call, and no relative URL is passed.
    unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            data, options, None, stale,
        )
    }
}

#[allow(unsafe_code)]
fn start_access(url: &NSURL) -> bool {
    // SAFETY: `url` is a live NSURL that was just resolved from a
    // security-scoped bookmark; the call only reports whether access began.
    unsafe { url.startAccessingSecurityScopedResource() }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn bytes_that_are_no_bookmark_are_corrupt() {
        let error = resolve_bookmark(b"this is not a bookmark").unwrap_err();
        assert!(matches!(error, BookmarkError::Corrupt(_)), "{error:?}");
    }

    /// Outside the sandbox the app holds every access already, so this proves
    /// the bytes round-trip and the path survives; the part that matters for
    /// the store, access after a relaunch, only shows inside the sandbox (see
    /// the spike for #120).
    #[test]
    fn a_bookmark_resolves_to_the_folder_it_was_made_from() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().canonicalize().unwrap();

        let bytes = create_bookmark(&folder).unwrap();
        let resolved = resolve_bookmark(&bytes).unwrap();

        assert_eq!(resolved.path.canonicalize().unwrap(), folder);
        assert!(!resolved.stale);
    }

    #[test]
    fn the_path_alone_is_the_same_folder_and_bytes_that_are_no_bookmark_are_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().canonicalize().unwrap();
        let bytes = create_bookmark(&folder).unwrap();

        assert_eq!(
            bookmark_path(&bytes).unwrap().canonicalize().unwrap(),
            folder
        );
        assert!(matches!(
            bookmark_path(b"this is not a bookmark"),
            Err(BookmarkError::Corrupt(_))
        ));
    }
}
