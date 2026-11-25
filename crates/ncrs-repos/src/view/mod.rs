//! The Repos tab: head, bulk bar, result banner, grouped list, log and status bar.

mod clone;
mod head;
mod list;
mod log;

use iced::widget::{column, stack};
use iced::{Element, Length, Point};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::scrim;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::popup_menu::{popup_layer, popup_menu, PopupEntry, RightPress};
use ncrs_ui_kit::theme;

use crate::bulk::BulkAction;
use crate::ctx::ViewCtx;
use crate::message::{MenuEntry, Message};
use crate::shortcuts::Shortcut;
use crate::state::State;


/// Where the menu opens: the keyboard and the right click both land here.
const MENU_ORIGIN: Point = Point::new(280.0, 150.0);

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
    page_status_bar(
        PageStatusBar {
            mode: ctx.lang.text(Msg::ReposMode),
            line,
            right: None,
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
    let full_page = page.push(status_bar(state, ctx)).height(Length::Fill);
    if let Some(draft) = state.clone_draft() {
        stack![full_page, scrim(clone::dialog(draft, ctx))].into()
    } else if state.menu_open() {
        let menu = popup_menu(menu_entries(state, ctx), MENU_ORIGIN, palette);
        stack![
            full_page,
            popup_layer(menu, Some(Message::CloseMenu), RightPress::Dismiss)
        ]
        .into()
    } else {
        full_page.into()
    }
}
