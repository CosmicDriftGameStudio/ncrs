use std::ffi::OsStr;
use std::path::{Component, Path};

use crate::location::Location;

const DOCUMENT_EXTENSIONS: [&str; 5] = ["pdf", "docx", "odt", "txt", "md"];
const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "heic", "tif", "tiff"];
const TEMPORARY_SUFFIXES: [&str; 3] = [".part", ".crdownload", ".tmp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Skip {
    Hidden,
    Temporary,
    Unsupported,
    ImagesOff,
    /// Never followed and never taken in: following one could leave the
    /// location.
    Symlink,
    /// The path is not below the location's root.
    OutsideLocation,
    /// The user excluded the file or a folder around it from filing.
    Excluded,
}

pub fn is_hidden_name(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// The part of `path` below the location's root. A path with `.` or `..`
/// components is never inside: it could name a place outside once resolved.
pub fn relative_to_root<'path>(location: &Location, path: &'path Path) -> Option<&'path Path> {
    let relative = path.strip_prefix(&location.path).ok()?;
    relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then_some(relative)
}

/// Decides from the name alone whether a file in the location is taken in.
pub fn classify(location: &Location, path: &Path) -> Result<(), Skip> {
    let relative = relative_to_root(location, path).ok_or(Skip::OutsideLocation)?;
    let hidden = relative
        .components()
        .any(|part| matches!(part, Component::Normal(name) if is_hidden_name(name)));
    if hidden {
        return Err(Skip::Hidden);
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.starts_with("~$")
        || TEMPORARY_SUFFIXES
            .iter()
            .any(|suffix| name.ends_with(suffix))
    {
        return Err(Skip::Temporary);
    }
    let extension = lowercase_extension(path);
    if DOCUMENT_EXTENSIONS.contains(&extension.as_str()) {
        Ok(())
    } else if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        if location.images {
            Ok(())
        } else {
            Err(Skip::ImagesOff)
        }
    } else {
        Err(Skip::Unsupported)
    }
}

fn lowercase_extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Whether the file is a picture, whose text only OCR can read.
pub fn is_image_file(path: &Path) -> bool {
    IMAGE_EXTENSIONS.contains(&lowercase_extension(path).as_str())
}

/// Whether the text chain can read files of this kind. Images it cannot.
pub fn text_is_readable(path: &Path) -> bool {
    DOCUMENT_EXTENSIONS.contains(&lowercase_extension(path).as_str())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::location::LocationMode;

    #[test]
    fn documents_are_readable_as_text_and_images_are_not() {
        for readable in ["a.pdf", "a.PDF", "a.docx", "a.odt", "a.txt", "a.md"] {
            assert!(text_is_readable(Path::new(readable)), "{readable}");
        }
        for unreadable in ["a.jpg", "a.png", "a.heic", "a.tif", "a", "a.xyz"] {
            assert!(!text_is_readable(Path::new(unreadable)), "{unreadable}");
        }
    }

    fn location(images: bool) -> Location {
        Location {
            path: PathBuf::from("/data/inbox"),
            mode: LocationMode::Inbox,
            images,
        }
    }

    fn verdict(images: bool, relative: &str) -> Result<(), Skip> {
        classify(&location(images), &Path::new("/data/inbox").join(relative))
    }

    #[test]
    fn documents_are_taken_in_whatever_the_case() {
        for name in [
            "a.pdf",
            "a.PDF",
            "b.Docx",
            "c.odt",
            "d.txt",
            "e.MD",
            "sub/f.pdf",
        ] {
            assert_eq!(verdict(false, name), Ok(()), "{name}");
        }
    }

    #[test]
    fn hidden_files_and_files_in_hidden_folders_are_skipped() {
        for name in [".a.pdf", ".hidden/a.pdf", "sub/.cache/deep/a.pdf"] {
            assert_eq!(verdict(false, name), Err(Skip::Hidden), "{name}");
        }
    }

    #[test]
    fn a_path_outside_the_location_or_with_dot_dot_is_outside() {
        for outside in ["/data/other/a.pdf", "/data/inbox/../other/a.pdf", "a.pdf"] {
            assert_eq!(
                classify(&location(false), Path::new(outside)),
                Err(Skip::OutsideLocation),
                "{outside}"
            );
        }
    }

    #[test]
    fn a_hidden_folder_above_the_location_does_not_count() {
        let inside_hidden = Location {
            path: PathBuf::from("/home/.local/inbox"),
            mode: LocationMode::Inbox,
            images: false,
        };

        let result = classify(&inside_hidden, Path::new("/home/.local/inbox/a.pdf"));

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn temporary_names_are_skipped_before_the_extension_is_looked_at() {
        for name in [
            "~$a.docx",
            "a.pdf.part",
            "a.pdf.PART",
            "a.pdf.crdownload",
            "a.tmp",
            "a.pdf.Tmp",
        ] {
            assert_eq!(verdict(true, name), Err(Skip::Temporary), "{name}");
        }
    }

    #[test]
    fn images_need_the_switch() {
        for name in ["a.jpg", "a.JPEG", "a.png", "a.heic", "a.tif", "a.Tiff"] {
            assert_eq!(verdict(false, name), Err(Skip::ImagesOff), "{name}");
            assert_eq!(verdict(true, name), Ok(()), "{name}");
        }
    }

    #[test]
    fn other_extensions_and_none_are_unsupported() {
        for name in ["a.zip", "a.exe", "README", "a.pdf.bak", "a."] {
            assert_eq!(verdict(true, name), Err(Skip::Unsupported), "{name}");
        }
    }
}
