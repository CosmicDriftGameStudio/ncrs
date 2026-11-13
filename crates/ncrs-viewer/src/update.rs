//! What a message does to the viewer tab.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::page_scroll::{scroll_page_by, scroll_page_to_edge};

use crate::action::Action;
use crate::ctx::Ctx;
use crate::link::{classify, Link};
use crate::load::{load, Loaded, MAX_BYTES};
use crate::message::{Message, Mode, Scroll};
use crate::state::State;
use crate::view::PAGE_ID;

/// Pixels one arrow press scrolls.
const LINE_PX: f32 = 48.0;
/// A page key scrolls this much of the window height.
const PAGE_SHARE: f32 = 0.9;

/// Reads `path` on a blocking thread; the answer is [`Message::Loaded`].
pub fn open(path: PathBuf) -> Task<Message> {
    let fallback = path.clone();
    Task::perform(
        tokio::task::spawn_blocking(move || load(path)),
        move |joined| {
            Message::Loaded(Box::new(joined.unwrap_or_else(|err| Loaded::Failed {
                path: fallback.clone(),
                reason: err.to_string(),
            })))
        },
    )
}

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let nothing = (Task::none(), None);
    match message {
        Message::Loaded(loaded) => loaded_file(state, *loaded, ctx),
        Message::SetMode(mode) => {
            set_mode(state, mode);
            nothing
        }
        Message::ToggleMode => {
            let other = match state.mode {
                Mode::Rendered => Mode::Source,
                Mode::Source => Mode::Rendered,
            };
            set_mode(state, other);
            nothing
        }
        Message::LinkClicked(target) => link_clicked(state, &target),
        Message::OpenExternal => (Task::none(), path_of(state).map(Action::OpenExternal)),
        Message::Edit => (Task::none(), path_of(state).map(Action::Edit)),
        Message::Close => {
            state.close();
            (Task::none(), Some(Action::Close))
        }
        Message::Scroll(scroll) => (scrolled(scroll, ctx), None),
    }
}

fn path_of(state: &State) -> Option<PathBuf> {
    state.doc.as_ref().map(|doc| doc.path.clone())
}

fn set_mode(state: &mut State, mode: Mode) {
    if state.shows_markdown() {
        state.mode = mode;
    }
}

fn link_clicked(state: &State, target: &str) -> (Task<Message>, Option<Action>) {
    let Some(doc) = state.doc.as_ref() else {
        return (Task::none(), None);
    };
    let base = doc.path.parent().unwrap_or(Path::new(""));
    match classify(target, base) {
        Link::Ignore => (Task::none(), None),
        Link::Url(url) => (Task::none(), Some(Action::OpenUrl(url))),
        Link::Markdown(next) => (open(next), None),
        Link::File(file) => (Task::none(), Some(Action::Reveal(file))),
    }
}

fn loaded_file(state: &mut State, loaded: Loaded, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    match loaded {
        Loaded::Doc(doc) => {
            state.mode = if doc.markdown {
                Mode::Rendered
            } else {
                Mode::Source
            };
            state.doc = Some(doc);
            (scroll_page_to_edge(PAGE_ID, false), None)
        }
        Loaded::TooLarge { path, .. } => {
            let notice = fill_template(
                ctx.lang.text(Msg::ViewerTooLarge),
                &[
                    ("{name}", &name_of(&path)),
                    ("{limit}", &format::size(MAX_BYTES)),
                ],
            );
            (
                Task::none(),
                Some(Action::OpenSystemInstead { path, notice }),
            )
        }
        Loaded::Failed { path, reason } => {
            let text = fill_template(
                ctx.lang.text(Msg::ViewerReadFailed),
                &[("{name}", &name_of(&path)), ("{reason}", &reason)],
            );
            (Task::none(), Some(Action::Error(text)))
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned()
}

fn scrolled(scroll: Scroll, ctx: &Ctx) -> Task<Message> {
    match scroll {
        Scroll::Lines(count) => scroll_page_by(PAGE_ID, f32::from(count) * LINE_PX),
        Scroll::Pages(count) => scroll_page_by(
            PAGE_ID,
            f32::from(count) * ctx.window_size.height * PAGE_SHARE,
        ),
        Scroll::Top => scroll_page_to_edge(PAGE_ID, false),
        Scroll::End => scroll_page_to_edge(PAGE_ID, true),
    }
}
