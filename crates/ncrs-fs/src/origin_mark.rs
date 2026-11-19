//! The mark that says a file came from the internet: the `Zone.Identifier`
//! stream on Windows (Word, Excel and SmartScreen go by it), the
//! `com.apple.quarantine` attribute on macOS (Gatekeeper). A copy or an unpacked
//! file that does not carry it opens without the warning its original had, so
//! every path that places a file from another file uses this one helper.

use std::io;
use std::path::Path;

#[cfg(any(windows, test))]
const ZONE_STREAM: &str = "Zone.Identifier";

#[cfg(any(windows, test))]
fn zone_stream_of(file: &Path) -> std::path::PathBuf {
    let mut stream = file.as_os_str().to_owned();
    stream.push(":");
    stream.push(ZONE_STREAM);
    stream.into()
}

/// Writes the zone mark of `origin` onto `copy`, unchanged. An origin without
/// a mark, or on a file system that has no streams, leaves the copy as it is.
#[cfg(any(windows, test))]
fn carry_zone_stream(origin: &Path, copy: &Path) -> io::Result<()> {
    let Ok(mark) = std::fs::read(zone_stream_of(origin)) else {
        return Ok(());
    };
    match std::fs::write(zone_stream_of(copy), mark) {
        Err(error) if no_streams(&error) => Ok(()),
        other => other,
    }
}

/// FAT, exFAT and shares without streams refuse the stream name; there the
/// copy cannot carry a mark and nothing reads one.
#[cfg(any(windows, test))]
fn no_streams(error: &io::Error) -> bool {
    const ERROR_NOT_SUPPORTED: i32 = 50;
    const ERROR_INVALID_NAME: i32 = 123;
    matches!(
        error.raw_os_error(),
        Some(ERROR_NOT_SUPPORTED | ERROR_INVALID_NAME)
    )
}

#[cfg(target_os = "macos")]
fn carry_quarantine(origin: &Path, copy: &Path) -> io::Result<()> {
    use rustix::fs::{lgetxattr, lgetxattr_query, lsetxattr, XattrFlags};
    use rustix::io::Errno;
    const NAME: &str = "com.apple.quarantine";
    let len = match lgetxattr_query(origin, NAME) {
        Ok(len) => len,
        Err(Errno::NOATTR | Errno::NOTSUP) => return Ok(()),
        Err(other) => return Err(other.into()),
    };
    let mut value = vec![0; len];
    let got = lgetxattr(origin, NAME, &mut value)?;
    match lsetxattr(
        copy,
        NAME,
        value.get(..got).unwrap_or_default(),
        XattrFlags::empty(),
    ) {
        // A file system that holds no attributes has nothing Gatekeeper reads.
        Ok(()) | Err(Errno::NOTSUP) => Ok(()),
        Err(other) => Err(other.into()),
    }
}

/// Puts the origin mark of `origin` on `copy`. A mark that cannot be put on
/// the copy is an error: an unmarked copy of a marked file is never shown.
/// Linux has no such mark, and nothing is done there.
pub fn origin_mark(origin: &Path, copy: &Path) -> io::Result<()> {
    #[cfg(windows)]
    carry_zone_stream(origin, copy)?;
    #[cfg(target_os = "macos")]
    carry_quarantine(origin, copy)?;
    let _ = (origin, copy);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Outside Windows the stream is a file with a colon in its name, which is
    // enough to see what is read from where and written to where.
    #[test]
    fn the_zone_mark_of_the_origin_is_written_to_the_copy_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("invoice.zip");
        let copy = dir.path().join("invoice.docm");
        std::fs::write(&copy, "x").unwrap();
        std::fs::write(
            dir.path().join("invoice.zip:Zone.Identifier"),
            "[ZoneTransfer]\r\nZoneId=3\r\n",
        )
        .unwrap();

        carry_zone_stream(&archive, &copy).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("invoice.docm:Zone.Identifier")).unwrap(),
            "[ZoneTransfer]\r\nZoneId=3\r\n"
        );
    }

    #[test]
    fn a_file_system_without_streams_is_not_a_failed_copy() {
        assert!(no_streams(&io::Error::from_raw_os_error(123)));
        assert!(no_streams(&io::Error::from_raw_os_error(50)));
        assert!(!no_streams(&io::Error::from_raw_os_error(5)));
    }

    #[test]
    fn an_origin_without_a_mark_leaves_the_copy_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("a.docm");
        std::fs::write(&copy, "x").unwrap();

        carry_zone_stream(&dir.path().join("local.zip"), &copy).unwrap();

        assert!(!dir.path().join("a.docm:Zone.Identifier").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_quarantine_of_the_origin_is_put_on_the_copy() {
        use rustix::fs::{lgetxattr, lsetxattr, XattrFlags};
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("download.zip");
        let copy = dir.path().join("inside.app");
        std::fs::write(&origin, "x").unwrap();
        std::fs::write(&copy, "x").unwrap();
        let name = "com.apple.quarantine";
        if lsetxattr(&origin, name, b"0081;5f3a;Safari", XattrFlags::empty()).is_err() {
            return;
        }

        origin_mark(&origin, &copy).unwrap();

        let mut value = [0; 64];
        let got = lgetxattr(&copy, name, &mut value[..]).unwrap();
        assert_eq!(value.get(..got), Some(&b"0081;5f3a;Safari"[..]));
    }
}
