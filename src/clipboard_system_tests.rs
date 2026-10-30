//! "Copy files" and paste against the real clipboard of this machine, read and
//! written the way the Finder and Explorer do it: file URLs on the general
//! pasteboard, `CF_HDROP` on Windows.
//!
//! Linux has no such test: the CI runner has no desktop session, so there is
//! no clipboard owner to talk to.
//!
//! Each test takes a [`ClipboardSnapshot`] first. It holds what the clipboard
//! held and puts it back when the test ends, also after a failed assertion, so
//! a local run leaves the developer's clipboard as it was.

// reason: a failing assertion is the signal in a test
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::clipboard_paste::{read_system_clipboard, FileListWriter, PastedContent};

/// One test at a time on the clipboard of the machine.
static ON_THE_CLIPBOARD: Mutex<()> = Mutex::new(());

/// The content of the clipboard when the test began, put back when this is
/// dropped. Holding it also keeps every other clipboard test of this run out.
pub struct ClipboardSnapshot {
    saved: platform::Saved,
    _turn: MutexGuard<'static, ()>,
}

impl ClipboardSnapshot {
    pub fn take() -> Self {
        let turn = ON_THE_CLIPBOARD
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Self {
            saved: platform::save(),
            _turn: turn,
        }
    }
}

impl Drop for ClipboardSnapshot {
    fn drop(&mut self) {
        platform::put_back(&self.saved);
    }
}

fn canonical(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|path| {
            std::fs::canonicalize(path)
                .unwrap_or_else(|err| panic!("{} cannot be resolved: {err}", path.display()))
        })
        .collect()
}

/// A file, a folder and a name that needs escaping in a URL.
fn three_entries(root: &Path) -> Vec<PathBuf> {
    let file = root.join("alpha.txt");
    std::fs::write(&file, "alpha").unwrap();
    let folder = root.join("docs");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("inner.txt"), "inner").unwrap();
    let escaped = root.join("Größe 100% #1.txt");
    std::fs::write(&escaped, "escaped").unwrap();
    vec![file, folder, escaped]
}

fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("ncrs-clipboard-")
        .tempdir()
        .unwrap()
}

#[test]
fn copied_files_are_on_the_clipboard_as_the_file_manager_reads_them() {
    let _snapshot = ClipboardSnapshot::take();
    let dir = scratch();
    let files = three_entries(dir.path());

    FileListWriter::default().write(&files).unwrap();

    assert_eq!(
        canonical(&platform::files_as_the_file_manager_reads_them()),
        canonical(&files)
    );
}

#[test]
fn files_the_file_manager_put_on_the_clipboard_are_pasted() {
    let _snapshot = ClipboardSnapshot::take();
    let dir = scratch();
    let files = three_entries(dir.path());

    platform::put_files_as_the_file_manager_does(&files);

    let Some(PastedContent::Files(pasted)) = read_system_clipboard() else {
        panic!("the paste did not see the files");
    };
    assert_eq!(canonical(&pasted), canonical(&files));
}

#[test]
fn the_snapshot_gives_the_clipboard_back_as_it_was() {
    let _outer = ClipboardSnapshot::take();
    let mut clipboard = arboard::Clipboard::new().unwrap();
    clipboard.set_text("before the test").unwrap();
    let inner = platform::save();
    let dir = scratch();
    FileListWriter::default()
        .write(&three_entries(dir.path()))
        .unwrap();

    platform::put_back(&inner);

    assert_eq!(clipboard.get_text().unwrap(), "before the test");
    assert!(clipboard.get().file_list().is_err(), "the files stayed");
}

#[cfg(target_os = "macos")]
pub mod platform {
    use std::path::{Path, PathBuf};

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString, NSURL};

    /// The value of `NSPasteboardTypeFileURL`, which as an extern static can
    /// only be read in unsafe code.
    const FILE_URL_TYPE: &str = "public.file-url";

    /// Every item of the pasteboard with the data of each of its types.
    pub(in crate::clipboard_system_tests) struct Saved(
        Vec<Vec<(Retained<NSString>, Retained<NSData>)>>,
    );

    pub(in crate::clipboard_system_tests) fn save() -> Saved {
        let items = NSPasteboard::generalPasteboard()
            .pasteboardItems()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        item.types()
                            .iter()
                            .filter_map(|kind| {
                                let data = item.dataForType(&kind)?;
                                Some((kind, data))
                            })
                            .collect()
                    })
                    .collect()
            })
            .unwrap_or_default();
        Saved(items)
    }

    pub(in crate::clipboard_system_tests) fn put_back(saved: &Saved) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if saved.0.is_empty() {
            return;
        }
        let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = saved
            .0
            .iter()
            .map(|kinds| {
                let item = NSPasteboardItem::new();
                for (kind, data) in kinds {
                    item.setData_forType(data, kind);
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        if !pasteboard.writeObjects(&NSArray::from_retained_slice(&items)) {
            eprintln!("the clipboard could not be given back");
        }
    }

    /// The Finder reads one file URL per pasteboard item.
    pub(in crate::clipboard_system_tests) fn files_as_the_file_manager_reads_them() -> Vec<PathBuf>
    {
        let kind = NSString::from_str(FILE_URL_TYPE);
        let items = NSPasteboard::generalPasteboard()
            .pasteboardItems()
            .expect("the pasteboard has items");
        items
            .iter()
            .map(|item| {
                let text = item
                    .stringForType(&kind)
                    .unwrap_or_else(|| panic!("an item without a file URL: {:?}", item.types()));
                let url = NSURL::URLWithString(&text).expect("a valid URL");
                PathBuf::from(url.path().expect("a file URL has a path").to_string())
            })
            .collect()
    }

    /// The Finder writes file reference URLs (`file:///.file/id=…`); other
    /// programs write path URLs. The first entry goes as a reference.
    pub fn put_files_as_the_file_manager_does(files: &[PathBuf]) {
        let urls: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = files
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let path_url = file_url(path);
                let written = if index == 0 {
                    path_url.fileReferenceURL().expect("a reference URL")
                } else {
                    path_url
                };
                ProtocolObject::from_retained(written)
            })
            .collect();
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        assert!(
            pasteboard.writeObjects(&NSArray::from_retained_slice(&urls)),
            "the pasteboard took no file URLs"
        );
    }

    fn file_url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()))
    }
}

