//! Dense demo folders for the trailer: a project on the left, Downloads on the
//! right, both longer than the panel. Names are listed, sizes and dates are
//! derived from the name so a picture is the same on every run.
//! Nothing here touches the disk; everything lives under /Users/alex.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::app::{fixed_result_entry, App};
use crate::fs::search::LineMatch;
use crate::fs::FileEntry;
use crate::messages::PanelSide;
use ncrs_core::CompareMark;

pub const PROJECT_DIRECTORY: &str = "/Users/alex/Projects/ncrs";
pub const DOWNLOADS_DIRECTORY: &str = "/Users/alex/Downloads";
const BACKUP_DIRECTORY: &str = "/Volumes/Backup/ncrs";

pub const FRAME_SIZE: iced::Size = iced::Size::new(1920.0, 1080.0);

const FAVOURITES: &[&str] = &[
    "/Users/alex/Projects/ncrs",
    "/Users/alex/Downloads",
    "/Users/alex/Documents",
    "/Users/alex/Downloads/Invoices",
    "/Users/alex/Pictures",
];

/// A trailing slash marks a folder.
const PROJECT_NAMES: &[&str] = &[
    ".github/",
    ".cargo/",
    "assets/",
    "benches/",
    "crates/",
    "docs/",
    "examples/",
    "fuzz/",
    "packaging/",
    "scripts/",
    "src/",
    "target/",
    "tests/",
    "vendor/",
    ".dockerignore",
    ".editorconfig",
    ".gitattributes",
    ".gitignore",
    "about.toml",
    "build.rs",
    "Cargo.lock",
    "Cargo.toml",
    "CHANGELOG.md",
    "clippy.toml",
    "CLAUDE.md",
    "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md",
    "deny.toml",
    "Dockerfile",
    "entitlements.plist",
    "flake.lock",
    "flake.nix",
    "Info.plist",
    "install.sh",
    "justfile",
    "LICENSE",
    "ncrs.1",
    "ncrs.desktop",
    "notes.txt",
    "NOTICE",
    "README.md",
    "release-checklist.md",
    "renovate.json",
    "ROADMAP.md",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "screenshots.sh",
    "SECURITY.md",
    "strings.json",
    "taplo.toml",
    "todo-old.md",
    "typos.toml",
    "winget.yaml",
];

const DOWNLOADS_NAMES: &[&str] = &[
    "Installers/",
    "Invoices/",
    "Screenshots/",
    "Taxes 2025/",
    "Trips/",
    "Wallpapers/",
    "backup-2026-09.tar.gz",
    "bank-statement-2026-08.pdf",
    "bank-statement-2026-09.pdf",
    "boarding-pass.pdf",
    "budget-2026.xlsx",
    "contract-signed.pdf",
    "DaVinci_Resolve_19.dmg",
    "dataset.csv",
    "demo-reel.mp4",
    "Docker.dmg",
    "ebook-rust.epub",
    "export.json",
    "Firefox 131.0.dmg",
    "fonts.zip",
    "holiday-001.jpg",
    "holiday-002.jpg",
    "holiday-003.jpg",
    "holiday-004.jpg",
    "holiday-005.jpg",
    "holiday-006.jpg",
    "icons.zip",
    "IMG_2041.jpg",
    "IMG_2042.jpg",
    "IMG_2043.jpg",
    "IMG_2044.jpg",
    "IMG_2045.jpg",
    "invoice-2026-07.pdf",
    "invoice-2026-08.pdf",
    "invoice-2026-09.pdf",
    "lease-agreement.pdf",
    "logo-draft.svg",
    "logo-final-v2.png",
    "logo-final.png",
    "manual.pdf",
    "meeting-notes.docx",
    "node-v22.pkg",
    "photos-2025.zip",
    "podcast-ep42.mp3",
    "backup-project.zip",
    "receipt-apple.pdf",
    "resume-alex.pdf",
    "screenshot-2026-09-28.png",
    "screenshot-2026-09-29.png",
    "screenshot-2026-09-30.png",
    "slides-q3.pptx",
    "tax-return-2025.pdf",
    "ticket-concert.pdf",
    "vlc-3.0.21.dmg",
    "wallpaper-4k.jpg",
    "zoom.pkg",
];

