//! The notes tab: tree or search hits on the left, the note on the right.

use std::path::Path;

use iced::widget::{column, container, row};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::{button, segmented, text_area, text_field, Tone};
use ncrs_ui_kit::markdown_view::markdown_view;
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::theme::{self, font_size, spacing};
use ncrs_ui_kit::tree_list::{result_row, tree_row, ResultRow, TreeRow};

use crate::ctx::ViewCtx;
use crate::message::{Message, Mode};
use crate::state::State;
use crate::store::{Kind, Note};
use crate::tree::RowKey;

/// The ids of the fields, for the focus tasks.
pub const SEARCH_ID: &str = "notes-search";
pub const EDITOR_ID: &str = "notes-editor";
const TREE_ID: &str = "notes-tree";
const NOTE_ID: &str = "notes-note";

const SIDEBAR_WIDTH: f32 = 300.0;

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let hint = ctx.lang.text(match (state.searching(), state.editing()) {
        (_, true) => Msg::NotesStatusEdit,
        (true, false) => Msg::NotesStatusSearch,
        (false, false) => Msg::NotesStatusView,
    });
    let line = state
        .problem
        .as_deref()
        .map_or(PageStatusLine::Hint(hint), PageStatusLine::Failure);
    let bar = PageStatusBar {
        mode: ctx.lang.text(Msg::ActionNotes),
        line,
        right: None,
    };
    let body = row![
        container(sidebar(state, ctx)).width(SIDEBAR_WIDTH),
        theme::vertical_rule(palette.border_inactive),
        container(note_pane(state, ctx)).width(Length::Fill),
    ]
    .height(Length::Fill);
    column![body, page_status_bar(bar, palette)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn sidebar<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let search = text_field(
        ctx.lang.text(Msg::NotesSearchPlaceholder),
        &state.query,
        SEARCH_ID,
        Some(Message::SearchChanged),
        Message::SearchSubmit,
        palette,
    );
    let buttons = row![
        button(
            ctx.lang.text(Msg::NotesNewNote),
            Some(Message::NewNote),
            Tone::Secondary,
            palette,
        ),
        button(
            ctx.lang.text(Msg::NotesNewFolder),
            Some(Message::NewFolder),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::XS);
    let list = if state.searching() {
        hits(state, ctx)
    } else {
        tree(state, ctx)
    };
    column![
        container(column![search, buttons].spacing(spacing::XS)).padding(spacing::M),
        theme::horizontal_rule(palette.border_inactive),
        page_scroll(TREE_ID, list),
    ]
    .height(Length::Fill)
    .into()
}

fn tree<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let rows = state.rows().into_iter().map(|entry| {
        let label = match &entry.key {
            RowKey::Folder(folder) => folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            RowKey::Files => ctx.lang.text(Msg::NotesFilesGroup).to_owned(),
            RowKey::Note(path) => state
                .note(path)
                .map(|note| match &note.kind {
                    Kind::File { file, .. } => {
                        shortened_left(&path_below_home(file, ctx.home), entry.depth)
                    }
                    Kind::Topic => note.title.clone(),
                })
                .unwrap_or_default(),
        };
        tree_row(
            TreeRow {
                depth: entry.depth,
                open: entry.open,
                label,
                chosen: state.cursor.as_ref() == Some(&entry.key),
                dim: entry.key == RowKey::Files,
                on_press: Message::Pressed(entry.key),
            },
            ctx.palette,
        )
    });
    column(rows).into()
}

/// The file as the tree lists it: below home with a leading slash, elsewhere
/// in full.
fn path_below_home(file: &Path, home: &Path) -> String {
    match file.strip_prefix(home) {
        Ok(relative) if !relative.as_os_str().is_empty() => {
            format!("/{}", relative.to_string_lossy())
        }
        _ => file.to_string_lossy().into_owned(),
    }
}

/// Rough width of one character of the tree's body text, as a share of its size.
const CHAR_WIDTH_SHARE: f32 = 0.6;
/// What a tree row takes besides its label: open mark, gap and padding.
const ROW_OVERHEAD: f32 = 26.0;
/// The width the tree list loses to the margin around its rows.
const LIST_INSET: f32 = 48.0;
/// The kit's indent per tree level.
const LEVEL_INDENT: f32 = 16.0;

