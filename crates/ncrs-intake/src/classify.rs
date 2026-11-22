use std::ffi::OsStr;
use std::path::{Component, Path};

use crate::image_text;
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
    /// The `body.txt` of a mail export: its folder holds the `.eml`, and the
    /// text belongs to that mail.
    MailBody,
}

/// A mail export keeps the mail text as `body.txt` beside its `.eml`; the text
/// belongs to the mail and is not a document of its own.
pub fn is_mail_body(name: &OsStr, folder_has_eml: bool) -> bool {
    folder_has_eml && name.eq_ignore_ascii_case("body.txt")
}

pub fn is_eml_file(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("eml"))
}

fn folder_has_eml(folder: &Path) -> bool {
    std::fs::read_dir(folder).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|entry| is_eml_file(&entry.file_name()))
    })
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

/// Decides from the name, plus a look at the folder for a `body.txt`, whether a
/// file in the location is taken in.
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
    // The folder is read only for a file that is named like a mail body.
    let named_like_mail_body = path
        .file_name()
        .is_some_and(|file_name| is_mail_body(file_name, true));
    if named_like_mail_body && path.parent().is_some_and(folder_has_eml) {
        return Err(Skip::MailBody);
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

/// Whether the text chain can read files of this kind: documents, and images
/// it can decode for OCR (not heic).
pub fn text_is_readable(path: &Path) -> bool {
    let extension = lowercase_extension(path);
    DOCUMENT_EXTENSIONS.contains(&extension.as_str())
        || image_text::EXTENSIONS.contains(&extension.as_str())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::location::LocationMode;

    #[test]
    fn documents_and_decodable_images_are_readable_and_heic_is_not() {
        for readable in [
            "a.pdf", "a.PDF", "a.docx", "a.odt", "a.txt", "a.md", "a.jpg", "a.JPEG", "a.png",
            "a.tif", "a.tiff",
        ] {
            assert!(text_is_readable(Path::new(readable)), "{readable}");
        }
        for unreadable in ["a.heic", "a", "a.xyz"] {
            assert!(!text_is_readable(Path::new(unreadable)), "{unreadable}");
        }
    }

    fn location(images: bool) -> Location {
        Location {
            path: PathBuf::from("/data/inbox"),
            mode: LocationMode::Inbox,
            images,
            continuous: false,
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
            continuous: false,
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
    fn a_body_text_beside_an_eml_is_skipped_and_a_lone_one_is_taken_in() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = Location {
            path: dir.path().to_path_buf(),
            mode: LocationMode::Inbox,
            images: false,
            continuous: false,
        };
        let mail = dir.path().join("mail");
        let lone = dir.path().join("lone");
        std::fs::create_dir_all(&mail).unwrap();
        std::fs::create_dir_all(&lone).unwrap();
        std::fs::write(mail.join("mail.eml"), "x").unwrap();
        std::fs::write(mail.join("Body.TXT"), "x").unwrap();
        std::fs::write(lone.join("body.txt"), "x").unwrap();

        assert_eq!(
            classify(&inbox, &mail.join("Body.TXT")),
            Err(Skip::MailBody)
        );
        assert_eq!(classify(&inbox, &lone.join("body.txt")), Ok(()));
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
