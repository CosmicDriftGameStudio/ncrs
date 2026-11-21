//! The left side: local and remote branches, tags, stashes and worktrees.

use iced::widget::column;
use iced::Element;
use ncrs_git::WorktreeInfo;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::git_rows::{worktree_row, WorktreeRow};
use ncrs_ui_kit::tree_list::{tree_row, tree_scroll, TreeRow};

use super::SIDEBAR_ID;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{Area, Section, SidebarItem, State};

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let rows = state
        .sidebar_items()
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let chosen = state.area == Area::Sidebar && index == state.sidebar_cursor;
            if let (SidebarItem::Worktree(which), Some(overview)) = (item, &state.overview) {
                if let Some(worktree) = overview.worktrees.get(which) {
                    return worktree_row_of(
                        worktree,
                        overview.main_branch.as_deref(),
                        chosen,
                        index,
                        ctx,
                    );
                }
            }
            let (depth, open, label) = match item {
                SidebarItem::Header(section) => (
                    0,
                    Some(!state.collapsed.contains(&section)),
                    format!(
                        "{} ({})",
                        ctx.lang.text(section_title(section)),
                        state.section_count(section)
                    ),
                ),
                other => (1, None, child_label(state, other)),
            };
            tree_row(
                TreeRow {
                    depth,
                    open,
                    label,
                    chosen,
                    dim: false,
                    lead: None,
                    trail: Vec::new(),
                    on_press: Message::SidebarPressed(index),
                },
                ctx.palette,
            )
        });
    tree_scroll(SIDEBAR_ID, column(rows).into())
}

fn section_title(section: Section) -> Msg {
    match section {
        Section::Local => Msg::GitViewSectionLocal,
        Section::Remote => Msg::GitViewSectionRemote,
        Section::Tags => Msg::GitViewSectionTags,
        Section::Stashes => Msg::GitViewSectionStashes,
        Section::Worktrees => Msg::GitViewSectionWorktrees,
    }
}

fn child_label(state: &State, item: SidebarItem) -> String {
    let Some(overview) = &state.overview else {
        return String::new();
    };
    match item {
        SidebarItem::Local(index) => overview
            .local
            .get(index)
            .map(|branch| {
                let mark = if branch.is_current { "\u{25cf} " } else { "" };
                if branch.has_upstream {
                    format!(
                        "{mark}{} \u{2191}{} \u{2193}{}",
                        branch.name, branch.ahead, branch.behind
                    )
                } else {
                    format!("{mark}{}", branch.name)
                }
            })
            .unwrap_or_default(),
        SidebarItem::Remote(index) => overview.remote.get(index).cloned().unwrap_or_default(),
        SidebarItem::Tag(index) => overview.tags.get(index).cloned().unwrap_or_default(),
        SidebarItem::Worktree(_) | SidebarItem::Header(_) => String::new(),
    }
}

fn worktree_row_of<'a>(
    worktree: &WorktreeInfo,
    main: Option<&str>,
    chosen: bool,
    index: usize,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let lang = ctx.lang;
    let folder = worktree
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let branch = worktree
        .branch
        .clone()
        .or_else(|| worktree.head_short.clone())
        .unwrap_or_default();
    let state = if worktree.branch.is_none() {
        lang.text(Msg::GitViewWorktreeDetached).to_owned()
    } else if !worktree.is_main && worktree.merged {
        lang.text(Msg::GitViewWorktreeMerged).to_owned()
    } else if worktree.wip == 0 {
        lang.text(Msg::GitViewWorktreeClean).to_owned()
    } else {
        fill_template(
            lang.text(Msg::GitViewWorktreeToMain),
            &[("{main}", main.unwrap_or_default())],
        )
    };
    worktree_row(
        WorktreeRow {
            depth: 1,
            name: folder,
            wip: (worktree.wip > 0).then(|| {
                fill_template(
                    lang.text(Msg::GitViewWorktreeWip),
                    &[("{count}", &worktree.wip.to_string())],
                )
            }),
            distance: (!worktree.is_main).then_some((worktree.ahead, worktree.behind)),
            detail: if worktree.branch.as_deref() == main {
                branch
            } else {
                format!("{branch} \u{b7} {state}")
            },
            chosen,
            on_press: Message::SidebarPressed(index),
        },
        ctx.palette,
    )
}
