//! The notes tab: tree or search hits on the left, the note on the right.

use std::path::Path;

use iced::widget::{column, container, mouse_area, responsive, row, stack};
use iced::{alignment, Element, Length, Point};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::{button, segmented, text_area, text_field, Tone};
use ncrs_ui_kit::markdown_view::markdown_view;
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::popup_menu::{popup_layer, popup_menu, PopupEntry, RightPress};
use ncrs_ui_kit::theme::{self, font_size, size, spacing};
use ncrs_ui_kit::tree_list::{result_row, tree_row_with_lock, ResultRow, TreeRow, TREE_INDENT};
use ncrs_user_verify::Check;

use crate::crypt::is_encrypted_path;
use crate::ctx::ViewCtx;
use crate::message::{Message, Mode};
use crate::secure::denied_text;
use crate::state::{Seal, State};
use crate::store::{Kind, Note};
use crate::tree::RowKey;

/// Where the first row of the sidebar tree starts below the notes tab: the
/// search field, the buttons, the rule and the padding of the list. Measured
/// from the drawn page, as the layout is made of widgets, not of numbers.
pub const SIDEBAR_ROWS_TOP: f32 = 84.0;

/// The ids of the fields, for the focus tasks.
pub const SEARCH_ID: &str = "notes-search";
pub const EDITOR_ID: &str = "notes-editor";
const TREE_ID: &str = "notes-tree";
const NOTE_ID: &str = "notes-note";

const SIDEBAR_WIDTH: f32 = size::SIDE_PANE;
/// From this width of the note pane the editor and the rendered note sit side by side.
const SPLIT_MIN_WIDTH: f32 = 1100.0;

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>, new_note_key: &str) -> Element<'a, Message> {
    let palette = ctx.palette;
    let hint = match (state.searching(), state.editing()) {
        (_, true) => ctx.lang.text(Msg::NotesStatusEdit).to_owned(),
        (true, false) => ctx.lang.text(Msg::NotesStatusSearch).to_owned(),
        (false, false) => fill_template(
            ctx.lang.text(Msg::NotesStatusView),
            &[("{key}", new_note_key)],
        ),
    };
    let line = state
        .problem
        .as_deref()
        .map_or(PageStatusLine::Hint(&hint), PageStatusLine::Failure);
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
    let page: Element<'a, Message> = column![body, page_status_bar(bar, palette)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    match context_menu(state, ctx) {
        Some(menu) => stack![page, menu].into(),
        None => page,
    }
}

/// The menu of the row that was pressed with the right button: encrypt or
/// decrypt it, and lock everything now.
fn context_menu<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let key = state.menu.clone()?;
    state
        .rows()
        .iter()
        .any(|entry| entry.key == key)
        .then_some(())?;
    let sealed = match &key {
        RowKey::Folder(folder) => state.folder_is_sealed(folder),
        RowKey::Note(path) => is_encrypted_path(path),
        RowKey::Files => return None,
    };
    let entry = |label: Msg, on_press: Message| PopupEntry {
        label: ctx.lang.text(label).to_owned(),
        key: None,
        is_selected: false,
        on_press: Some(on_press),
        on_hover: None,
    };
    let mut entries = vec![if sealed {
        entry(Msg::NotesMenuDecrypt, Message::Seal(Seal::Row(key), false))
    } else {
        entry(Msg::NotesMenuEncrypt, Message::Seal(Seal::Row(key), true))
    }];
    if state.vault.is_unlocked() {
        entries.push(entry(Msg::NotesMenuLockNow, Message::Lock));
    }
    // The pointer is a position in the window; the tab starts below the title bar.
    let origin = Point::new(state.menu_pointer.x, state.menu_pointer.y - size::TITLE_BAR);
    Some(popup_layer(
        popup_menu(entries, origin, ctx.palette),
        Some(Message::CloseMenu),
        RightPress::Dismiss,
    ))
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
    let empty_hint = match (state.loaded, state.rows().is_empty()) {
        (false, _) => Some(Msg::NotesLoading),
        (true, true) => Some(Msg::NotesEmpty),
        (true, false) => None,
    };
    if let Some(hint) = empty_hint {
        return container(page_line(
            ctx.lang.text(hint),
            font_size::BODY,
            ctx.palette.dim_text,
        ))
        .padding(spacing::M)
        .into();
    }
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
                        path_fitted_to_row(&path_below_home(file, ctx.home), entry.depth)
                    }
                    Kind::Topic => note.title.clone(),
                })
                .unwrap_or_default(),
        };
        let locked = match &entry.key {
            RowKey::Folder(folder) => state.folder_is_sealed(folder),
            RowKey::Note(path) => is_encrypted_path(path),
            RowKey::Files => false,
        };
        let key = entry.key.clone();
        let row: Element<'a, Message> = tree_row_with_lock(
            TreeRow {
                depth: entry.depth,
                open: entry.open,
                label,
                chosen: state.cursor.as_ref() == Some(&entry.key),
                dim: entry.key == RowKey::Files,
                on_press: Message::Pressed(entry.key),
            },
            locked,
            locked.then_some(if state.vault.is_unlocked() {
                Message::Lock
            } else {
                Message::Unlock
            }),
            ctx.palette,
        );
        Element::from(mouse_area(row).on_right_press(Message::OpenMenu(key)))
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

