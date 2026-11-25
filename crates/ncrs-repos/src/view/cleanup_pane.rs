//! The pane at the right of the list that cleans up merged branches and
//! worktrees: what was found per repository, what stays, and the button that
//! deletes what is ticked.

use std::path::Path;

use iced::widget::text::Wrapping;
use iced::widget::{column, container, row, text, Space};
use iced::{alignment, Element, Length, Padding};
use ncrs_git::KeepReason;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_ui_kit::check_rows::{
    check_group_head, check_row, close_cross, worktree_detail, CheckGroupHead, CheckRow, RowOutcome,
};
use ncrs_ui_kit::format::time;
use ncrs_ui_kit::kit::{self, Check, Tone};
use ncrs_ui_kit::page_scroll::page_scroll_padded;
use ncrs_ui_kit::page_text::{page_heading, page_line};
use ncrs_ui_kit::row_scroll::FOCUSED_ROW_ID;
use ncrs_ui_kit::section_head::{section_head, SectionHead};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use crate::cleanup::{
    Cleanup, CleanupMessage, Item, Line, Offer, OfferReason, Phase, RepoState, CLEANUP_LIST_ID,
};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::rows::name_of;
use crate::shortcuts::Shortcut;

/// The width of the pane; the list keeps the rest of the window.
pub const PANE_WIDTH: f32 = 560.0;

fn keep_text(why: KeepReason, lang: Language) -> String {
    let fixed = |msg: Msg| lang.text(msg).to_owned();
    match why {
        KeepReason::DefaultBranch => fixed(Msg::GitCleanupKeepDefault),
        KeepReason::CurrentBranch => fixed(Msg::GitCleanupKeepCurrent),
        KeepReason::Empty => fixed(Msg::GitCleanupKeepEmpty),
        KeepReason::Unpushed { commits: 1 } => fixed(Msg::GitCleanupKeepUnpushedOne),
        KeepReason::Unpushed { commits } => fill_template(
            lang.text(Msg::GitCleanupKeepUnpushedMany),
            &[("{count}", &commits.to_string())],
        ),
        KeepReason::NotMerged => fixed(Msg::GitCleanupKeepNotMerged),
        KeepReason::AfterLastPush => fixed(Msg::GitCleanupKeepAfterLastPush),
        KeepReason::WorktreeChanges { count } => fill_template(
            lang.text(Msg::GitCleanupKeepWorktreeChanges),
            &[("{count}", &count.to_string())],
        ),
        KeepReason::MainWorktree => fixed(Msg::GitCleanupKeepMainWorktree),
        KeepReason::CurrentWorktree => fixed(Msg::GitCleanupKeepCurrentWorktree),
        KeepReason::Detached => fixed(Msg::GitCleanupKeepDetached),
        KeepReason::Locked => fixed(Msg::GitCleanupKeepLocked),
        KeepReason::InKeptWorktree => fixed(Msg::GitCleanupKeepInKeptWorktree),
    }
}

fn offer_text(cleanup: &Cleanup, item: &Item, lang: Language) -> String {
    match item.reason {
        OfferReason::InDefault => {
            let main = cleanup
                .scan(&item.pick.repo)
                .and_then(|scan| scan.default_branch.as_deref())
                .unwrap_or("main");
            fill_template(lang.text(Msg::GitCleanupReasonInMain), &[("{main}", main)])
        }
        OfferReason::Gone => lang.text(Msg::GitCleanupReasonGone).to_owned(),
        OfferReason::WorktreeOnly => lang.text(Msg::GitCleanupReasonWorktree).to_owned(),
    }
}

fn dim<'a>(content: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    text(content)
        .size(font_size::SMALL)
        .color(ctx.palette.dim_text)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

fn problem<'a>(content: String, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    text(content)
        .size(font_size::SMALL)
        .color(ctx.palette.error)
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

fn outcome(cleanup: &Cleanup, item: &Item, ctx: ViewCtx<'_>) -> Option<RowOutcome> {
    Some(match cleanup.result_of(item)? {
        Ok(()) => RowOutcome::Done(ctx.lang.text(Msg::ReposCleanupRowDeleted).to_owned()),
        Err(reason) => RowOutcome::Failed(fill_template(
            ctx.lang.text(Msg::GitCleanupRowFailed),
            &[("{reason}", &reason)],
        )),
    })
}

