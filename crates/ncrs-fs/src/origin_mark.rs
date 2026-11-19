//! The mark Windows puts on a file that came from the internet (the
//! `Zone.Identifier` stream). Word, Excel and SmartScreen go by it, so a copy
//! unpacked out of a marked archive has to carry it on, or the viewer opens it
//! without Protected View.

use std::io;
use std::path::{Path, PathBuf};

const ZONE_STREAM: &str = "Zone.Identifier";

fn zone_stream_of(file: &Path) -> PathBuf {
    let mut stream = file.as_os_str().to_owned();
    stream.push(":");
    stream.push(ZONE_STREAM);
    PathBuf::from(stream)
}

/// Writes the mark of `archive` onto `copy`, unchanged. An archive without a
/// mark, or on a file system that has no streams, leaves the copy as it is. A
/// mark that cannot be put on the copy is an error: no unmarked copy is shown.
pub(in crate::archive) fn carry_zone_mark(archive: &Path, copy: &Path) -> io::Result<()> {
    let Ok(mark) = std::fs::read(zone_stream_of(archive)) else {
        return Ok(());
    };
    std::fs::write(zone_stream_of(copy), mark)
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::super::fixtures::scratch;
    use super::*;

    // Outside Windows the stream is a file with a colon in its name, which is
    // enough to see what is read from where and written to where.
    #[test]
    fn the_mark_of_the_archive_is_written_to_the_copy_unchanged() {
        let dir = scratch("zone-mark");
        let archive = dir.path().join("invoice.zip");
        let copy = dir.path().join("invoice.docm");
        std::fs::write(&copy, "x").unwrap();
        std::fs::write(
            dir.path().join("invoice.zip:Zone.Identifier"),
            "[ZoneTransfer]\r\nZoneId=3\r\n",
        )
        .unwrap();

        carry_zone_mark(&archive, &copy).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("invoice.docm:Zone.Identifier")).unwrap(),
            "[ZoneTransfer]\r\nZoneId=3\r\n"
        );
    }

    #[test]
    fn an_archive_without_a_mark_leaves_the_copy_without_one() {
        let dir = scratch("zone-mark-none");
        let archive = dir.path().join("local.zip");
        let copy = dir.path().join("a.docm");
        std::fs::write(&copy, "x").unwrap();

        carry_zone_mark(&archive, &copy).unwrap();

        assert!(!dir.path().join("a.docm:Zone.Identifier").exists());
    }
}
