//! The search as the app holds it: the view, whether it is up, and the run
//! that fills it, with the tasks that start and stop that run.

use std::time::Duration;

use iced::widget::operation;
use iced::{Size, Task};
use ncrs_core::PanelSide;
use ncrs_ui_kit::focus::release_focus;
use ncrs_ui_kit::layout::{self, HIT_LIST_ROW_HEIGHT};

use crate::ctx::Ctx;
use crate::engine::{self, CompiledSearch, SearchError, SearchEvent};
use crate::message::SearchMessage;
use crate::view::{field_id, QUERY_ID};
use crate::view_state::SearchView;

/// How long the search view waits for the next keystroke before it searches.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// The most events one message carries, so a search that finds thousands of
/// files does not make thousands of updates.
const SEARCH_BATCH: usize = 256;

#[derive(Debug)]
pub struct Search {
    /// The search view. Kept when it is closed, so reopening shows the last
    /// fields.
    pub view: Option<SearchView>,
    /// Whether the search view is up, in place of the panel on `side`.
    pub open: bool,
    pub side: PanelSide,
    /// Result rows that fit into the search view.
    pub rows: usize,
    /// Counts the searches started, so a late timer or batch can tell it is
    /// of a search it abandoned.
    pub generation: u64,
    /// Stops the running search when dropped, which also drops the receiver
    /// of a search it abandoned.
    pub handle: Option<iced::task::Handle>,
}

/// The messages of one search, in batches. Ends when the engine is done or the
/// task is aborted.
///
/// The engine is started inside the stream: it spawns onto the blocking pool,
/// which needs the runtime a task runs in, and `update` does not run in one.
pub fn search_events<T: Send + 'static>(
    compiled: CompiledSearch,
    generation: u64,
    to_message: fn(u64, Vec<SearchEvent>) -> T,
) -> impl iced::futures::Stream<Item = T> + Send + 'static {
    use iced::futures::StreamExt as _;
    iced::futures::stream::once(async move { engine::spawn(compiled) })
        .map(move |receiver| {
            iced::futures::stream::unfold(receiver, move |mut open| async move {
                let mut events = Vec::new();
                let received = open.recv_many(&mut events, SEARCH_BATCH).await;
                (received > 0).then(|| (to_message(generation, events), open))
            })
        })
        .flatten()
}

fn events_message(generation: u64, events: Vec<SearchEvent>) -> SearchMessage {
    SearchMessage::Events { generation, events }
}

impl Search {
    pub fn new(window: Size) -> Self {
        Self {
            view: None,
            open: false,
            side: PanelSide::Left,
            rows: layout::search_visible_rows(window, false, HIT_LIST_ROW_HEIGHT),
            generation: 0,
            handle: None,
        }
    }

    pub fn resize(&mut self, window: Size) {
        self.rows = self.rows_for(window);
        if let Some(view) = self.view.as_mut() {
            view.ensure_visible(self.rows);
        }
    }

    /// The result rows that fit in `window` at the height the search's rows
    /// have now.
    pub fn rows_for(&self, window: Size) -> usize {
        let row_height = self
            .view
            .as_ref()
            .map_or(HIT_LIST_ROW_HEIGHT, SearchView::row_height);
        layout::search_visible_rows(window, self.filtered(), row_height)
    }