const ARCHIVE_NAMES: &[&str] = &[
    "assets/",
    "benches/",
    "crates/",
    "docs/",
    "scripts/",
    "src/",
    "tests/",
    ".gitignore",
    "build.rs",
    "Cargo.lock",
    "Cargo.toml",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "deny.toml",
    "LICENSE",
    "README.md",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "strings.json",
    "todo-old.md",
    "notes.txt",
    "justfile",
    "NOTICE",
    "ROADMAP.md",
    "SECURITY.md",
    "Info.plist",
    "install.sh",
    "ncrs.1",
    "flake.nix",
    "flake.lock",
    "taplo.toml",
    "typos.toml",
    "about.toml",
    "clippy.toml",
    "Dockerfile",
    "renovate.json",
    "winget.yaml",
    "release-checklist.md",
    "screenshots.sh",
    "ncrs.desktop",
    "entitlements.plist",
    "CODE_OF_CONDUCT.md",
    ".editorconfig",
    ".gitattributes",
    ".dockerignore",
];

const SERVER_NAMES: &[&str] = &[
    "backups/",
    "documents/",
    "music/",
    "photos/",
    "projects/",
    "scans/",
    "videos/",
    "www/",
    "archive-2024.tar.gz",
    "archive-2025.tar.gz",
    "cv.pdf",
    "family-album.zip",
    "invoice-0914.pdf",
    "invoice-0915.pdf",
    "invoice-0916.pdf",
    "notes.txt",
    "passwords.kdbx",
    "plex.log",
    "report-2026.pdf",
    "router-config.bak",
    "server.log",
    "server.log.1",
    "server.log.2.gz",
    "shopping-list.md",
    "sync.conf",
    "taxes-2025.pdf",
    "todo.md",
    "wifi-qr.png",
    "backup.sh",
    "crontab.txt",
    "disk-usage.csv",
    "nas-readme.md",
    "speedtest.csv",
    "uptime.txt",
    "holiday-2026.mp4",
    "wedding.mp4",
    "recipes.docx",
    "budget.xlsx",
    "photos-2024.zip",
    "docker-compose.yml",
    "nginx.conf",
    "id_ed25519.pub",
    "authorized_keys",
    "history.log",
    "monitoring.json",
    "scan-0001.pdf",
    "scan-0002.pdf",
    "scan-0003.pdf",
    "scan-0004.pdf",
];

/// One line per hit: where, which line, the text.
const SEARCH_HITS: &[(&str, u64, &str)] = &[
    ("README.md", 12, "Say hello to a two-panel file manager."),
    ("src/main.rs", 8, "    println!(\"hello, world\");"),
    ("src/main.rs", 41, "    // hello from the main window"),
    (
        "src/app/mod.rs",
        212,
        "    /// The hello card shown on first start.",
    ),
    (
        "src/app/startup.rs",
        77,
        "        notice: Some(\"hello again\".to_owned()),",
    ),
    ("src/about.rs", 19, "const GREETING: &str = \"hello\";"),
    (
        "src/ui/titlebar.rs",
        133,
        "    // hello is also the first search suggestion",
    ),
    (
        "crates/ncrs-search/src/lib.rs",
        54,
        "    let query = \"hello\"; // doc example",
    ),
    (
        "crates/ncrs-search/src/view.rs",
        301,
        "        text(\"No hello here\").size(14)",
    ),
    (
        "crates/ncrs-fs/src/lib.rs",
        88,
        "/// Reads `hello.txt` to check the sandbox.",
    ),
    (
        "crates/ncrs-fs/src/archive.rs",
        410,
        "    assert_eq!(read(\"hello.txt\"), b\"hello\");",
    ),
    (
        "crates/ncrs-ops/src/copy.rs",
        27,
        "    // A file named hello.rs is copied like any other.",
    ),
    (
        "crates/ncrs-ui-kit/src/button.rs",
        15,
        "        button(\"hello\").on_press(msg)",
    ),
    ("docs/guide.md", 3, "A first hello for new users."),
    (
        "docs/keys.md",
        61,
        "Press F1 and type hello to find the greeting.",
    ),
    (
        "docs/architecture.md",
        140,
        "The hello module is the smallest example.",
    ),
    ("examples/hello.rs", 1, "fn main() { println!(\"hello\"); }"),
    (
        "tests/smoke.rs",
        9,
        "    assert!(output.contains(\"hello\"));",
    ),
    ("CHANGELOG.md", 96, "- Fix the hello page opening twice."),
];

