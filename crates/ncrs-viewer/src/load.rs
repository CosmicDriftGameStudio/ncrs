//! Reading the file on a blocking thread.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ncrs_ui_kit::markdown_view::{parse_markdown, MarkdownItem};

/// Files above this are not shown in the tab.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;

const MARKDOWN_EXTENSIONS: [&str; 2] = ["md", "markdown"];

/// A file read and, when it is Markdown, parsed.
#[derive(Debug, Clone)]
pub struct Doc {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub text: String,
    pub items: Vec<MarkdownItem>,
    pub markdown: bool,
}

#[derive(Debug, Clone)]
pub enum Loaded {
    Doc(Doc),
    TooLarge { path: PathBuf, size: u64 },
    Failed { path: PathBuf, reason: String },
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .is_some_and(|extension| MARKDOWN_EXTENSIONS.contains(&extension.as_str()))
}

/// Reads `path`; a text that is not UTF-8 is shown with replacement characters.
pub fn load(path: PathBuf) -> Loaded {
    let failed = |reason: std::io::Error| Loaded::Failed {
        path: path.clone(),
        reason: reason.to_string(),
    };
    let meta = match fs::metadata(&path) {
        Ok(meta) => meta,
        Err(err) => return failed(err),
    };
    if meta.len() > MAX_BYTES {
        return Loaded::TooLarge {
            path,
            size: meta.len(),
        };
    }
    let mut bytes = Vec::new();
    // The file may have grown since the metadata: the cap holds for the read too.
    let read =
        fs::File::open(&path).and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes));
    if let Err(err) = read {
        return failed(err);
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Loaded::TooLarge {
            path,
            size: bytes.len() as u64,
        };
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let markdown = is_markdown(&path);
    let items = if markdown {
        parse_markdown(&text)
    } else {
        Vec::new()
    };
    let name = path
        .file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned();
    Loaded::Doc(Doc {
        name,
        size: bytes.len() as u64,
        modified: meta.modified().ok(),
        text,
        items,
        markdown,
        path,
    })
}

impl Loaded {
    fn path(&self) -> &Path {
        match self {
            Loaded::Doc(doc) => &doc.path,
            Loaded::TooLarge { path, .. } | Loaded::Failed { path, .. } => path,
        }
    }
}

// The app's `Message` is `PartialEq`; parsed Markdown is not, so a load counts
// as the same when it is for the same file.
impl PartialEq for Loaded {
    fn eq(&self, other: &Self) -> bool {
        self.path() == other.path()
    }
}