    /// Whether the search shows its filters.
    pub fn filtered(&self) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| view.filters.is_some())
    }

    /// The rows of the search after its filters changed what they offer.
    pub fn refresh_rows(&mut self, ctx: &Ctx) {
        self.resize(ctx.window_size);
    }

    pub fn focus_field<T: Send + 'static>(&self) -> Task<T> {
        let id = self
            .view
            .as_ref()
            .map_or(QUERY_ID, |view| field_id(view.focused));
        operation::focus(id).chain(operation::select_all(id))
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    /// Searches again once the typing pauses; the running search stops now.
    pub fn schedule(&mut self) -> Task<SearchMessage> {
        self.handle = None;
        let generation = self.next_generation();
        if let Some(view) = self.view.as_mut() {
            view.generation = generation;
            view.running = false;
        }
        Task::perform(
            async { tokio::time::sleep(SEARCH_DEBOUNCE).await },
            move |()| SearchMessage::Debounced(generation),
        )
    }

    /// Searches again now, for when the files changed under the results.
    pub fn restart(&mut self, ctx: &Ctx) -> Task<SearchMessage> {
        // The search walks the disk of this computer; a panel on a server has
        // nothing for it to walk.
        if !self.open || ctx.side_is_remote {
            return Task::none();
        }
        self.handle = None;
        let generation = self.next_generation();
        self.start(generation, ctx)
    }

    pub fn start(&mut self, generation: u64, ctx: &Ctx) -> Task<SearchMessage> {
        let asked = self.ask_index(ctx);
        let Some(view) = self.view.as_mut() else {
            return asked;
        };
        view.generation = generation;
        if view.is_filtered() {
            // The index answers; the walk would find other things.
            self.handle = None;
            view.running = true;
            return asked;
        }
        let walk = match engine::compile(&view.query_for_engine()) {
            // Nothing to look for yet: the empty list is the answer.
            Err(SearchError::Empty) => {
                view.clear_results();
                Task::none()
            }
            Err(error) => {
                view.clear_results();
                view.error = Some(error);
                Task::none()
            }
            Ok(compiled) => {
                view.begin(generation);
                let (task, handle) = Task::run(
                    search_events(compiled, generation, events_message),
                    std::convert::identity,
                )
                .abortable();
                self.handle = Some(handle.abort_on_drop());
                task
            }
        };
        self.refresh_rows(ctx);
        Task::batch([asked, walk])
    }

    /// Asks the index for the documents of the words below the folder of the
    /// search, on a blocking thread; the filters count and narrow its answer.
    fn ask_index(&mut self, ctx: &Ctx) -> Task<SearchMessage> {
        let Some(view) = self.view.as_mut() else {
            return Task::none();
        };
        let root = view.root.clone();
        let words = view.query.clone();
        let Some(filters) = view.filters.as_mut() else {
            return Task::none();
        };
        let (generation, spec) = filters.ask(&words);
        let Some(access) = ctx.index.clone() else {
            filters.failed(ncrs_core::INDEX_NOT_OPEN.to_owned());
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || access.run_under(&spec, &root)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                SearchMessage::IndexRan { generation, result }
            },
        )
    }

    /// Asks for the snippets of the documents shown.
    pub fn request_snippets(&self, ctx: &Ctx) -> Task<SearchMessage> {
        let Some(filters) = self
            .view
            .as_ref()
            .filter(|view| view.is_filtered())
            .and_then(|view| view.filters.as_ref())
        else {
            return Task::none();
        };
        let generation = filters.generation();
        let Some((paths, words)) = filters.snippet_request() else {
            return Task::none();
        };
        let Some(access) = ctx.index.clone() else {
            return Task::none();
        };
        Task::perform(
            tokio::task::spawn_blocking(move || access.snippets(&paths, &words)),
            move |joined| {
                let result = joined.unwrap_or_else(|join| Err(join.to_string()));
                SearchMessage::SnippetsLoaded { generation, result }
            },
        )
    }

    /// A value was chosen or cleared: the walk takes over or the index
    /// answer is narrowed anew.
    pub fn filters_changed(&mut self, ctx: &Ctx) -> Task<SearchMessage> {
        self.refresh_rows(ctx);
        if !self.view.as_ref().is_some_and(SearchView::is_filtered) {
            return self.restart(ctx);
        }
        self.handle = None;
        let generation = self.next_generation();
        let rows = self.rows;
        if let Some(view) = self.view.as_mut() {
            view.generation = generation;
            view.mirror_index(rows);
        }
        self.refresh_rows(ctx);
        self.request_snippets(ctx)
    }

    /// Stops the run and drops what it still sends.
    pub fn close(&mut self) -> Task<SearchMessage> {
        // Late timers and batches of the stopped search find another
        // generation and are dropped.
        self.open = false;
        self.handle = None;
        let generation = self.next_generation();
        if let Some(kept) = self.view.as_mut() {
            kept.running = false;
            kept.generation = generation;
            if let Some(filters) = kept.filters.as_mut() {
                filters.leave_report();
            }
        }
        release_focus()
    }
}
