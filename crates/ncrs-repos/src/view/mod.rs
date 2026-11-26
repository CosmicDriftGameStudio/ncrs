//! The Repos tab: head, bulk bar, result banner, grouped list, log and status bar.

mod cleanup_pane;
mod clone;
mod credential;
mod head;
mod list;
mod log;

use iced::widget::{column, row, stack};
use iced::{Element, Length, Point};
use ncrs_config::keymap::KeyAction;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::git_rows::drag_capture;
use ncrs_ui_kit::kit::scrim;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::popup_menu::{
    menu_rows_height, popup_layer, popup_menu_rows, MenuRow, PopupEntry, RightPress,
    POPUP_MENU_WIDTH,
};
use ncrs_ui_kit::theme::{self, size};

use crate::cleanup::Phase;
use crate::ctx::ViewCtx;
use crate::menu::{menu_items, MenuKey};
use crate::message::Message;
use crate::state::State;

/// Where the menu opens horizontally; its height follows the row of the cursor.
const MENU_X: f32 = 280.0;
/// The title, the bar of the actions, the rule and the column head above the list.
const LIST_TOP: f32 = 32.0 + 1.0 + 32.0 + 22.0;
/// The status bar and the note under the list.
const BELOW_LIST: f32 = 22.0 + 30.0;

/// The title of the tab.
pub fn tab_title(ctx: ViewCtx<'_>) -> String {
    ctx.lang.text(Msg::ReposTitle).to_owned()
}

fn menu_key_label(key: MenuKey, ctx: ViewCtx<'_>) -> Option<String> {
    let app_key = |action| ctx.keymap.shortcut_label_of(action);
    match key {
        MenuKey::Own(shortcut) => Some(ctx.keys.label(shortcut)),
        MenuKey::AppOpen => app_key(KeyAction::Open),
        MenuKey::AppTerminal => app_key(KeyAction::OpenTerminal),
        MenuKey::AppCopyPath => app_key(KeyAction::CopyPath),
    }
    .filter(|label| !label.is_empty())
}

/// The menu of the cursor row as the menu of the file panels draws it: groups
/// with a line between them, what destroys something last.
fn menu_rows(state: &State, ctx: ViewCtx<'_>) -> Vec<MenuRow<Message>> {
    let mut rows = Vec::new();
    for (index, item) in menu_items(state, ctx.actions_available)
        .into_iter()
        .enumerate()
    {
        if item.starts_group {
            rows.push(MenuRow::Separator);
        }
        let entry = PopupEntry {
            label: ctx.lang.text(item.label).to_owned(),
            key: menu_key_label(item.key, ctx),
            is_selected: index == state.menu_cursor,
            on_press: item.enabled.then_some(Message::MenuPicked(item.entry)),
            on_hover: item.enabled.then_some(Message::MenuCursor(index)),
        };
        rows.push(if item.is_danger {
            MenuRow::Danger(entry)
        } else {
            MenuRow::Entry(entry)
        });
    }
    rows
}

fn status_bar<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let progress;
    let line = if let Some(failure) = state.failure() {
        PageStatusLine::Failure(failure)
    } else if state.cleanup.is_some() {
        PageStatusLine::Hint(ctx.lang.text(Msg::ReposCleanupHint))
    } else if let Some(run) = state.run().filter(|run| !run.is_finished()) {
        progress = fill_template(
            ctx.lang.text(Msg::ReposRunProgress),
            &[
                ("{done}", &run.done().to_string()),
                ("{total}", &run.targets().len().to_string()),
                ("{running}", &run.running().to_string()),
            ],
        );
        PageStatusLine::Hint(&progress)
    } else if state.run().is_some() {
        PageStatusLine::Hint(ctx.lang.text(Msg::ReposRunHint))
    } else {
        PageStatusLine::Hint(ctx.lang.text(Msg::ReposHint))
    };
    let right = state
        .cleanup
        .as_ref()
        .filter(|cleanup| cleanup.phase == Phase::Ready)
        .map(|cleanup| {
            let (branches, worktrees, _) = cleanup.offered();
            fill_template(
                ctx.lang.text(Msg::ReposCleanupChosen),
                &[
                    ("{chosen}", &cleanup.ticked().0.to_string()),
                    ("{count}", &(branches + worktrees).to_string()),
                ],
            )
        });
    page_status_bar(
        PageStatusBar {
            mode: ctx.lang.text(Msg::ReposMode),
            line,
            right: right.as_deref(),
        },
        ctx.palette,
    )
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let mut page = column![
        head::title(state, ctx),
        theme::horizontal_rule(palette.border_inactive),
        head::actions(state, ctx),
    ];
    if let Some(banner) = head::banner(state, ctx) {
        page = page.push(banner);
    }
    page = match &state.cleanup {
        Some(cleanup) => page.push(
            row![list::view(state, ctx), cleanup_pane::view(cleanup, ctx)].height(Length::Fill),
        ),
        None => page.push(list::view(state, ctx)),
    };
    if let Some(run) = state.run() {
        page = page.push(log::view(run, ctx));
    }
    let filled_page = page.width(Length::Fill).height(Length::Fill);
    let full_page = column![filled_page, status_bar(state, ctx)].height(Length::Fill);
    // The page stays the first layer whatever lies over it, so the list keeps
    // its scroll when a menu or a dialog opens.
    let mut layers = stack![full_page];
    if state.column_drag.is_some() {
        layers = layers.push(drag_capture(Message::ResizeMoved, Message::ResizeReleased));
    }
    if let Some(ask) = &state.credential {
        layers = layers.push(scrim(credential::dialog(ask, ctx)));
    } else if let Some(draft) = state.clone_draft() {
        layers = layers.push(scrim(clone::dialog(draft, ctx)));
    } else if state.menu_open() {
        let rows = menu_rows(state, ctx);
        let origin = menu_origin(state, ctx, menu_rows_height(&rows));
        let menu = popup_menu_rows(rows, POPUP_MENU_WIDTH, origin, palette);
        layers = layers.push(popup_layer(
            menu,
            Some(Message::CloseMenu),
            RightPress::Dismiss,
        ));
    }
    layers.into()
}
/// The menu opens under the visible row of the cursor, and above it where it
/// would run past the bottom of the window. The row is placed from the scroll
/// of the list; a scroll the keyboard made is not reported, so the row is
/// taken to be kept in view the way `row_into_view` does it.
fn menu_origin(state: &State, ctx: ViewCtx<'_>, height: f32) -> Point {
    let cursor = state.cursor();
    let mut content_top = 0.0;
    'found: for group in state.groups() {
        content_top += size::SECTION_HEAD;
        for row in &group.rows {
            if Some(&row.path) == cursor.as_ref() {
                break 'found;
            }
            content_top += size::REPO_ROW;
        }
    }
    let viewport = (ctx.window.height - LIST_TOP - BELOW_LIST).max(size::REPO_ROW);
    let in_view = (content_top - state.list_scroll).clamp(0.0, viewport - size::REPO_ROW);
    let row_top = LIST_TOP + in_view;
    let below = row_top + size::REPO_ROW;
    let y = if below + height > ctx.window.height {
        (row_top - height).max(0.0)
    } else {
        below
    };
    Point::new(MENU_X, y)
}