fn mix(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    }) >> 8
}

fn stamp(name: &str) -> (i32, u32, u32, u32, u32) {
    let hash = mix(name);
    (
        2026,
        5 + (hash % 6) as u32,
        1 + ((hash >> 8) % 28) as u32,
        ((hash >> 16) % 24) as u32,
        ((hash >> 24) % 60) as u32,
    )
}

fn size_of(name: &str) -> u64 {
    let hash = mix(name);
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "rs" => 1_500 + hash % 38_000,
        "md" => 800 + hash % 18_000,
        "toml" | "yml" | "yaml" | "nix" | "plist" => 200 + hash % 2_400,
        "json" | "lock" => 1_000 + hash % 80_000,
        "pdf" => 60_000 + hash % 3_000_000,
        "jpg" => 900_000 + hash % 5_000_000,
        "png" => 80_000 + hash % 2_000_000,
        "svg" => 2_000 + hash % 30_000,
        "dmg" => 40_000_000 + hash % 2_000_000_000,
        "pkg" => 20_000_000 + hash % 300_000_000,
        "zip" | "gz" => 2_000_000 + hash % 300_000_000,
        "mp4" => 40_000_000 + hash % 900_000_000,
        "mp3" => 3_000_000 + hash % 40_000_000,
        "xlsx" | "docx" | "pptx" | "epub" => 20_000 + hash % 4_000_000,
        _ => 300 + hash % 9_000,
    }
}

fn entry(folder: &Path, listed: &str) -> FileEntry {
    let is_dir = listed.ends_with('/');
    let name = listed.trim_end_matches('/');
    let mut row = fixed_result_entry(
        folder.join(name),
        if is_dir { 0 } else { size_of(name) },
        stamp(name),
    );
    row.is_dir = is_dir;
    row
}

fn parent_entry(folder: &Path) -> FileEntry {
    FileEntry {
        name: OsString::from(".."),
        path: folder.join(".."),
        is_dir: true,
        is_symlink: false,
        is_parent: true,
        size: 0,
        modified: None,
        folder_size: None,
        details: Default::default(),
    }
}

/// `names` as rows: `..` first, then folders, then files, each by name.
fn listing(folder: &str, names: &[&str]) -> Vec<FileEntry> {
    let directory = Path::new(folder);
    let mut rows: Vec<FileEntry> = names
        .iter()
        .map(|listed| entry(directory, listed))
        .collect();
    rows.sort_by_key(|row| (!row.is_dir, row.name.to_ascii_lowercase()));
    std::iter::once(parent_entry(directory))
        .chain(rows)
        .collect()
}

pub fn archive_listing(archive: &Path) -> Vec<FileEntry> {
    listing(&archive.to_string_lossy(), ARCHIVE_NAMES)
}

pub fn server_listing(folder: &str) -> Vec<FileEntry> {
    listing(folder, SERVER_NAMES)
}

fn position_of(rows: &[FileEntry], name: &str) -> usize {
    rows.iter()
        .position(|row| row.name == name)
        .unwrap_or_default()
}

