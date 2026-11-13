//! What happens in the viewer tab.

use crate::load::Loaded;

/// How a file is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Markdown formatted.
    #[default]
    Rendered,
    /// The text as on disk.
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    Lines(i8),
    Pages(i8),
    Top,
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The blocking read of a file is done.
    Loaded(Box<Loaded>),
    SetMode(Mode),
    ToggleMode,
    /// A link of the rendered text, as written there.
    LinkClicked(String),
    OpenExternal,
    Edit,
    Close,
    Scroll(Scroll),
}
