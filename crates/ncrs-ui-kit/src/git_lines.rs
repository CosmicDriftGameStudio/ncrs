//! Lines of the Git views that come on top of the plain rows: the work in
//! progress above the commits, a worktree on one line, and what the names on
//! a commit are called when a local branch and its remote are the same place.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineDash, Path, Stroke};
use iced::widget::{button, container, row, text, Space};
use iced::{alignment, mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::commit_graph::{lane_x, LANE_WIDTH};
use crate::file_icon::git_branch_icon;
use crate::git_rows::{CommitColumns, RefChipKind};
use crate::palette::Palette;
use crate::theme::{self, font_size, git, size, spacing};
use crate::{APP_FONT, APP_FONT_BOLD};

const RING_RADIUS: f32 = 5.0;
const RING_WIDTH: f32 = 1.5;
const DASH: [f32; 2] = [2.0, 2.0];
const BRANCH_ICON: f32 = 12.0;

/// The ring of the work in progress, a dashed line down to the commits.
struct WipRing;

impl<M> canvas::Program<M> for WipRing {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let middle = bounds.height / 2.0;
        let stroke = || Stroke {
            line_dash: LineDash {
                segments: &DASH,
                offset: 0,
            },
            ..Stroke::default()
                .with_color(git::cyan())
                .with_width(RING_WIDTH)
        };
        frame.stroke(
            &Path::line(
                Point::new(lane_x(0), middle + RING_RADIUS),
                Point::new(lane_x(0), bounds.height),
            ),
            stroke(),
        );
        frame.stroke(
            &Path::circle(Point::new(lane_x(0), middle), RING_RADIUS),
            stroke(),
        );
        vec![frame.into_geometry()]
    }
}

/// The first row of the commit list while the folder has changes that are not
/// committed.
#[derive(Debug)]
pub struct WipRow<M> {
    pub changed: u32,
    pub staged: u32,
    pub chosen: bool,
    /// How many lanes the graph column of the list is wide.
    pub lanes: usize,
    pub columns: CommitColumns,
    pub show_sha: bool,
    pub on_press: M,
}

/// "Local changes", how many files are changed and staged, and "WIP" where
/// the other rows have a commit id.
pub fn wip_row<'a, M: Clone + 'a>(
    wip: WipRow<M>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let dim = if wip.chosen {
        palette.cursor_text
    } else {
        palette.dim_text
    };
    let title_ink = if wip.chosen {
        palette.cursor_text
    } else {
        git::cyan()
    };
    let count =
        |msg: Msg, number: u32| fill_template(lang.text(msg), &[("{count}", &number.to_string())]);
    let ring = Canvas::new(WipRing)
        .width(LANE_WIDTH * wip.lanes.max(1) as f32)
        .height(size::ROW);
    let message = row![
        text(lang.text(Msg::GitViewWipTitle))
            .size(font_size::BODY)
            .font(APP_FONT_BOLD)
            .color(title_ink)
            .wrapping(text::Wrapping::None),
        text(format!(
            "{} \u{b7} {}",
            count(Msg::GitViewWipChanged, wip.changed),
            count(Msg::GitViewWipStaged, wip.staged)
        ))
        .size(font_size::BODY)
        .color(dim)
        .wrapping(text::Wrapping::None),
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    let cell = |content: &str, width: u16| {
        container(
            text(content.to_owned())
                .size(font_size::BODY)
                .color(dim)
                .wrapping(text::Wrapping::None),
        )
        .width(f32::from(width))
        .clip(true)
    };
    let line = row![
        ring,
        container(message).width(Length::Fill).clip(true),
        cell("", wip.columns.author),
        cell("", wip.columns.date),
    ]
    .extend(
        wip.show_sha
            .then(|| Element::from(cell(lang.text(Msg::GitViewWipMarker), wip.columns.sha))),
    )
    .spacing(size::COLUMN_GAP)
    .align_y(alignment::Vertical::Center);
    button(
        container(line)
            .padding([0.0, spacing::CELL_PADDING_X])
            .center_y(Length::Fill),
    )
    .on_press(wip.on_press)
    .width(Length::Fill)
    .height(size::ROW)
    .padding(0.0)
    .style(theme::menu_item(palette, wip.chosen))
    .into()
}

/// What a worktree is, as far as its line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeState {
    /// The one the view is open in: a tick in front.
    Current,
    Other,
    /// No branch is checked out, only a commit: in orange.
    Detached,
    /// Everything on its branch is in the main branch: dimmed.
    Merged,
}

