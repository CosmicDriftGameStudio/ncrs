//! The notes tab: tree or search hits on the left, the note on the right.

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

/// The key of the notes in the status bar's mode field.
const MODE_FIELD: &str = "Notes";

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
        mode: MODE_FIELD,
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
                .map(|note| note.title.clone())
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

fn place(note: &Note, ctx: ViewCtx<'_>) -> String {
    match &note.kind {
        Kind::Topic if note.folder.as_os_str().is_empty() => {
            ctx.lang.text(Msg::ActionNotes).to_owned()
        }
        Kind::Topic => note.folder.to_string_lossy().into_owned(),
        Kind::File { file, .. } => format!(
            "{} {}",
            ctx.lang.text(Msg::NotesFilesGroup),
            format::shown_path(file, ctx.home)
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
    let changed = fill_template(
        ctx.lang.text(Msg::NotesChanged),
        &[("{time}", &format::time(note.modified))],
    );
    let meta = format!("{} \u{b7} {changed}", place(note, ctx));
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
