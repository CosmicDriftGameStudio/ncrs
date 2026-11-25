//! The left side: local and remote branches, tags, stashes and worktrees,
//! each group under a section head.

use iced::widget::column;
use iced::Element;
use ncrs_git::{BranchInfo, WorktreeInfo};
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::action_rows::context_area;
use ncrs_ui_kit::format::{estimated_width, fit_end};
use ncrs_ui_kit::git_lines::{worktree_line, WorktreeLine, WorktreeState};
use ncrs_ui_kit::section_head::{section_head, SectionHead};
use ncrs_ui_kit::theme::{font_size, git, spacing};
use ncrs_ui_kit::tree_list::{
    tree_row, tree_row_strong, tree_scroll, TreeRow, LIST_INSET, ROW_OVERHEAD, TREE_INDENT,
};

use super::{sidebar_width, SIDEBAR_ID};
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{remote_groups, Area, Fold, Section, SidebarItem, State};
use crate::update::menu_target_of;

/// Room the scroll bar of the list takes from a label.
const SCROLL_BAR_ROOM: f32 = 12.0;

/// How much of a label of a row `depth` levels down and `trail` wide shows
/// before it is cut with an ellipsis.
fn label_budget(width: f32, depth: usize, trail: &str) -> f32 {
    let trail_room = if trail.is_empty() {
        0.0
    } else {
        estimated_width(trail, font_size::SMALL) + spacing::XS
    };
    width
        - 2.0 * LIST_INSET
        - ROW_OVERHEAD
        - spacing::CELL_PADDING_X
        - TREE_INDENT * depth as f32
        - SCROLL_BAR_ROOM
        - trail_room
}

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let width = sidebar_width(ctx, state);
    let rows = state
        .sidebar_items()
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let has_focus = state.area == Area::Sidebar && index == state.sidebar_cursor;
            let chosen = has_focus || item == SidebarItem::Changes && state.actions.changes_open;
            if let SidebarItem::Header(section) = item {
                return section_head(
                    SectionHead {
                        title: ctx.lang.text(section_title(section)),
                        count: state.section_count(section),
                        open: !state.collapsed.contains(&Fold::Section(section)),
                        has_focus,
                        on_toggle: Message::SidebarPressed(index),
                    },
                    ctx.palette,
                );
            }
            if let (SidebarItem::Worktree(which), Some(overview)) = (item, &state.overview) {
                if let Some(worktree) = overview.worktrees.get(which) {
                    return worktree_row_of(worktree, chosen, index, ctx);
                }
            }
            let (depth, open, label, trail) = match item {
                SidebarItem::Changes => {
                    let count = state
                        .actions
                        .local
                        .as_ref()
                        .map_or(0, |local| local.unstaged.len() + local.staged.len());
                    (
                        0,
                        None,
                        fill_template(
                            ctx.lang.text(Msg::GitViewWorkingCopy),
                            &[("{count}", &count.to_string())],
                        ),
                        String::new(),
                    )
                }
                SidebarItem::RemoteHeader(position) => {
                    let (name, count) = remote_label(state, position);
                    let folded = state.collapsed.contains(&Fold::Remote(name.clone()));
                    (1, Some(!folded), format!("{name} ({count})"), String::new())
                }
                other => {
                    let (label, trail) = child_label(state, other);
                    (
                        if matches!(other, SidebarItem::Remote(_)) {
                            2
                        } else {
                            1
                        },
                        None,
                        label,
                        trail,
                    )
                }
            };
            let current = is_current_branch(state, item);
            let fitted = fit_end(&label, label_budget(width, depth, &trail), font_size::BODY);
            let menu_target = menu_target_of(item);
            let tree = TreeRow {
                depth,
                open,
                label: fitted,
                chosen,
                dim: false,
                lead: current.then(|| ("\u{2713}".to_owned(), git::purple(ctx.palette))),
                trail: if trail.is_empty() {
                    Vec::new()
                } else {
                    vec![(trail, ctx.palette.dim_text)]
                },
                on_press: Message::SidebarPressed(index),
            };
            let row = if current {
                tree_row_strong(tree, ctx.palette)
            } else {
                tree_row(tree, ctx.palette)
            };
            match menu_target {
                Some(target) => context_area(row, Message::OpenMenu(target)),
                None => row,
            }
        });
    tree_scroll(SIDEBAR_ID, column(rows).into())
}

