//! The picture in front of a file name: folder, link or kind of file. The
//! shapes are Lucide's (ISC, see `assets/icons/LICENSE`), drawn here as inline
//! bodies so one handle per kind can be cached.
use std::sync::LazyLock;

use iced::widget::svg::{self, Svg};
use iced::{Color, Element, Theme};
use ncrs_core::FileEntry;
use ncrs_i18n::Msg;

use crate::palette::Palette;

/// A folder with a meaning of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialFolder {
    Home,
    Desktop,
    Downloads,
    /// A folder Hermes reads from.
    ReadFolder,
    /// A folder Hermes files into.
    WriteFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Folder,
    FolderLink,
    Parent,
    Home,
    Desktop,
    Downloads,
    ReadFolder,
    WriteFolder,
    BrokenLink,
    FileLink,
    File,
    Text,
    Pdf,
    Sheet,
    Code,
    Image,
    Video,
    Audio,
    Archive,
    Program,
}

const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "rtf", "doc", "docx", "odt", "pages", "log", "tex",
];
const PDF_EXTENSIONS: &[&str] = &["pdf"];
const SHEET_EXTENSIONS: &[&str] = &["xls", "xlsx", "ods", "csv", "tsv", "numbers"];
const CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "json", "toml", "yaml", "yml", "html", "css", "c", "h",
    "cpp", "hpp", "java", "go", "rb", "php", "sh", "swift", "kt", "xml", "sql", "lua",
];
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "tif", "tiff", "heic", "ico", "avif",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "m4v", "wmv"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "aiff"];
const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "zst", "dmg", "iso",
];
const PROGRAM_EXTENSIONS: &[&str] = &["exe", "msi", "bat", "cmd", "com", "app", "appimage"];

const EXECUTE_BITS: u32 = 0o111;

/// What `entry` is drawn as. `special` is what the caller knows about a folder
/// from its place: home, desktop, a Hermes folder.
pub fn icon_kind(entry: &FileEntry, special: Option<SpecialFolder>) -> IconKind {
    if entry.is_parent {
        return IconKind::Parent;
    }
    // A link is checked before the folder: a link to a folder is a folder
    // too, and a broken one is neither.
    if entry.details.link_broken {
        return IconKind::BrokenLink;
    }
    if entry.is_symlink {
        return if entry.is_dir {
            IconKind::FolderLink
        } else {
            IconKind::FileLink
        };
    }
    if entry.is_dir {
        return match special {
            None => IconKind::Folder,
            Some(SpecialFolder::Home) => IconKind::Home,
            Some(SpecialFolder::Desktop) => IconKind::Desktop,
            Some(SpecialFolder::Downloads) => IconKind::Downloads,
            Some(SpecialFolder::ReadFolder) => IconKind::ReadFolder,
            Some(SpecialFolder::WriteFolder) => IconKind::WriteFolder,
        };
    }
    let extension = std::path::Path::new(&entry.name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());
    match extension.as_deref().and_then(kind_of_extension) {
        Some(kind) => kind,
        None if entry
            .details
            .mode
            .is_some_and(|mode| mode & EXECUTE_BITS != 0) =>
        {
            IconKind::Program
        }
        None => IconKind::File,
    }
}

fn kind_of_extension(extension: &str) -> Option<IconKind> {
    let kinds: [(&[&str], IconKind); 9] = [
        (TEXT_EXTENSIONS, IconKind::Text),
        (PDF_EXTENSIONS, IconKind::Pdf),
        (SHEET_EXTENSIONS, IconKind::Sheet),
        (CODE_EXTENSIONS, IconKind::Code),
        (IMAGE_EXTENSIONS, IconKind::Image),
        (VIDEO_EXTENSIONS, IconKind::Video),
        (AUDIO_EXTENSIONS, IconKind::Audio),
        (ARCHIVE_EXTENSIONS, IconKind::Archive),
        (PROGRAM_EXTENSIONS, IconKind::Program),
    ];
    kinds
        .into_iter()
        .find(|(extensions, _)| extensions.contains(&extension))
        .map(|(_, kind)| kind)
}

const FILE_SHEET: &str = r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>"#;
const FOLDER_SHAPE: &str = r#"<path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z"/>"#;

impl IconKind {
    pub const ALL: [IconKind; 20] = [
        Self::Folder,
        Self::FolderLink,
        Self::Parent,
        Self::Home,
        Self::Desktop,
        Self::Downloads,
        Self::ReadFolder,
        Self::WriteFolder,
        Self::BrokenLink,
        Self::FileLink,
        Self::File,
        Self::Text,
        Self::Pdf,
        Self::Sheet,
        Self::Code,
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::Archive,
        Self::Program,
    ];