/// A worktree on one line.
#[derive(Debug)]
pub struct WorktreeLine<M> {
    pub state: WorktreeState,
    /// The branch; the short commit id for a detached worktree.
    pub branch: String,
    pub folder: String,
    /// Commits the upstream lacks and has; 0 is not drawn.
    pub ahead: u32,
    pub behind: u32,
    /// Files with changes; 0 is not drawn.
    pub changes: u32,
    pub chosen: bool,
    pub on_press: M,
}

/// The name after the branch symbol.
pub fn worktree_title<M>(line: &WorktreeLine<M>, lang: Language) -> String {
    match line.state {
        WorktreeState::Detached => fill_template(
            lang.text(Msg::GitViewWorktreeDetachedAt),
            &[("{sha}", &line.branch)],
        ),
        WorktreeState::Current | WorktreeState::Other | WorktreeState::Merged => {
            line.branch.clone()
        }
    }
}

/// The tick or its room, the branch symbol with the name, the folder dimmed,
/// and at the right how far the branch is from its upstream, what has changed
/// and, for a merged one, the word that says it can go.
pub fn worktree_line<'a, M: Clone + 'a>(
    line: WorktreeLine<M>,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let merged = line.state == WorktreeState::Merged;
    let (ink, dim, name_ink) = if line.chosen {
        (
            palette.cursor_text,
            palette.cursor_text,
            palette.cursor_text,
        )
    } else if merged {
        (palette.dim_text, palette.dim_text, palette.dim_text)
    } else if line.state == WorktreeState::Detached {
        (palette.text, palette.dim_text, git::orange())
    } else {
        (palette.text, palette.dim_text, palette.text)
    };
    let small = |content: String, color: Color| {
        text(content)
            .size(font_size::SMALL)
            .color(color)
            .wrapping(text::Wrapping::None)
    };
    let tick = if line.state == WorktreeState::Current {
        "\u{2713}"
    } else {
        ""
    };
    let tick_ink = if line.chosen {
        palette.cursor_text
    } else {
        git::purple(palette)
    };
    let title = worktree_title(&line, lang);
    let changes_ink = if line.chosen {
        palette.cursor_text
    } else {
        git::cyan()
    };
    let content = row![
        container(text(tick).size(font_size::BODY).color(tick_ink)).width(spacing::L),
        git_branch_icon(name_ink, BRANCH_ICON),
        text(title)
            .size(font_size::BODY)
            .font(APP_FONT)
            .color(name_ink)
            .wrapping(text::Wrapping::None),
        text(format!("\u{25a2} {}", line.folder))
            .size(font_size::BODY)
            .color(dim)
            .wrapping(text::Wrapping::None),
        Space::new().width(Length::Fill),
    ]
    .extend((line.behind > 0).then(|| small(format!("\u{2193}{}", line.behind), ink).into()))
    .extend((line.ahead > 0).then(|| small(format!("\u{2191}{}", line.ahead), ink).into()))
    .extend(
        (line.changes > 0).then(|| small(format!("\u{25cf} {}", line.changes), changes_ink).into()),
    )
    .extend(merged.then(|| small(lang.text(Msg::GitViewWorktreeMerged).to_owned(), dim).into()))
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center);
    button(container(content).clip(true).center_y(Length::Fill))
        .on_press(line.on_press)
        .width(Length::Fill)
        .height(size::ROW_TALL)
        .padding([0.0, spacing::L])
        .style(theme::menu_item(palette, line.chosen))
        .into()
}

fn is_local(kind: RefChipKind) -> bool {
    matches!(kind, RefChipKind::Head | RefChipKind::Branch)
}

/// The local branch named `head` becomes the head chip.
pub fn mark_head(
    chips: Vec<(String, RefChipKind)>,
    head: Option<&str>,
) -> Vec<(String, RefChipKind)> {
    chips
        .into_iter()
        .map(|(label, kind)| {
            if kind == RefChipKind::Branch && head == Some(label.as_str()) {
                (label, RefChipKind::Head)
            } else {
                (label, kind)
            }
        })
        .collect()
}

/// A local branch and its remote on the same commit are one label,
/// "main · origin": the remote chip is dropped and its remote named after the
/// branch. A remote branch without a local one stays as it is, after the others.
pub fn summarize_refs(chips: Vec<(String, RefChipKind)>) -> Vec<(String, RefChipKind)> {
    let mut merged: Vec<(String, RefChipKind, Vec<String>)> = Vec::new();
    let mut remotes = Vec::new();
    for (label, kind) in chips {
        if kind == RefChipKind::Remote {
            remotes.push(label);
        } else {
            merged.push((label, kind, Vec::new()));
        }
    }
    let mut alone = Vec::new();
    for label in remotes {
        let owner = label.split_once('/').and_then(|(remote, branch)| {
            merged
                .iter()
                .position(|(name, kind, _)| is_local(*kind) && name == branch)
                .map(|at| (at, remote.to_owned()))
        });
        match owner {
            Some((at, remote)) => {
                if let Some(entry) = merged.get_mut(at) {
                    entry.2.push(remote);
                }
            }
            None => alone.push((label, RefChipKind::Remote)),
        }
    }
    merged
        .into_iter()
        .map(|(name, kind, joined)| {
            if joined.is_empty() {
                (name, kind)
            } else {
                (format!("{name} \u{b7} {}", joined.join(", ")), kind)
            }
        })
        .chain(alone)
        .collect()
}