fn item_row<'a>(cleanup: &Cleanup, item: Item, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let cursor =
        !cleanup.list_focus && cleanup.cursor_line() == Some(Line::Item(item.pick.clone()));
    let ready = cleanup.phase == Phase::Ready;
    let built = check_row(
        CheckRow {
            check: if cleanup.is_ticked(&item.pick) {
                Check::On
            } else {
                Check::Off
            },
            name: item.name.clone(),
            detail: item
                .folder
                .as_ref()
                .map(|folder| worktree_detail(folder, &item.ignored_files, ctx.lang)),
            reason: match (&item.remote, cleanup.delete_remote) {
                (Some(remote), true) => {
                    format!("{} + {remote}", offer_text(cleanup, &item, ctx.lang))
                }
                _ => offer_text(cleanup, &item, ctx.lang),
            },
            worktree: matches!(item.pick.offer, Offer::Worktree(_)),
            kept: false,
            cursor,
            on_toggle: ready.then(|| Message::Cleanup(CleanupMessage::Toggle(item.pick.clone()))),
            outcome: outcome(cleanup, &item, ctx),
        },
        ctx.palette,
    );
    if cursor {
        container(built).id(FOCUSED_ROW_ID).into()
    } else {
        built
    }
}

fn group<'a>(cleanup: &Cleanup, repo: &Path, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let items = cleanup.items(repo);
    let open = !cleanup.folded.contains(repo);
    let subtitle = if open {
        repo.display().to_string()
    } else {
        let worktrees = items
            .iter()
            .filter(|item| matches!(item.pick.offer, Offer::Worktree(_)))
            .count();
        fill_template(
            ctx.lang.text(Msg::ReposCleanupGroupSub),
            &[
                ("{branches}", &(items.len() - worktrees).to_string()),
                ("{worktrees}", &worktrees.to_string()),
            ],
        )
    };
    let head = check_group_head(
        CheckGroupHead {
            title: name_of(repo),
            subtitle,
            count: items.len(),
            open,
            check: cleanup.group_check(repo),
            cursor: !cleanup.list_focus
                && cleanup.cursor_line() == Some(Line::Group(repo.to_path_buf())),
            on_toggle_open: Message::Cleanup(CleanupMessage::FoldGroup(repo.to_path_buf())),
            on_toggle_check: Message::Cleanup(CleanupMessage::ToggleGroup(repo.to_path_buf())),
        },
        ctx.palette,
    );
    let mut body = column![head];
    if open {
        for item in items {
            body = body.push(item_row(cleanup, item, ctx));
        }
        let stays = cleanup
            .scan(repo)
            .map_or(&[][..], |scan| &*scan.list.kept);
        for kept in stays.iter().filter(|kept| kept.is_worktree) {
            let name = Path::new(&kept.name).file_name().map_or_else(
                || kept.name.clone(),
                |last| last.to_string_lossy().into_owned(),
            );
            body = body.push(check_row(
                CheckRow {
                    check: Check::Off,
                    name,
                    detail: None,
                    reason: keep_text(kept.why, ctx.lang),
                    worktree: true,
                    kept: true,
                    cursor: false,
                    on_toggle: None,
                    outcome: None,
                },
                ctx.palette,
            ));
        }
    }
    body.into()
}

fn keep_section<'a>(cleanup: &Cleanup, ctx: ViewCtx<'a>) -> Option<Element<'a, Message>> {
    let kept: Vec<(&Path, &ncrs_git::Kept)> = cleanup
        .order
        .iter()
        .filter_map(|repo| {
            cleanup
                .scan(repo)
                .map(|scan| (repo.as_path(), &scan.list.kept))
        })
        .flat_map(|(repo, kept)| kept.iter().map(move |one| (repo, one)))
        .collect();
    if kept.is_empty() {
        return None;
    }
    let mut section = column![section_head(
        SectionHead {
            title: ctx.lang.text(Msg::ReposCleanupKeepTitle),
            count: kept.len(),
            open: cleanup.keep_open,
            has_focus: false,
            on_toggle: Message::Cleanup(CleanupMessage::ToggleKeep),
        },
        ctx.palette,
    )];
    if cleanup.keep_open {
        let example = kept
            .iter()
            .find(|(_, one)| matches!(one.why, KeepReason::Unpushed { .. }))
            .map_or_else(String::new, |(repo, one)| {
                let sample = format!(
                    "{}/{}: {}",
                    name_of(repo),
                    one.name,
                    keep_text(one.why, ctx.lang)
                );
                fill_template(
                    ctx.lang.text(Msg::ReposCleanupKeepExample),
                    &[("{example}", &sample)],
                )
            });
        section = section.push(
            container(dim(
                fill_template(
                    ctx.lang.text(Msg::ReposCleanupKeepExplain),
                    &[("{example}", &example)],
                ),
                ctx,
            ))
            .padding(Padding::from([spacing::M, spacing::XL])),
        );
    }
    Some(section.into())
}