/// What a tree row takes besides its label: open mark, gap and padding.
const ROW_OVERHEAD: f32 = 26.0;
/// The width the tree list loses to the margin around its rows.
const LIST_INSET: f32 = 48.0;

/// The path cut in the middle to fit the row, so the file name stays visible
/// where the row is too narrow.
fn path_fitted_to_row(path: &str, depth: usize) -> String {
    let room = SIDEBAR_WIDTH - LIST_INSET - ROW_OVERHEAD - TREE_INDENT * depth as f32;
    format::fit_middle(path, room, font_size::BODY)
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
    if state.is_locked(note) {
        return locked_page(state, note, ctx);
    }
    let meta = match note.modified {
        Some(modified) => format!(
            "{} \u{b7} {}",
            place(note, ctx),
            fill_template(
                ctx.lang.text(Msg::NotesChanged),
                &[(
                    "{time}",
                    &format::time_in(Some(modified), ctx.date_format, ctx.lang),
                )],
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
        Mode::View => rendered(state, palette),
        Mode::Edit => responsive(move |room| {
            let editor = container(text_area(
                &state.editor,
                EDITOR_ID,
                Message::Edited,
                palette,
            ))
            .padding([0.0, spacing::XXL])
            .height(Length::Fill);
            if room.width >= SPLIT_MIN_WIDTH {
                row![
                    editor.width(Length::FillPortion(1)),
                    theme::vertical_rule(palette.border_inactive),
                    container(rendered(state, palette)).width(Length::FillPortion(1)),
                ]
                .into()
            } else {
                editor.into()
            }
        })
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

/// What stands where an encrypted note would be shown while the key is not in memory.
fn locked_page<'a>(state: &State, note: &'a Note, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let unlock_label = match state.vault.verifier().check() {
        Check::TouchId => Msg::NotesUnlockTouchId,
        Check::WindowsHello => Msg::NotesUnlockWindowsHello,
        Check::Nothing => Msg::NotesUnlock,
    };
    let mut page = column![
        page_heading(&note.title, palette),
        page_line(
            ctx.lang.text(Msg::NotesLockedBody),
            font_size::BODY,
            palette.dim_text
        ),
        button(
            ctx.lang.text(unlock_label),
            Some(Message::Unlock),
            Tone::Primary,
            palette,
        ),
    ]
    .spacing(spacing::L);
    if let Some(denied) = &state.unlock_denied {
        page = page.push(page_line(
            denied_text(denied, ctx.lang),
            font_size::BODY,
            palette.error,
        ));
    }
    container(page).padding(spacing::XXL).into()
}

fn rendered<'a>(state: &'a State, palette: &'a Palette) -> Element<'a, Message> {
    page_scroll(
        NOTE_ID,
        container(markdown_view(&state.items, palette, Message::LinkClicked))
            .padding([0.0, spacing::XXL])
            .into(),
    )
}