/// The two dense panels: the project (cursor on `README.md`, `Cargo.toml` and
/// `CHANGELOG.md` tagged) and Downloads (cursor on `project-backup.zip`).
pub fn dense_showcase_app() -> App {
    let mut app = App::with_fixed_panels().with_version_for_test(env!("CARGO_PKG_VERSION"));
    app = app
        .with_favourites_for_test(FAVOURITES)
        .with_window_size_for_test(FRAME_SIZE);

    let project = app.panel_mut_for_test(PanelSide::Left);
    project.path = PathBuf::from(PROJECT_DIRECTORY);
    project.entries = listing(PROJECT_DIRECTORY, PROJECT_NAMES);
    project.selected = position_of(&project.entries, "Cargo.lock");
    for name in ["Cargo.toml", "CHANGELOG.md"] {
        project
            .selection
            .toggle(&PathBuf::from(PROJECT_DIRECTORY).join(name));
    }

    let downloads = app.panel_mut_for_test(PanelSide::Right);
    downloads.path = PathBuf::from(DOWNLOADS_DIRECTORY);
    downloads.entries = listing(DOWNLOADS_DIRECTORY, DOWNLOADS_NAMES);
    downloads.selected = position_of(&downloads.entries, "backup-project.zip");
    app
}

/// The place of `name` in the rows of the Downloads panel.
pub fn downloads_position(name: &str) -> usize {
    position_of(&listing(DOWNLOADS_DIRECTORY, DOWNLOADS_NAMES), name)
}

const BACKUP_ONLY: &[&str] = &[
    "old-release.zip",
    "todo-2025.md",
    "notes-backup.txt",
    "logo.png",
];

/// The project against a backup of it: every kind of mark, over more rows than fit.
pub fn dense_compare_app() -> App {
    let mut app = dense_showcase_app();
    let mut left_marks = ncrs_core::CompareMarks::new();
    let mut right_marks = ncrs_core::CompareMarks::new();
    let mut backup_names: Vec<&str> = Vec::new();
    for (index, listed) in PROJECT_NAMES.iter().enumerate() {
        let name = listed.trim_end_matches('/');
        let mark = match index % 9 {
            0 | 1 | 3 | 6 => CompareMark::Same,
            2 | 8 => CompareMark::NewerHere,
            4 => CompareMark::OlderHere,
            5 => CompareMark::OnlyHere,
            _ => CompareMark::Different,
        };
        left_marks.insert(OsString::from(name), mark);
        let opposite = match mark {
            CompareMark::NewerHere => Some(CompareMark::OlderHere),
            CompareMark::OlderHere => Some(CompareMark::NewerHere),
            CompareMark::OnlyHere => None,
            same_or_different => Some(same_or_different),
        };
        if let Some(counterpart) = opposite {
            right_marks.insert(OsString::from(name), counterpart);
            backup_names.push(listed);
        }
    }
    for name in BACKUP_ONLY {
        right_marks.insert(OsString::from(*name), CompareMark::OnlyHere);
        backup_names.push(name);
    }
    let backup = listing(BACKUP_DIRECTORY, &backup_names);
    let without_parent = |rows: Vec<FileEntry>| -> Vec<FileEntry> {
        rows.into_iter().filter(|row| !row.is_parent).collect()
    };
    let compared = ncrs_compare::Compared {
        left: left_marks,
        right: right_marks,
        left_rows: without_parent(listing(PROJECT_DIRECTORY, PROJECT_NAMES)),
        right_rows: without_parent(backup.clone()),
    };
    let right = app.panel_mut_for_test(PanelSide::Right);
    right.path = PathBuf::from(BACKUP_DIRECTORY);
    right.entries = backup;
    right.selected = 0;
    app.with_comparison_for_test(compared)
}

pub fn dense_search_results() -> Vec<(FileEntry, Vec<LineMatch>)> {
    let root = PathBuf::from(PROJECT_DIRECTORY);
    let mut results: Vec<(FileEntry, Vec<LineMatch>)> = Vec::new();
    for &(relative, number, text) in SEARCH_HITS {
        let from = text.find("hello").unwrap_or_default();
        let hit = LineMatch {
            number,
            text: text.to_owned(),
            ranges: std::iter::once(from..from + "hello".len()).collect(),
        };
        match results.last_mut() {
            Some((found, hits)) if found.path.ends_with(relative) => hits.push(hit),
            _ => results.push((
                fixed_result_entry(root.join(relative), size_of(relative), stamp(relative)),
                vec![hit],
            )),
        }
    }
    results
}
