//! The top of the tab: title and summary, the buttons of the list, the bar of
//! the bulk actions and, after a run, the result banner.

use std::path::PathBuf;

use iced::widget::{container, row, Space};
use iced::{alignment, Element, Length};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::kit::{self, text_field, Check, GitAction, Tone};
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::repo_row::result_banner;
use ncrs_ui_kit::theme::{font_size, size, spacing};

use crate::bulk::BulkAction;
use crate::cleanup::CleanupMessage;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::shortcuts::Shortcut;
use crate::state::{Scope, State};
use crate::update::FILTER_ID;

pub fn title<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let (total, changed, behind) = state.counts();
    let summary = fill_template(
        ctx.lang.text(Msg::ReposSummary),
        &[
            ("{total}", &total.to_string()),
            ("{changed}", &changed.to_string()),
            ("{behind}", &behind.to_string()),
        ],
    );
    let scan_label = if state.is_scanning() {
        Msg::ReposScanning
    } else {
        Msg::ReposScan
    };
    let filter = text_field(
        ctx.lang.text(Msg::ReposSearchPlaceholder),
        state.filter(),
        FILTER_ID,
        Some(Message::FilterChanged),
        Message::FilterSubmitted,
        palette,
    );
    container(
        row![
            page_heading(ctx.lang.text(Msg::ReposTitle), palette),
            page_line(summary, font_size::BODY, palette.dim_text),
            Space::new().width(Length::Fill),
            kit::button(
                ctx.lang.text(scan_label),
                Some(Message::ScanNow),
                Tone::Secondary,
                palette,
            ),
            kit::button(
                ctx.lang.text(Msg::ReposAdd),
                Some(Message::AddRepo),
                Tone::Secondary,
                palette,
            ),
            kit::button(
                ctx.lang.text(Msg::ReposClone),
                ctx.actions_available.then_some(Message::CloneOpen),
                Tone::Secondary,
                palette,
            ),
            container(filter).width(size::FIELD_WIDE),
        ]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center),
    )
    .padding([0.0, spacing::XXL])
    .height(size::HEADER)
    .center_y(size::HEADER)
    .into()
}

/// What the actions run on; a single repository by its name.
pub fn scope_text(scope: Scope, targets: &[PathBuf], ctx: ViewCtx<'_>) -> String {
    let (msg, count) = match scope {
        Scope::Marked(count) => (Msg::ReposSelected, count),
        Scope::Favourites(count) => (Msg::ReposScopeFavourites, count),
        Scope::Row => match targets.first().and_then(|path| path.file_name()) {
            Some(name) => return name.to_string_lossy().into_owned(),
            None => (Msg::ReposScopeRow, 1),
        },
    };
    fill_template(ctx.lang.text(msg), &[("{n}", &count.to_string())])
}

/// The bar of the bulk actions: what they apply to, the four buttons, the
/// tabs button and the filter for what needs an action.
pub fn actions<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let (scope, targets) = state.targets();
    let busy = state.run().is_some_and(|run| !run.is_finished()) || state.cleanup.is_some();
    let can_run = ctx.actions_available && !busy && !targets.is_empty();
    let git_button = |git_action: GitAction, what: Msg, shortcut: Shortcut, message: Message| {
        let key = ctx.keys.label(shortcut);
        let tip = if key.is_empty() {
            ctx.lang.text(what).to_owned()
        } else {
            format!("{} ({key})", ctx.lang.text(what))
        };
        kit::icon_button(
            git_action.icon(),
            None,
            &tip,
            can_run.then_some(message),
            Tone::Secondary,
            palette,
        )
    };
    let bulk = |action: BulkAction, shortcut: Shortcut| {
        let (git_action, what) = match action {
            BulkAction::Fetch => (GitAction::Fetch, Msg::GitViewToolbarFetch),
            BulkAction::SwitchToDefault => (GitAction::SwitchToDefault, Msg::ReposActionSwitchDefault),
            BulkAction::PullFastForward => (GitAction::Pull, Msg::ReposActionPullFf),
            BulkAction::Both => (GitAction::SwitchAndPull, Msg::ReposActionBoth),
        };
        git_button(git_action, what, shortcut, Message::Bulk(action))
    };
    let marked = state.marked_repos().len();
    let mut line = row![]
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center);
    let scope_label = fill_template(
        ctx.lang.text(Msg::ReposScopeLabel),
        &[("{what}", &scope_text(scope, &targets, ctx))],
    );
    line = line
        .push(page_line(&scope_label, font_size::BODY, palette.dim_text))
        .push(bulk(BulkAction::Fetch, Shortcut::Fetch))
        .push(bulk(BulkAction::SwitchToDefault, Shortcut::SwitchToDefault))
        .push(bulk(BulkAction::PullFastForward, Shortcut::PullFastForward))
        .push(bulk(BulkAction::Both, Shortcut::Both))
        .push(git_button(
            GitAction::CleanUp,
            Msg::ReposToolbarCleanup,
            Shortcut::CleanUp,
            Message::Cleanup(CleanupMessage::Open),
        ))
        .push(kit::button(
            ctx.lang.text(Msg::ReposOpenInTabs),
            (marked > 0 && !busy).then_some(Message::OpenInTabs),
            Tone::Secondary,
            palette,
        ))
        .push(Space::new().width(Length::Fill));
    if !ctx.actions_available {
        line = line.push(page_line(
            ctx.lang.text(Msg::GitViewHintSandbox),
            font_size::SMALL,
            palette.dim_text,
        ));
    }
    let only_action = row![
        kit::checkbox(
            if state.only_needing_action() {
                Check::On
            } else {
                Check::Off
            },
            Some(Message::OnlyNeedingActionToggled),
            palette,
        ),
        page_line(
            ctx.lang.text(Msg::ReposOnlyNeedsAction),
            font_size::BODY,
            palette.text
        ),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    container(line.push(only_action))
        .padding([0.0, spacing::XXL])
        .height(size::HEADER)
        .center_y(size::HEADER)
        .into()
}

/// The result of a run, until it is closed.
pub fn banner<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let run = state.run().filter(|run| run.is_finished())?;
    let palette = ctx.palette;
    let problems = if run.only_problems() {
        Msg::ReposAllResults
    } else {
        Msg::ReposOnlyProblems
    };
    let failed = !run.failed().is_empty() && ctx.actions_available;
    let buttons = row![
        kit::button(
            ctx.lang.text(problems),
            Some(Message::OnlyProblemsToggled),
            Tone::Secondary,
            palette,
        ),
        kit::button(
            &format!(
                "{}  {}",
                ctx.lang.text(Msg::ReposRetryFailed),
                ctx.keys.label(Shortcut::RetryFailed)
            ),
            failed.then_some(Message::RetryFailed),
            Tone::Secondary,
            palette,
        ),
    ]
    .spacing(spacing::M);
    Some(result_banner(
        &run.summary(),
        ctx.lang,
        palette,
        Some(buttons.into()),
        Message::CloseResult,
    ))
}
