//! The left side: local and remote branches, tags, stashes and worktrees.

use iced::widget::column;
use iced::Element;
use ncrs_git::WorktreeInfo;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::format::{estimated_width, fit_end};
use ncrs_ui_kit::git_rows::{worktree_row, WorktreeRow};
use ncrs_ui_kit::theme::{font_size, size, spacing};
use ncrs_ui_kit::tree_list::{
    tree_row, tree_scroll, TreeRow, LIST_INSET, ROW_OVERHEAD, TREE_INDENT,
};

use super::SIDEBAR_ID;
use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::state::{remote_groups, Area, Fold, Section, SidebarItem, State};

/// Room the scroll bar of the list takes from a label.
const SCROLL_BAR_ROOM: f32 = 12.0;

/// How much of a label of a row `depth` levels down and `trail` wide shows
/// before it is cut with an ellipsis.
fn label_budget(depth: usize, trail: &str) -> f32 {
    let trail_room = if trail.is_empty() {
        0.0
    } else {
        estimated_width(trail, font_size::SMALL) + spacing::XS
    };
    size::SIDE_PANE
        - 2.0 * LIST_INSET
        - ROW_OVERHEAD
        - spacing::CELL_PADDING_X
        - TREE_INDENT * depth as f32
        - SCROLL_BAR_ROOM
        - trail_room
}

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
            let (depth, open, label, trail) = match item {
                SidebarItem::Header(section) => (
                    0,
                    Some(!state.collapsed.contains(&Fold::Section(section))),
                    format!(
                        "{} ({})",
                        ctx.lang.text(section_title(section)),
                        state.section_count(section)
                    ),
                    String::new(),
                ),
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
            let fitted = fit_end(&label, label_budget(depth, &trail), font_size::BODY);
            tree_row(
                TreeRow {
                    depth,
                    open,
                    label: fitted,
                    chosen,
                    dim: false,
                    lead: None,
                    trail: if trail.is_empty() {
                        Vec::new()
                    } else {
                        vec![(trail, ctx.palette.dim_text)]
                    },
                    on_press: Message::SidebarPressed(index),
                },
                ctx.palette,
            )
        });
    tree_scroll(SIDEBAR_ID, column(rows).into())
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

/// The label of a leaf and what stands at the right of it.
fn child_label(state: &State, item: SidebarItem) -> (String, String) {
    let Some(overview) = &state.overview else {
        return (String::new(), String::new());
    };
    match item {
        SidebarItem::Local(index) => overview
            .local
            .get(index)
            .map(|branch| {
                let mark = if branch.is_current { "\u{25cf} " } else { "" };
                let distance = if branch.has_upstream {
                    format!("\u{2191}{} \u{2193}{}", branch.ahead, branch.behind)
                } else {
                    String::new()
                };
                (format!("{mark}{}", branch.name), distance)
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
        SidebarItem::Worktree(_) | SidebarItem::Header(_) | SidebarItem::RemoteHeader(_) => {
            (String::new(), String::new())
        }
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