fn summary_line<'a>(cleanup: &Cleanup, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let text_line = match cleanup.phase {
        Phase::Reading => fill_template(
            lang.text(Msg::ReposCleanupReading),
            &[
                ("{done}", &cleanup.read_count().to_string()),
                ("{total}", &cleanup.order.len().to_string()),
            ],
        ),
        Phase::Ready | Phase::Deleting => {
            let (branches, worktrees, repos) = cleanup.offered();
            fill_template(
                lang.text(Msg::ReposCleanupSummary),
                &[
                    ("{branches}", &branches.to_string()),
                    ("{worktrees}", &worktrees.to_string()),
                    ("{repos}", &repos.to_string()),
                ],
            )
        }
        Phase::Done => match &cleanup.undone {
            Some(undone) => fill_template(
                lang.text(Msg::ReposCleanupUndone),
                &[("{count}", &undone.branches.to_string())],
            ),
            None => fill_template(
                lang.text(Msg::ReposCleanupDeleted),
                &[("{count}", &cleanup.deleted_count().to_string())],
            ),
        },
    };
    let fetched = cleanup.fetched_at.map(|at| {
        fill_template(
            lang.text(Msg::ReposCleanupFetchedAt),
            &[("{time}", &time(Some(at)))],
        )
    });
    let mut line = row![page_line(text_line, font_size::BODY, ctx.palette.text)]
        .align_y(alignment::Vertical::Center);
    line = line.push(Space::new().width(Length::Fill));
    if let Some(at) = fetched {
        line = line.push(page_line(at, font_size::SMALL, ctx.palette.dim_text));
    }
    line.into()
}

