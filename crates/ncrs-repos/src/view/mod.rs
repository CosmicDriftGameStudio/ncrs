//! The Repos tab: head, bulk bar, result banner, grouped list, log and status bar.

mod cleanup_pane;
mod clone;
mod head;
mod list;
mod log;

use iced::widget::{column, row, stack};
use iced::{Element, Length, Point};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::scrim;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::popup_menu::{popup_layer, popup_menu, PopupEntry, RightPress};
use ncrs_ui_kit::theme::{self, size};

use crate::bulk::BulkAction;
use crate::cleanup::Phase;
use crate::ctx::ViewCtx;
use crate::message::{MenuEntry, Message};
use crate::shortcuts::Shortcut;
use crate::state::State;


/// Where the menu opens horizontally; its height follows the row of the cursor.
const MENU_X: f32 = 280.0;
/// The title, the bar of the actions, the rule and the column head above the list.
const LIST_TOP: f32 = 32.0 + 1.0 + 32.0 + 22.0;
/// What the menu needs below its top edge before it must move up.
const MENU_HEIGHT: f32 = 300.0;

/// The title of the tab.
pub fn tab_title(ctx: ViewCtx<'_>) -> String {
    ctx.lang.text(Msg::ReposTitle).to_owned()
}

fn menu_entries(state: &State, ctx: ViewCtx<'_>) -> Vec<PopupEntry<Message>> {
    let target = state.cursor();
    let unreachable = target
        .as_deref()
        .and_then(|path| state.reading(path))
        .is_some_and(crate::summary::Reading::is_unreachable);
    let entry = |label: Msg, key: Option<Shortcut>, pick: MenuEntry, enabled: bool| PopupEntry {
        label: ctx.lang.text(label).to_owned(),
        key: key.map(|shortcut| ctx.keys.label(shortcut)),
        is_selected: false,
        on_press: enabled.then_some(Message::MenuPicked(pick)),
        on_hover: None,
    };
    let run = ctx.actions_available;
    let mut entries = vec![
        entry(Msg::ReposOpen, None, MenuEntry::Open, !unreachable),
        entry(
            Msg::ReposToggleFavourite,
            Some(Shortcut::Favourite),
            MenuEntry::Favourite,
            true,
        ),
    ];
    if unreachable {
        entries.push(entry(
            Msg::ReposRemove,
            Some(Shortcut::Remove),
            MenuEntry::Remove,
            true,
        ));
    }
    entries.extend([
        entry(
            Msg::GitViewToolbarFetch,
            Some(Shortcut::Fetch),
            MenuEntry::Bulk(BulkAction::Fetch),
            run,
        ),
        entry(
            Msg::ReposActionSwitchDefault,
            Some(Shortcut::SwitchToDefault),
            MenuEntry::Bulk(BulkAction::SwitchToDefault),
            run,
        ),
        entry(
            Msg::ReposActionPullFf,
            Some(Shortcut::PullFastForward),
            MenuEntry::Bulk(BulkAction::PullFastForward),
            run,
        ),
        entry(
            Msg::ReposActionBoth,
            Some(Shortcut::Both),
            MenuEntry::Bulk(BulkAction::Both),
            run,
        ),
        entry(
            Msg::ReposCommandCleanup,
            Some(Shortcut::CleanUp),
            MenuEntry::Cleanup,
            run,
        ),
        entry(
            Msg::ReposOpenInTabs,
            Some(Shortcut::OpenInTabs),
            MenuEntry::OpenInTabs,
            true,
        ),
    ]);
    entries
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
    page = page.push(list::view(state, ctx));
    if let Some(run) = state.run() {
        page = page.push(log::view(run, ctx));
    }
    let page = page.width(Length::Fill).height(Length::Fill);
    let upper: Element<'a, Message> = match &state.cleanup {
        Some(cleanup) => row![page, cleanup_pane::view(cleanup, ctx)]
            .height(Length::Fill)
            .into(),
        None => page.into(),
    };
    let full_page = column![upper, status_bar(state, ctx)].height(Length::Fill);
    if let Some(draft) = state.clone_draft() {
        stack![full_page, scrim(clone::dialog(draft, ctx))].into()
    } else if state.menu_open() {
        let menu = popup_menu(menu_entries(state, ctx), menu_origin(state, ctx), palette);
        stack![
            full_page,
            popup_layer(menu, Some(Message::CloseMenu), RightPress::Dismiss)
        ]
        .into()
    } else {
        full_page.into()
    }
}

/// The menu opens at the row of the cursor, where the pointer went for a
/// right click and where the keyboard is. The scroll position of the list is
/// not known here, so a scrolled list puts it too low or too high.
fn menu_origin(state: &State, ctx: ViewCtx<'_>) -> Point {
    let cursor = state.cursor();
    let mut top = LIST_TOP;
    'found: for group in state.groups() {
        top += size::SECTION_HEAD;
        for row in &group.rows {
            if Some(&row.path) == cursor.as_ref() {
                break 'found;
            }
            top += size::REPO_ROW;
        }
    }
    Point::new(MENU_X, top.min((ctx.window.height - MENU_HEIGHT).max(0.0)))
}
