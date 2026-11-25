//! The left side: local and remote branches, tags, stashes and worktrees,
//! each group under a section head.

use iced::widget::{column, container};
use iced::Element;
use ncrs_git::{BranchInfo, WorktreeInfo};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::action_rows::context_area;
use ncrs_ui_kit::kit::dim;
use ncrs_ui_kit::format::{estimated_text_width, estimated_width, fit_end, fit_middle};
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

/// Tick, branch symbol, the square before the folder and the gaps of a worktree row.
const WORKTREE_FIXED_ROOM: f32 = 70.0;
/// Room for the distance and the number of changes at the right of a worktree row.
const WORKTREE_TRAIL_ROOM: f32 = 60.0;
/// "detached at " and the short id of a detached worktree, in characters.
const DETACHED_TITLE_CHARS: usize = 20;
/// The "merged" label at the right of a merged worktree, in characters.
const MERGED_LABEL_CHARS: usize = 8;
/// The fewest characters of a folder name worth showing.
const FOLDER_MIN_CHARS: usize = 6;

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
            let chosen = has_focus;
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
                    let row = worktree_row_of(worktree, chosen, index, width, ctx);
                    return match menu_target_of(state, item) {
                        Some(target) => context_area(row, Message::OpenMenu(target)),
                        None => row,
                    };
                }
            }
            let (depth, open, label, trail) = match item {
                SidebarItem::RemoteHeader(position) => {
                    let (name, count) = remote_label(state, position);
                    let folded = state.collapsed.contains(&Fold::Remote(name.clone()));
                    (1, Some(!folded), format!("{name} ({count})"), String::new())
                }
                other => {
                    let (label, trail) = child_label(state, other, ctx.lang);
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
            let menu_target = menu_target_of(state, item);
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
    let mut tree = column(rows);
    if !state.actions.cleanable.is_empty() {
        tree = tree.push(
            container(dim(ctx.lang.text(Msg::GitViewLegendMerged), ctx.palette))
                .padding([spacing::M, spacing::L]),
        );
    }
    tree_scroll(SIDEBAR_ID, tree.into())
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

/// How far a branch is from its upstream, "↑0 ↓2"; none for a branch without one.
fn distance(branch: &BranchInfo) -> String {
    if !branch.has_upstream {
        return String::new();
    }
    format!("\u{2191}{} \u{2193}{}", branch.ahead, branch.behind)
}

/// The label of a leaf and what stands at the right of it.
fn child_label(state: &State, item: SidebarItem, lang: Language) -> (String, String) {
    let Some(overview) = &state.overview else {
        return (String::new(), String::new());
    };
    match item {
        SidebarItem::Local(index) => overview
            .local
            .get(index)
            .map(|branch| {
                let mark = state.actions.cleanable.contains(&branch.name);
                let merged = if mark {
                    lang.text(Msg::GitViewBranchMerged).to_owned()
                } else {
                    String::new()
                };
                let trail = [distance(branch), merged]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                (branch.name.clone(), trail)
            })
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
        SidebarItem::Worktree(_) | SidebarItem::Header(_) | SidebarItem::RemoteHeader(_) => {
            (String::new(), String::new())
        }
    }
}

fn worktree_row_of<'a>(
    worktree: &WorktreeInfo,
    chosen: bool,
    index: usize,
    width: f32,
    ctx: ViewCtx<'a>,
) -> Element<'a, Message> {
    let folder = worktree
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let branch_name = worktree
        .branch
        .clone()
        .or_else(|| worktree.head_short.clone())
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
    // What stands in the row besides the folder: tick, symbol, name, the distance and the changes.
    let name_room = match state {
        WorktreeState::Detached => estimated_text_width(DETACHED_TITLE_CHARS, font_size::BODY),
        _ => estimated_text_width(branch_name.chars().count(), font_size::BODY),
    };
    let merged_room = if state == WorktreeState::Merged {
        estimated_text_width(MERGED_LABEL_CHARS, font_size::SMALL)
    } else {
        0.0
    };
    let beside = WORKTREE_FIXED_ROOM + name_room + WORKTREE_TRAIL_ROOM + merged_room;
    let branch_name = fit_end(
        &branch_name,
        (label_budget(width, 0, "") - WORKTREE_FIXED_ROOM - WORKTREE_TRAIL_ROOM - merged_room)
            .max(0.0),
        font_size::BODY,
    );
    let room = label_budget(width, 0, "") - beside;
    // A folder that would shrink to a few letters says nothing: leave it out.
    let folder = if room < estimated_text_width(FOLDER_MIN_CHARS, font_size::BODY) {
        String::new()
    } else {
        fit_middle(&folder, room, font_size::BODY)
    };
    worktree_line(
        WorktreeLine {
            state,
            branch: branch_name,
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