/// One fact of a commit: the dim `label` in a column of [`size::FACT_LABEL`]
/// and the `content` after it.
pub fn fact_row<'a, M: 'a>(
    label: &str,
    content: Element<'a, M>,
    palette: &Palette,
) -> Element<'a, M> {
    row![
        container(
            text(label.to_owned())
                .size(font_size::SMALL)
                .color(palette.dim_text)
                .wrapping(text::Wrapping::None),
        )
        .width(size::FACT_LABEL),
        content,
    ]
    .spacing(spacing::M)
    .align_y(alignment::Vertical::Center)
    .into()
}

/// A short text in purple that acts like a link: a parent commit.
pub fn purple_link<'a, M: Clone + 'a>(
    label: String,
    on_press: M,
    palette: &Palette,
) -> Element<'a, M> {
    let ink = git::purple(palette);
    button(
        text(label)
            .size(font_size::BODY)
            .color(ink)
            .wrapping(text::Wrapping::None),
    )
    .padding(0.0)
    .on_press(on_press)
    .style(move |_theme: &iced::Theme, _status| button::Style {
        text_color: ink,
        ..button::Style::default()
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chips(items: &[(&str, RefChipKind)]) -> Vec<(String, RefChipKind)> {
        items
            .iter()
            .map(|(label, kind)| ((*label).to_owned(), *kind))
            .collect()
    }

    #[test]
    fn a_local_branch_and_its_remote_on_one_commit_are_one_label() {
        let shown = summarize_refs(chips(&[
            ("main", RefChipKind::Branch),
            ("origin/main", RefChipKind::Remote),
        ]));
        assert_eq!(shown, chips(&[("main \u{b7} origin", RefChipKind::Branch)]));
    }

    #[test]
    fn the_head_keeps_its_kind_when_it_takes_the_remote_in() {
        let marked = mark_head(
            chips(&[
                ("main", RefChipKind::Branch),
                ("origin/main", RefChipKind::Remote),
            ]),
            Some("main"),
        );
        assert_eq!(
            summarize_refs(marked),
            chips(&[("main \u{b7} origin", RefChipKind::Head)])
        );
    }

    #[test]
    fn a_remote_without_a_local_branch_stays_and_tags_are_left_alone() {
        let shown = summarize_refs(chips(&[
            ("origin/feat/x", RefChipKind::Remote),
            ("v1.7.1", RefChipKind::Tag),
            ("feat", RefChipKind::Branch),
        ]));
        assert_eq!(
            shown,
            chips(&[
                ("v1.7.1", RefChipKind::Tag),
                ("feat", RefChipKind::Branch),
                ("origin/feat/x", RefChipKind::Remote),
            ])
        );
    }

    #[test]
    fn two_remotes_of_one_branch_are_named_one_after_the_other() {
        let shown = summarize_refs(chips(&[
            ("main", RefChipKind::Branch),
            ("origin/main", RefChipKind::Remote),
            ("upstream/main", RefChipKind::Remote),
        ]));
        assert_eq!(
            shown,
            chips(&[("main \u{b7} origin, upstream", RefChipKind::Branch)])
        );
    }

    #[test]
    fn only_the_checked_out_branch_becomes_the_head() {
        let marked = mark_head(
            chips(&[("main", RefChipKind::Branch), ("dev", RefChipKind::Branch)]),
            Some("dev"),
        );
        assert_eq!(
            marked,
            chips(&[("main", RefChipKind::Branch), ("dev", RefChipKind::Head)])
        );
        assert_eq!(
            mark_head(chips(&[("main", RefChipKind::Branch)]), None),
            chips(&[("main", RefChipKind::Branch)])
        );
    }

    #[test]
    fn a_detached_worktree_names_its_commit() {
        let line = WorktreeLine {
            state: WorktreeState::Detached,
            branch: "9aeef74".to_owned(),
            folder: "ncrs-probe".to_owned(),
            ahead: 0,
            behind: 0,
            changes: 0,
            chosen: false,
            on_press: (),
        };
        assert_eq!(
            worktree_title(&line, Language::English),
            "detached at 9aeef74"
        );
    }
}