fn warnings<'a>(cleanup: &Cleanup, ctx: ViewCtx<'a>) -> Vec<Element<'a, Message>> {
    let mut lines = Vec::new();
    for repo in &cleanup.order {
        let name = name_of(repo);
        match cleanup.repos.get(repo) {
            Some(RepoState::Failed(reason)) => lines.push(problem(
                fill_template(
                    ctx.lang.text(Msg::ReposCleanupReadFailed),
                    &[("{repo}", &name), ("{reason}", reason)],
                ),
                ctx,
            )),
            Some(RepoState::Scanned(scan)) => {
                if let Some(reason) = &scan.fetch_error {
                    lines.push(problem(
                        fill_template(
                            ctx.lang.text(Msg::ReposCleanupFetchFailed),
                            &[("{repo}", &name), ("{reason}", reason)],
                        ),
                        ctx,
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some(undone) = &cleanup.undone {
        for (repo, reason) in &undone.failures {
            lines.push(problem(
                fill_template(
                    ctx.lang.text(Msg::ReposCleanupUndoFailed),
                    &[("{repo}", &name_of(repo)), ("{reason}", reason)],
                ),
                ctx,
            ));
        }
    }
    lines
}

fn remote_option<'a>(cleanup: &Cleanup, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let (count, remote) = cleanup.remote_candidates();
    let ready = cleanup.phase == Phase::Ready;
    let mut text_cell = column![page_line(
        ctx.lang.text(Msg::ReposCleanupDeleteRemote),
        font_size::BODY,
        ctx.palette.text,
    )]
    .spacing(spacing::XS);
    text_cell = text_cell.push(page_line(
        fill_template(
            ctx.lang.text(Msg::ReposCleanupDeleteRemoteSub),
            &[
                ("{count}", &count.to_string()),
                ("{remote}", remote.as_deref().unwrap_or("origin")),
            ],
        ),
        font_size::SMALL,
        ctx.palette.dim_text,
    ));
    row![
        kit::checkbox(
            if cleanup.delete_remote {
                Check::On
            } else {
                Check::Off
            },
            ready.then_some(Message::Cleanup(CleanupMessage::ToggleRemote)),
            ctx.palette,
        ),
        text_cell,
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center)
    .into()
}

fn buttons<'a>(cleanup: &Cleanup, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let (entries, repos) = cleanup.ticked();
    let confirm = fill_template(
        ctx.lang.text(Msg::ReposCleanupConfirm),
        &[
            ("{count}", &entries.to_string()),
            ("{repos}", &repos.to_string()),
        ],
    );
    let can_undo = cleanup.phase == Phase::Done && !cleanup.undos.is_empty();
    let mut line = row![]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if cleanup.phase != Phase::Done {
        line = line.push(kit::button(
            &confirm,
            (cleanup.phase == Phase::Ready && entries > 0)
                .then_some(Message::Cleanup(CleanupMessage::Confirm)),
            Tone::Danger,
            ctx.palette,
        ));
    }
    if cleanup.phase == Phase::Done {
        line = line.push(kit::button(
            &format!(
                "{}  {}",
                ctx.lang.text(Msg::ReposCleanupUndo),
                ctx.keys.label(Shortcut::UndoCleanup)
            ),
            can_undo.then_some(Message::Cleanup(CleanupMessage::Undo)),
            Tone::Secondary,
            ctx.palette,
        ));
    }
    line.push(Space::new().width(Length::Fill))
        .push(kit::button(
            ctx.lang.text(if cleanup.phase == Phase::Done {
                Msg::DialogClose
            } else {
                Msg::ReposCleanupCancel
            }),
            (cleanup.phase != Phase::Deleting).then_some(Message::Cleanup(CleanupMessage::Close)),
            Tone::Secondary,
            ctx.palette,
        ))
        .into()
}

/// The pane, with a rule at its left edge.
pub fn view<'a>(cleanup: &'a Cleanup, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let title = fill_template(
        ctx.lang.text(Msg::ReposCleanupTitle),
        &[("{count}", &cleanup.order.len().to_string())],
    );
    let mut entries = column![].width(Length::Fill);
    for repo in cleanup.offering() {
        entries = entries.push(group(cleanup, repo, ctx));
    }
    if let Some(keep) = keep_section(cleanup, ctx) {
        entries = entries.push(keep);
    }
    let without = cleanup.without_offers();
    let mut notes = column![].spacing(spacing::XS);
    if cleanup.phase != Phase::Reading && !without.is_empty() {
        let names: Vec<String> = without.iter().map(|repo| name_of(repo)).collect();
        notes = notes.push(dim(
            fill_template(
                ctx.lang.text(Msg::ReposCleanupNothing),
                &[("{repos}", &names.join(", "))],
            ),
            ctx,
        ));
    }
    for warning in warnings(cleanup, ctx) {
        notes = notes.push(warning);
    }
    let pane = column![
        row![
            page_heading(&title, palette),
            dim(super::head::scope_text(cleanup.scope, ctx), ctx),
            Space::new().width(Length::Fill),
            close_cross(
                (cleanup.phase != Phase::Deleting)
                    .then_some(Message::Cleanup(CleanupMessage::Close)),
                palette,
            ),
        ]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center),
        dim(ctx.lang.text(Msg::ReposCleanupIntro).to_owned(), ctx),
        summary_line(cleanup, ctx),
        notes,
        page_scroll_padded(CLEANUP_LIST_ID, entries.into(), Padding::from(0.0)),
        remote_option(cleanup, ctx),
        dim(ctx.lang.text(Msg::ReposCleanupUndoHint).to_owned(), ctx),
        buttons(cleanup, ctx),
        dim(ctx.lang.text(Msg::ReposCleanupRunNote).to_owned(), ctx),
    ]
    .spacing(spacing::L)
    .padding(spacing::XL)
    .width(PANE_WIDTH);
    row![
        theme::vertical_rule(palette.border_inactive),
        container(pane)
            .width(PANE_WIDTH)
            .height(Length::Fill)
            .style(theme::panel(palette)),
    ]
    .into()
}
