//! The Platz tab: head with the drive, the actions, the groups with a detail pane, and the
//! dialog, the result or the decide pane over the same page.

mod confirm;
mod decide;
mod detail;
mod head;
mod list;
mod log;
mod result;

use iced::widget::{column, stack};
use iced::{Element, Length, Point};
use ncrs_i18n::{fill_template, Msg};
use ncrs_space::Group;
use ncrs_ui_kit::kit::scrim;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::popup_menu::{
    menu_rows_height, popup_layer, popup_menu_rows, MenuRow, PopupEntry, RightPress,
    POPUP_MENU_WIDTH,
};
use ncrs_ui_kit::theme::{self, size};

use crate::ctx::ViewCtx;
use crate::menu::menu_items;
use crate::message::Message;
use crate::rows::Item;
use crate::shortcuts::Shortcut;
use crate::state::State;
use crate::texts::fill_size;

/// Where the menu opens horizontally; its height follows the row of the cursor.
const MENU_X: f32 = 280.0;
/// The head, the drive, the actions and the rule above the list.
const LIST_TOP: f32 = 32.0 + 1.0 + 70.0 + 40.0;
/// The status bar.
const BELOW_LIST: f32 = 22.0;

/// The title of the tab.
pub fn tab_title(ctx: ViewCtx<'_>) -> String {
    ctx.lang.text(Msg::SpaceTabTitle).to_owned()
}

fn menu_rows(state: &State, ctx: ViewCtx<'_>) -> Vec<MenuRow<Message>> {
    let mut rows = Vec::new();
    for (index, item) in menu_items(state).into_iter().enumerate() {
        if item.starts_group && index > 0 {
            rows.push(MenuRow::Separator);
        }
        let label = ctx.keys.label(item.key);
        let entry = PopupEntry {
            label: ctx.lang.text(item.label).to_owned(),
            key: Some(label).filter(|label| !label.is_empty()),
            is_selected: index == state.menu_cursor(),
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

/// The menu opens under the row of the cursor, and above it where it would run past the
/// bottom of the window. The row is placed from the scroll of the list.
fn menu_origin(state: &State, ctx: ViewCtx<'_>, height: f32) -> Point {
    let mut content_top = 0.0;
    for item in state.items() {
        let at_cursor = match item {
            Item::Head(_) => false,
            Item::Row(place) => {
                state.rows().get(place).map(|row| row.path.as_path()) == state.menu_path()
            }
        };
        if at_cursor {
            break;
        }
        content_top += match item {
            Item::Head(_) => size::SECTION_HEAD,
            Item::Row(_) => size::SIZE_ROW,
        };
    }
    let viewport = (ctx.window.height - LIST_TOP - BELOW_LIST).max(size::SIZE_ROW);
    let in_view = (content_top - state.list_scroll).clamp(0.0, viewport - size::SIZE_ROW);
    let row_top = LIST_TOP + in_view;
    let below = row_top + size::SIZE_ROW;
    let y = if below + height > ctx.window.height {
        (row_top - height).max(0.0)
    } else {
        below
    };
    Point::new(MENU_X, y)
}

fn key_hints(state: &State, ctx: ViewCtx<'_>) -> Vec<(String, Msg)> {
    let own = |shortcut: Shortcut| ctx.keys.label(shortcut);
    if state.decide().is_some() {
        return vec![
            ("Space".to_owned(), Msg::SpaceKeyChoose),
            (own(Shortcut::Trash), Msg::SpaceKeyTrash),
            ("Esc".to_owned(), Msg::SpaceKeyBack),
        ];
    }
    if state.result().is_some() {
        return vec![
            (own(Shortcut::Retry), Msg::SpaceKeyRetry),
            ("Esc".to_owned(), Msg::SpaceKeyCloseResult),
        ];
    }
    let mut hints = vec![
        (own(Shortcut::DeleteSafe), Msg::SpaceKeyDeleteSafe),
        ("Space".to_owned(), Msg::SpaceKeyToggle),
        ("\u{21b5}".to_owned(), Msg::SpaceKeyDetails),
        ("\u{2190}/\u{2192}".to_owned(), Msg::SpaceKeyFold),
        (own(Shortcut::Filter), Msg::SpaceKeySearch),
        (own(Shortcut::AskHermes), Msg::SpaceKeyHermes),
    ];
    if !ctx.store_build {
        hints.insert(
            1,
            (own(Shortcut::RemoveWorktrees), Msg::SpaceKeyRemoveWorktrees),
        );
    }
    hints
}

fn status_bar<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let hints = key_hints(state, ctx);
    let labelled: Vec<(&str, &str)> = hints
        .iter()
        .map(|(key, msg)| (key.as_str(), ctx.lang.text(*msg)))
        .collect();
    let line = match state.failure() {
        Some(failure) => PageStatusLine::Failure(failure),
        None => PageStatusLine::Keys(&labelled),
    };
    let chosen = chosen_text(state, ctx);
    page_status_bar(
        PageStatusBar {
            mode: ctx.lang.text(Msg::SpaceTabTitle),
            line,
            right: chosen.as_deref(),
        },
        ctx.palette,
    )
}

fn chosen_text(state: &State, ctx: ViewCtx<'_>) -> Option<String> {
    if let Some(pane) = state.decide() {
        return (pane.chosen_count() > 0)
            .then(|| fill_size(Msg::SpaceStatusSelected, pane.chosen_bytes(), ctx.lang));
    }
    let bytes = state
        .ticked_bytes(Group::Safe)
        .saturating_add(state.ticked_bytes(Group::Worktrees));
    (bytes > 0 && state.result().is_none())
        .then(|| fill_size(Msg::SpaceStatusSelected, bytes, ctx.lang))
}

fn body<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    if let Some(pane) = state.decide() {
        return decide::view(pane, state, ctx);
    }
    if let Some(done) = state.result() {
        return if done.show_log {
            log::view(done, ctx)
        } else {
            result::view(done, ctx)
        };
    }
    column![
        head::title(state, ctx),
        theme::horizontal_rule(palette.border_inactive),
        head::drive(state, ctx),
        head::actions(state, ctx),
        list::view(state, ctx),
    ]
    .height(Length::Fill)
    .into()
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let page = column![
        column![body(state, ctx)].height(Length::Fill),
        status_bar(state, ctx)
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    // The page stays the first layer whatever lies over it, so the list keeps its scroll.
    let mut layers = stack![page];
    if let Some(ask) = state.confirm() {
        layers = layers.push(scrim(confirm::dialog(ask, ctx)));
    } else if state.menu_open() {
        let rows = menu_rows(state, ctx);
        let origin = menu_origin(state, ctx, menu_rows_height(&rows));
        let menu = popup_menu_rows(rows, POPUP_MENU_WIDTH, origin, ctx.palette);
        layers = layers.push(popup_layer(
            menu,
            Some(Message::CloseMenu),
            RightPress::Dismiss,
        ));
    }
    layers.into()
}

/// What `{ago}` and `{took}` of the measured line are.
pub(crate) fn measured_line(state: &State, ctx: ViewCtx<'_>) -> String {
    match state.scan() {
        Some(scan) => fill_template(
            ctx.lang.text(Msg::SpaceMeasured),
            &[
                ("{ago}", &crate::texts::ago_text(ctx.now, scan.measured_at)),
                ("{took}", &crate::texts::took_text(scan.took)),
            ],
        ),
        None => String::new(),
    }
}