    /// The text of the Type column for this kind.
    pub const fn type_message(self) -> Msg {
        match self {
            Self::Folder
            | Self::Parent
            | Self::Home
            | Self::Desktop
            | Self::Downloads
            | Self::ReadFolder
            | Self::WriteFolder => Msg::ColumnTypeFolder,
            Self::FolderLink | Self::FileLink => Msg::ColumnTypeLink,
            Self::BrokenLink => Msg::ColumnTypeBrokenLink,
            Self::File => Msg::ColumnTypeFile,
            Self::Text => Msg::ColumnTypeDocument,
            Self::Pdf => Msg::ColumnTypePdf,
            Self::Sheet => Msg::ColumnTypeSpreadsheet,
            Self::Code => Msg::ColumnTypeCode,
            Self::Image => Msg::ColumnTypeImage,
            Self::Video => Msg::ColumnTypeVideo,
            Self::Audio => Msg::ColumnTypeAudio,
            Self::Archive => Msg::ColumnTypeArchive,
            Self::Program => Msg::ColumnTypeProgram,
        }
    }

    const fn is_folder(self) -> bool {
        matches!(
            self,
            Self::Folder
                | Self::FolderLink
                | Self::Parent
                | Self::Home
                | Self::Desktop
                | Self::Downloads
        )
    }

