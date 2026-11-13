//! The state of the viewer tab.

use crate::load::Doc;
use crate::message::Mode;

/// The one file the tab shows; `None` while no tab is up.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub doc: Option<Doc>,
    pub mode: Mode,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.doc.is_some()
    }

    /// The tab title: the file name.
    pub fn title(&self) -> Option<&str> {
        self.doc.as_ref().map(|doc| doc.name.as_str())
    }

    pub fn close(&mut self) {
        self.doc = None;
    }

    pub fn shows_markdown(&self) -> bool {
        self.doc.as_ref().is_some_and(|doc| doc.markdown)
    }
}