#[cfg(windows)]
mod platform {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use std::path::PathBuf;

    use clipboard_win::formats::{
        CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE, CF_HDROP,
        CF_METAFILEPICT, CF_OWNERDISPLAY, CF_PALETTE,
    };
    use clipboard_win::raw;

    /// Formats whose data is a handle rather than global memory, which cannot
    /// be copied as bytes. The system makes the bitmap again from the DIB.
    const HANDLE_FORMATS: [u32; 8] = [
        CF_BITMAP,
        CF_METAFILEPICT,
        CF_PALETTE,
        CF_ENHMETAFILE,
        CF_OWNERDISPLAY,
        CF_DSPBITMAP,
        CF_DSPMETAFILEPICT,
        CF_DSPENHMETAFILE,
    ];

    /// `sizeof(DROPFILES)`: the offset of the first path.
    const DROP_FILES_HEADER: usize = 20;

    pub(in crate::clipboard_system_tests) struct Saved(Vec<(u32, Vec<u8>)>);

    /// The clipboard history service opens the clipboard briefly after every
    /// change; every Windows program retries the open for that, arboard too.
    fn open() -> clipboard_win::Clipboard {
        clipboard_win::Clipboard::new_attempts(10).expect("the clipboard can be opened")
    }

    pub(in crate::clipboard_system_tests) fn save() -> Saved {
        let _open = open();
        let formats = raw::EnumFormats::new()
            .filter(|format| !HANDLE_FORMATS.contains(format))
            .filter_map(|format| {
                let mut data = Vec::new();
                raw::get_vec(format, &mut data).ok()?;
                Some((format, data))
            })
            .collect();
        Saved(formats)
    }

    pub(in crate::clipboard_system_tests) fn put_back(saved: &Saved) {
        let _open = open();
        if let Err(err) = raw::empty() {
            eprintln!("the clipboard could not be emptied: {err}");
            return;
        }
        for (format, data) in &saved.0 {
            if let Err(err) = raw::set_without_clear(*format, data) {
                eprintln!("format {format} could not be given back: {err}");
            }
        }
    }

    fn drop_files_on_the_clipboard() -> Vec<u8> {
        let _open = open();
        let mut bytes = Vec::new();
        raw::get_vec(CF_HDROP, &mut bytes).expect("CF_HDROP is on the clipboard");
        bytes
    }

    // reason: the clipboard format is little-endian by definition
    #[allow(clippy::little_endian_bytes)]
    fn field_at(bytes: &[u8], offset: usize) -> u32 {
        let field = bytes
            .get(offset..offset + 4)
            .expect("a whole DROPFILES header");
        u32::from_le_bytes(field.try_into().expect("four bytes"))
    }

    /// `DROPFILES`, then the paths as wide strings, each closed by a null and
    /// the list by one more.
    // reason: the clipboard format is little-endian by definition
    #[allow(clippy::little_endian_bytes)]
    pub(in crate::clipboard_system_tests) fn files_as_the_file_manager_reads_them() -> Vec<PathBuf>
    {
        let bytes = drop_files_on_the_clipboard();
        let start = usize::try_from(field_at(&bytes, 0)).expect("an offset");
        assert_eq!(field_at(&bytes, 16), 1, "the paths are not wide strings");
        let units: Vec<u16> = bytes
            .get(start..)
            .expect("the paths behind the header")
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        units
            .split(|unit| *unit == 0)
            .take_while(|path| !path.is_empty())
            .map(|path| PathBuf::from(OsString::from_wide(path)))
            .collect()
    }

    /// As Explorer writes it: plain drive paths, never the `\\?\` form.
    // reason: the clipboard format is little-endian by definition
    #[allow(clippy::little_endian_bytes)]
    pub(in crate::clipboard_system_tests) fn put_files_as_the_file_manager_does(files: &[PathBuf]) {
        let header = u32::try_from(DROP_FILES_HEADER).expect("a small offset");
        // pFiles, pt.x, pt.y, fNC, then fWide.
        let fields: [u32; 5] = [header, 0, 0, 0, 1];
        let mut bytes: Vec<u8> = fields
            .iter()
            .flat_map(|field| field.to_le_bytes())
            .collect();
        for path in files {
            let canonical = std::fs::canonicalize(path).unwrap();
            let shown = canonical.to_str().unwrap();
            let plain = shown.strip_prefix(r"\\?\").unwrap_or(shown);
            for unit in std::ffi::OsStr::new(plain).encode_wide().chain([0]) {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&[0, 0]);
        let _open = open();
        raw::set(CF_HDROP, &bytes).expect("CF_HDROP can be set");
    }
}
