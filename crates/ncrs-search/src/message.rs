use std::path::PathBuf;

use ncrs_core::PanelSide;
use ncrs_index::QueryHit;

use crate::engine::SearchEvent;
use crate::filters::{Facet, FacetValue};

/// Everything the search view sends. Its own enum so the panel messages stay
/// the panels': the same key means "move the cursor" in both.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchMessage {
    QueryChanged(String),
    IncludeChanged(String),
    ExcludeChanged(String),
    /// A printable key typed while the result list has the keys: it goes to the
    /// end of the query, as in VS Code.
    AppendToQuery(String),
    ToggleCase,
    ToggleWholeWord,
    ToggleRegex,
    /// Tag the cursor's file and move to the next file row.
    ToggleTag,
    /// Tag the cursor's file and move to the next (`1`) or previous (`-1`) file row.
    TagMove(isize),
    TagAll,
    ClearTags,
    /// The typing pause of this generation is over. An older timer finds a
    /// newer generation and does nothing.
    Debounced(u64),
    /// A batch of what the running search found. Dropped when `generation` is
    /// not the current search's.
    Events {
        generation: u64,
        events: Vec<SearchEvent>,
    },
    Close,
    Move(isize),
    Page(isize),
    First,
    Last,
    /// Enter: open the file in the other panel.
    Activate,
    RowClicked(usize),
    /// Double click on a row.
    RowActivated(usize),
    /// The star of the file at this index in the results.
    ToggleTagAt(usize),
    /// A right click on the file at this index: the context menu at the pointer.
    ContextAt(usize),
    Scrolled(iced::mouse::ScrollDelta),
    /// The index answered the question `generation` of the filters. Dropped
    /// when the words changed meanwhile.
    IndexRan {
        generation: u64,
        result: Result<Vec<QueryHit>, String>,
    },
    /// The pieces of text around the words, by path. Dropped like `IndexRan`.
    SnippetsLoaded {
        generation: u64,
        result: Result<Vec<(PathBuf, String)>, String>,
    },
    /// Chooses the value of a filter, or drops it when it is chosen.
    ToggleFacet(FacetValue),
    /// The cross of a filter: drops what is chosen of that facet.
    ClearFacet(Facet),
    /// Opens the Reports page with a new report asking what the search asks.
    SaveAsReport,
    /// Writes what the search asks over the report it was opened for.
    UpdateReport,
    /// The report was changed (or not): back to the Reports page.
    ReportUpdated(Result<(), String>),
    /// The hits of the search as a list in the panel on this side.
    OpenHitList(PanelSide),
}

impl SearchMessage {
    /// One of the three switches of the query: case, whole word, regex.
    pub fn is_toggle(&self) -> bool {
        matches!(
            self,
            Self::ToggleCase | Self::ToggleWholeWord | Self::ToggleRegex
        )
    }
}