    /// The shapes inside the 24-unit square.
    pub fn body(self) -> String {
        let on_sheet = |extra: &str| format!("{FILE_SHEET}{extra}");
        match self {
            Self::Folder => FOLDER_SHAPE.to_owned(),
            Self::FolderLink => r#"<path d="M2 9V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H2"/><path d="m8 16 3-3-3-3"/><path d="M2 16v-1a2 2 0 0 1 2-2h6"/>"#.to_owned(),
            Self::Parent => r#"<path d="M14 9 9 4 4 9"/><path d="M20 20h-7a4 4 0 0 1-4-4V4"/>"#.to_owned(),
            Self::Home => r#"<path d="M3 10 12 3l9 7v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1Z"/><path d="M9 21v-7h6v7"/>"#.to_owned(),
            Self::Desktop => r#"<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>"#.to_owned(),
            Self::Downloads => format!(
                r#"{FOLDER_SHAPE}<path d="M12 10v6"/><path d="m9 13 3 3 3-3"/>"#
            ),
            Self::ReadFolder => r#"<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>"#.to_owned(),
            Self::WriteFolder => r#"<rect x="2" y="3" width="20" height="5" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/>"#.to_owned(),
            Self::BrokenLink => r#"<path d="M9 17H7A5 5 0 0 1 7 7"/><path d="M15 7h2a5 5 0 0 1 4 8"/><path d="M8 12h4"/><path d="m2 2 20 20"/>"#.to_owned(),
            Self::FileLink => on_sheet(r#"<path d="m10 18 3-3-3-3"/><path d="M4 18v-1a2 2 0 0 1 2-2h6"/>"#),
            Self::File => FILE_SHEET.to_owned(),
            Self::Text => on_sheet(r#"<path d="M16 13H8"/><path d="M16 17H8"/><path d="M10 9H8"/>"#),
            Self::Pdf => on_sheet(r#"<path d="M9 18v-6h2a2 2 0 0 1 0 4H9"/>"#),
            Self::Sheet => on_sheet(r#"<path d="M8 13h2"/><path d="M14 13h2"/><path d="M8 17h2"/><path d="M14 17h2"/>"#),
            Self::Code => on_sheet(r#"<path d="m10 12-2 2.5 2 2.5"/><path d="m14 17 2-2.5-2-2.5"/>"#),
            Self::Video => on_sheet(r#"<path d="m10 11 5 3-5 3v-6Z"/>"#),
            Self::Audio => on_sheet(r#"<circle cx="10" cy="17" r="2"/><path d="M12 17v-6l3 1"/>"#),
            Self::Image => r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.09-3.09a2 2 0 0 0-2.82 0L6 21"/>"#.to_owned(),
            Self::Archive => r#"<rect x="2" y="3" width="20" height="5" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M12 11v2"/><path d="M12 15v2"/>"#.to_owned(),
            Self::Program => r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="m7 11 2-2-2-2"/><path d="M11 13h4"/>"#.to_owned(),
        }
    }

    fn handle(self) -> svg::Handle {
        static HANDLES: LazyLock<Vec<svg::Handle>> = LazyLock::new(|| {
            IconKind::ALL
                .iter()
                .map(|kind| svg::Handle::from_memory(document(&kind.body()).into_bytes()))
                .collect()
        });
        HANDLES
            .get(self as usize)
            .cloned()
            .unwrap_or_else(|| svg::Handle::from_memory(document(&self.body()).into_bytes()))
    }
}

fn document(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    )
}

/// The color of the symbol in a row. On the cursor row and on a tagged row of
/// the active panel it is the color of the name (`row_color`).
pub fn icon_color(
    kind: IconKind,
    palette: &Palette,
    row_color: Color,
    on_marked_row: bool,
) -> Color {
    if on_marked_row {
        row_color
    } else if kind.is_folder() {
        palette.directory
    } else {
        match kind {
            IconKind::ReadFolder | IconKind::WriteFolder => palette.primary_button().0,
            IconKind::BrokenLink => palette.error,
            _ => palette.dim_text,
        }
    }
}

/// The symbol of `kind` in `color`, `size` pixels square.
pub fn file_icon<'a, M: 'a>(kind: IconKind, color: Color, size: f32) -> Element<'a, M> {
    Svg::new(kind.handle())
        .width(size)
        .height(size)
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// The small sheet after the name of a file that has a note.
pub fn note_marker<'a, M: 'a>(color: Color, size: f32) -> Element<'a, M> {
    static HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| {
        svg::Handle::from_memory(
            document(r#"<path d="M5 3h10l4 4v14H5z"/><path d="M14 3v5h5"/><path d="M8.5 13h7M8.5 17h5"/>"#)
                .into_bytes(),
        )
    });
    Svg::new(LazyLock::force(&HANDLE).clone())
        .width(size)
        .height(size)
        .style(move |_: &Theme, _| svg::Style { color: Some(color) })
        .into()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> FileEntry {
        FileEntry {
            name: name.into(),
            path: name.into(),
            is_dir: false,
            is_symlink: false,
            is_parent: false,
            size: 0,
            modified: None,
            folder_size: None,
            details: Default::default(),
        }
    }

    fn folder(name: &str) -> FileEntry {
        FileEntry {
            is_dir: true,
            ..entry(name)
        }
    }

    #[test]
    fn files_are_told_apart_by_their_extension_in_any_case() {
        let kind = |name| icon_kind(&entry(name), None);
        assert_eq!(kind("Rechnung.PDF"), IconKind::Pdf);
        assert_eq!(kind("notes.md"), IconKind::Text);
        assert_eq!(kind("budget.xlsx"), IconKind::Sheet);
        assert_eq!(kind("main.rs"), IconKind::Code);
        assert_eq!(kind("photo.JPG"), IconKind::Image);
        assert_eq!(kind("clip.mkv"), IconKind::Video);
        assert_eq!(kind("song.flac"), IconKind::Audio);
        assert_eq!(kind("backup.tar"), IconKind::Archive);
        assert_eq!(kind("setup.exe"), IconKind::Program);
        assert_eq!(kind("README"), IconKind::File);
        assert_eq!(kind("data.unknown"), IconKind::File);
    }

    #[test]
    fn a_file_without_a_known_extension_is_a_program_when_it_can_be_run() {
        let mut tool = entry("tool");
        tool.details.mode = Some(0o755);
        assert_eq!(icon_kind(&tool, None), IconKind::Program);
        tool.details.mode = Some(0o644);
        assert_eq!(icon_kind(&tool, None), IconKind::File);
        let mut script = entry("run.sh");
        script.details.mode = Some(0o755);
        assert_eq!(icon_kind(&script, None), IconKind::Code);
    }

    #[test]
    fn links_and_the_parent_entry_have_their_own_symbols() {
        let mut link = folder("docs");
        link.is_symlink = true;
        assert_eq!(icon_kind(&link, None), IconKind::FolderLink);
        link.is_dir = false;
        assert_eq!(icon_kind(&link, None), IconKind::FileLink);
        link.details.link_broken = true;
        assert_eq!(icon_kind(&link, None), IconKind::BrokenLink);
        let mut parent = folder("..");
        parent.is_parent = true;
        assert_eq!(icon_kind(&parent, None), IconKind::Parent);
    }

    #[test]
    fn a_folder_with_a_place_of_its_own_gets_its_symbol_a_file_does_not() {
        assert_eq!(icon_kind(&folder("x"), None), IconKind::Folder);
        let special = |place| icon_kind(&folder("x"), Some(place));
        assert_eq!(special(SpecialFolder::Home), IconKind::Home);
        assert_eq!(special(SpecialFolder::Desktop), IconKind::Desktop);
        assert_eq!(special(SpecialFolder::Downloads), IconKind::Downloads);
        assert_eq!(special(SpecialFolder::ReadFolder), IconKind::ReadFolder);
        assert_eq!(special(SpecialFolder::WriteFolder), IconKind::WriteFolder);
        assert_eq!(
            icon_kind(&entry("x.pdf"), Some(SpecialFolder::Home)),
            IconKind::Pdf
        );
    }

    #[test]
    fn the_cache_index_of_every_kind_is_its_place_in_all() {
        for (index, kind) in IconKind::ALL.into_iter().enumerate() {
            assert_eq!(kind as usize, index, "{kind:?}");
            assert!(kind.body().starts_with('<'), "{kind:?}");
        }
    }
}