/// Cuts a path from the left, at a folder border, so the file name stays
/// visible where the row is too narrow: `…/House/Offer.pdf`.
fn shortened_left(path: &str, depth: usize) -> String {
    let room = SIDEBAR_WIDTH - LIST_INSET - ROW_OVERHEAD - LEVEL_INDENT * depth as f32;
    let fits = (room / (font_size::BODY * CHAR_WIDTH_SHARE)) as usize;
    if path.chars().count() <= fits {
        return path.to_owned();
    }
    let mut kept = path;
    while let Some(at) = kept.find('/') {
        let rest = &kept[at + 1..];
        if rest.is_empty() {
            break;
        }
        kept = rest;
        let candidate = format!("\u{2026}/{kept}");
        if candidate.chars().count() <= fits {
            return candidate;
        }
    }
    let tail: String = kept.chars().rev().take(fits.saturating_sub(1)).collect();
    format!("\u{2026}{}", tail.chars().rev().collect::<String>())
}

fn place(note: &Note, ctx: ViewCtx<'_>) -> String {
    match &note.kind {
        Kind::Topic if note.folder.as_os_str().is_empty() => {
            ctx.lang.text(Msg::ActionNotes).to_owned()
        }
        Kind::Topic => note.folder.to_string_lossy().into_owned(),
        Kind::File { file, .. } => format!(
            "{} {}",
            ctx.lang.text(Msg::NotesFilesGroup),
            path_below_home(file, ctx.home)
        ),
    }
}

fn hits<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    if state.hits.is_empty() {
        return container(page_line(
            ctx.lang.text(Msg::NotesSearchNone),
            font_size::BODY,
            ctx.palette.dim_text,
        ))
        .padding(spacing::M)
        .into();
    }
    let rows = state
        .hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| Some((index, hit, state.note(&hit.note)?)))
        .map(|(index, hit, note)| {
            result_row(
                ResultRow {
                    title: note.title.clone(),
                    place: place(note, ctx),
                    snippet: hit.snippet.clone(),
                    chosen: index == state.hit_cursor,
                    on_press: Message::HitPressed(index),
                },
                ctx.palette,
            )
        });
    column(rows).into()
}

fn note_pane<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let Some(note) = state.shown() else {
        return container(page_line(
            ctx.lang.text(Msg::NotesNoneShown),
            font_size::BODY,
            palette.dim_text,
        ))
        .padding(spacing::XXL)
        .into();
    };
    let meta = match note.modified {
        Some(modified) => format!(
            "{} \u{b7} {}",
            place(note, ctx),
            fill_template(
                ctx.lang.text(Msg::NotesChanged),
                &[("{time}", &format::time(Some(modified)))],
            )
        ),
        None => place(note, ctx),
    };
    let titles = column![
        page_heading(&note.title, palette),
        page_line(meta, font_size::SMALL, palette.dim_text),
    ]
    .spacing(spacing::XS);
    let mut head = row![titles, container("").width(Length::Fill)]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if matches!(note.kind, Kind::File { .. }) {
        head = head.push(button(
            ctx.lang.text(Msg::NotesShowFile),
            Some(Message::ShowFile),
            Tone::Secondary,
            palette,
        ));
    }
    head = head.push(segmented(
        &[
            (
                ctx.lang.text(Msg::NotesModeView),
                Message::SetMode(Mode::View),
            ),
            (
                ctx.lang.text(Msg::NotesModeEdit),
                Message::SetMode(Mode::Edit),
            ),
        ],
        usize::from(state.mode == Mode::Edit),
        true,
        palette,
    ));
    let content: Element<'a, Message> = match state.mode {
        Mode::View => page_scroll(
            NOTE_ID,
            container(markdown_view(&state.items, palette, Message::LinkClicked))
                .padding([0.0, spacing::XXL])
                .into(),
        ),
        Mode::Edit => container(text_area(
            &state.editor,
            EDITOR_ID,
            Message::Edited,
            palette,
        ))
        .padding([0.0, spacing::XXL])
        .height(Length::Fill)
        .into(),
    };
    column![
        container(head).padding([spacing::L, spacing::XXL]),
        theme::horizontal_rule(palette.border_inactive),
        content,
    ]
    .height(Length::Fill)
    .into()
}