fn is_current_branch(state: &State, item: SidebarItem) -> bool {
    let SidebarItem::Local(index) = item else {
        return false;
    };
    state
        .overview
        .as_ref()
        .and_then(|overview| overview.local.get(index))
        .is_some_and(|branch| branch.is_current)
}

fn remote_label(state: &State, position: usize) -> (String, usize) {
    let Some(overview) = &state.overview else {
        return (String::new(), 0);
    };
    remote_groups(overview)
        .get(position)
        .map(|group| (group.name.to_owned(), group.branches.len()))
        .unwrap_or_default()
}

fn section_title(section: Section) -> Msg {
    match section {
        Section::Local => Msg::GitViewSectionLocal,
        Section::Remotes => Msg::GitViewSectionRemotes,
        Section::Tags => Msg::GitViewSectionTags,
        Section::Stashes => Msg::GitViewSectionStashes,
        Section::Worktrees => Msg::GitViewSectionWorktrees,
    }
}

/// How far a branch is from its upstream; a distance of zero is not written.
fn distance(branch: &BranchInfo) -> String {
    if !branch.has_upstream {
        return String::new();
    }
    let mut parts = Vec::new();
    if branch.behind > 0 {
        parts.push(format!("\u{2193}{}", branch.behind));
    }
    if branch.ahead > 0 {
        parts.push(format!("\u{2191}{}", branch.ahead));
    }
    parts.join(" ")
}

/// The label of a leaf and what stands at the right of it.
fn child_label(state: &State, item: SidebarItem) -> (String, String) {
    let Some(overview) = &state.overview else {
        return (String::new(), String::new());
    };
    match item {
        SidebarItem::Local(index) => overview
            .local
            .get(index)
            .map(|branch| (branch.name.clone(), distance(branch)))
            .unwrap_or_default(),
        SidebarItem::Remote(index) => {
            let label = remote_groups(overview)
                .iter()
                .flat_map(|group| group.branches.iter())
                .find(|(at, _)| *at == index)
                .map(|(_, branch)| (*branch).to_owned())
                .unwrap_or_default();
            (label, String::new())
        }
        SidebarItem::Tag(index) => (
            overview.tags.get(index).cloned().unwrap_or_default(),
            String::new(),
        ),
        SidebarItem::Stash(index) => state
            .actions
            .stashes
            .get(index)
            .map(|stash| (stash.message.clone(), stash.origin.clone()))
            .unwrap_or_default(),
        SidebarItem::Worktree(_)
        | SidebarItem::Header(_)
        | SidebarItem::RemoteHeader(_)
        | SidebarItem::Changes => (String::new(), String::new()),
    }
}

fn worktree_row_of<'a>(
    worktree: &WorktreeInfo,
    chosen: bool,
    index: usize,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let folder = worktree
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let state = if worktree.is_current {
        WorktreeState::Current
    } else if worktree.branch.is_none() {
        WorktreeState::Detached
    } else if !worktree.is_main && worktree.merged {
        WorktreeState::Merged
    } else {
        WorktreeState::Other
    };
    worktree_line(
        WorktreeLine {
            state,
            branch: worktree
                .branch
                .clone()
                .or_else(|| worktree.head_short.clone())
                .unwrap_or_default(),
            folder,
            ahead: worktree.upstream_ahead,
            behind: worktree.upstream_behind,
            changes: u32::try_from(worktree.wip).unwrap_or(u32::MAX),
            chosen,
            on_press: Message::SidebarPressed(index),
        },
        ctx.lang,
        ctx.palette,
    )
}
