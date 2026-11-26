//! The repository list: the column head, one row per repository or worktree,
//! and the bar that sums up what a fetch or pull did.

use iced::widget::text::Wrapping;
use iced::mouse::Interaction;
use iced::widget::{container, mouse_area, row, stack, text, Row, Space};
use iced::{alignment, Color, Element, Length};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::file_icon::git_branch_icon;
use crate::git_state::{standing_chip, standing_chip_inked, RunOutcome, Standing};
use crate::kit::{self, icon, Check, Icon, Tone};
use crate::palette::Palette;
use crate::theme::git::{self, faint};
use crate::theme::{self, font_size, size, spacing};

/// The height of the result bar.
const BANNER_HEIGHT: f32 = 36.0;
/// The width of the rule at the left of the result bar.
const BANNER_RULE: f32 = 3.0;
/// A column dragged narrower stops at the room its title needs, a wider one at the max.
pub const REPO_COLUMN_MIN: f32 = 80.0;
pub const REPO_COLUMN_MAX: f32 = 800.0;

/// The widths of the columns in pixels; the location takes what is left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RepoColumns {
    pub checkbox: f32,
    pub star: f32,
    pub name: f32,
    /// Whether the folder is shown, in what room the others leave.
    pub location: bool,
    pub branch: f32,
    pub standing: f32,
    pub changes: f32,
    pub fetched: f32,
    pub actions: f32,
}

impl RepoColumns {
    pub const STANDARD: Self = Self {
        checkbox: 36.0,
        star: 28.0,
        name: 260.0,
        location: true,
        branch: 300.0,
        standing: 110.0,
        changes: 120.0,
        fetched: 110.0,
        actions: 150.0,
    };

    pub fn of(self, column: RepoColumn) -> f32 {
        match column {
            RepoColumn::Name => self.name,
            RepoColumn::Branch => self.branch,
            RepoColumn::Standing => self.standing,
            RepoColumn::Changes => self.changes,
            RepoColumn::Fetched => self.fetched,
        }
    }

    pub fn with(self, column: RepoColumn, wanted: f32) -> Self {
        let width = wanted.round().clamp(REPO_COLUMN_MIN, REPO_COLUMN_MAX);
        match column {
            RepoColumn::Name => Self { name: width, ..self },
            RepoColumn::Branch => Self {
                branch: width,
                ..self
            },
            RepoColumn::Standing => Self {
                standing: width,
                ..self
            },
            RepoColumn::Changes => Self {
                changes: width,
                ..self
            },
            RepoColumn::Fetched => Self {
                fetched: width,
                ..self
            },
        }
    }

    /// `column` after its handle was dragged `delta` pixels to the right. The
    /// name has its handle at the right edge and grows with the pointer; the
    /// columns behind the location have it at the left edge, where the pointer
    /// moving right makes them narrower.
    pub fn dragged(self, column: RepoColumn, delta: f32) -> Self {
        let signed = if column == RepoColumn::Name {
            delta
        } else {
            -delta
        };
        self.with(column, self.of(column) + signed)
    }
}

/// The columns of the repository list whose width can be dragged; the
/// location takes the rest and has no handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoColumn {
    Name,
    Branch,
    Standing,
    Changes,
    Fetched,
}

/// The branch of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTag {
    pub name: String,
    /// The branch the remote names as its standard: shown dim, any other orange.
    pub is_default: bool,
}

/// One repository or worktree of the list, with everything it shows.
pub struct RepoRow<'a, M> {
    pub name: String,
    /// The finished text of the location, see [`path_tail`].
    pub location: String,
    pub branch: Option<BranchTag>,
    /// What [`repo_standing`] made of the commits.
    pub standing: Vec<Standing>,
    /// What [`repo_changes`] made of the files.
    pub changes: Vec<Standing>,
    pub fetched: String,
    pub favourite: bool,
    pub checked: bool,
    /// The row the keys are on.
    pub cursor: bool,
    /// A worktree of the repository above it: no checkbox, no star, indented.
    pub worktree: bool,
    /// The folder is gone or cannot be read: the row is faint.
    pub unreachable: bool,
    pub on_toggle: Option<M>,
    pub on_favourite: Option<M>,
    /// A click on a chip of the standing column.
    pub on_standing: Option<M>,
    /// The buttons of the last column; the caller shows them while the row is hovered.
    pub actions: Option<Element<'a, M>>,
}

impl<M> std::fmt::Debug for RepoRow<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoRow")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// The chips of the standing column: behind, ahead, or up to date when neither.
/// A count of zero draws nothing.
pub fn repo_standing(behind: u32, ahead: u32, upstream: &str) -> Vec<Standing> {
    let mut chips = Vec::new();
    if behind > 0 {
        chips.push(Standing::Behind {
            count: behind,
            upstream: upstream.to_owned(),
        });
    }
    if ahead > 0 {
        chips.push(Standing::Ahead { count: ahead });
    }
    if chips.is_empty() {
        chips.push(Standing::UpToDate);
    }
    chips
}

/// The chips of the changes column; a repository without any has none.
pub fn repo_changes(changes: u32, conflicts: u32) -> Vec<Standing> {
    let mut chips = Vec::new();
    if conflicts > 0 {
        chips.push(Standing::Conflicts { count: conflicts });
    }
    if changes > 0 {
        chips.push(Standing::Changes { count: changes });
    }
    chips
}

/// `path` cut at its start with `…` to at most `max_chars` characters, so the
/// folder it ends in stays readable. The separators are left as they are.
pub fn path_tail(path: &str, max_chars: usize) -> String {
    let count = path.chars().count();
    if count <= max_chars {
        return path.to_owned();
    }
    let Some(kept) = max_chars.checked_sub(1) else {
        return String::new();
    };
    let tail: String = path.chars().skip(count.saturating_sub(kept)).collect();
    format!("\u{2026}{tail}")
}

fn cell<'a, M: 'a>(width: Length, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    container(content)
        .width(width)
        .height(size::REPO_ROW)
        .padding([0.0, spacing::XS])
        .align_y(alignment::Vertical::Center)
        .clip(true)
        .into()
}

fn label<'a, M: 'a>(content: String, ink: Color) -> Element<'a, M> {
    text(content)
        .size(font_size::BODY)
        .color(ink)
        .wrapping(Wrapping::None)
        .into()
}

/// The column head. `on_toggle_all` is the click on the checkbox over the rows.
pub fn repo_header<'a, M: Clone + 'a>(
    columns: RepoColumns,
    lang: Language,
    palette: &Palette,
    all: Check,
    on_toggle_all: Option<M>,
) -> Element<'a, M> {
    repo_header_titled(columns, lang, palette, all, on_toggle_all, None, None)
}

/// [`repo_header`] with a title over the last column, which holds what the
/// caller puts in `RepoRow::actions`. With `on_resize` the edge of each
/// column that can be dragged (see [`RepoColumns::dragged`]) is a handle: a
/// press on it gives the message for that column.
pub fn repo_header_titled<'a, M: Clone + 'a>(
    columns: RepoColumns,
    lang: Language,
    palette: &Palette,
    all: Check,
    on_toggle_all: Option<M>,
    actions_title: Option<&str>,
    on_resize: Option<Box<dyn Fn(RepoColumn) -> M + 'a>>,
) -> Element<'a, M> {
    let title = |msg: Msg| label(lang.text(msg).to_owned(), palette.dim_text);
    let fixed = |width: f32, content: Element<'a, M>| cell(Length::Fixed(width), content);
    // The handle is invisible and lies over the edge, so the head looks the same with and without it.
    let draggable = |column: RepoColumn, content: Element<'a, M>| -> Element<'a, M> {
        let width = columns.of(column);
        let Some(pressed) = on_resize.as_ref().filter(|_| width > 0.0) else {
            return fixed(width, content);
        };
        let edge = if column == RepoColumn::Name {
            alignment::Horizontal::Right
        } else {
            alignment::Horizontal::Left
        };
        let handle = mouse_area(
            Space::new()
                .width(size::COLUMN_GAP)
                .height(Length::Fill),
        )
        .on_press(pressed(column))
        .interaction(Interaction::ResizingHorizontally);
        stack![
            fixed(width, content),
            container(handle)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(edge)
        ]
        .into()
    };
    container(
        row![
            fixed(columns.checkbox, kit::checkbox(all, on_toggle_all, palette)),
            fixed(columns.star, Space::new().into()),
            draggable(RepoColumn::Name, title(Msg::RepoRowColumnName)),
            cell(
                Length::Fill,
                if columns.location {
                    title(Msg::RepoRowColumnLocation)
                } else {
                    Space::new().into()
                },
            ),
            draggable(RepoColumn::Branch, title(Msg::RepoRowColumnBranch)),
            draggable(RepoColumn::Standing, title(Msg::RepoRowColumnStanding)),
            draggable(RepoColumn::Changes, title(Msg::RepoRowColumnChanges)),
            draggable(RepoColumn::Fetched, title(Msg::RepoRowColumnFetched)),
            fixed(
                columns.actions,
                actions_title.map_or_else(
                    || Space::new().into(),
                    |words| label(words.to_owned(), palette.dim_text),
                ),
            ),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(size::COLUMN_HEADER)
    .into()
}

/// One row of the list.
pub fn repo_row<'a, M: Clone + 'a>(
    item: RepoRow<'a, M>,
    columns: RepoColumns,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let fade = |ink: Color| if item.unreachable { faint(ink) } else { ink };
    let (base_text, base_dim) = if item.cursor {
        (palette.cursor_text, palette.cursor_text)
    } else {
        (palette.text, palette.dim_text)
    };
    let (text_ink, dim_ink) = (fade(base_text), fade(base_dim));
    let fixed = |width: f32, content: Element<'a, M>| cell(Length::Fixed(width), content);
    let nothing = || Space::new().into();

    let checkbox = if item.worktree {
        nothing()
    } else {
        let state = if item.checked { Check::On } else { Check::Off };
        kit::checkbox(state, item.on_toggle, palette)
    };
    let star = if item.worktree || !(item.favourite || item.cursor) {
        nothing()
    } else {
        let ink = if item.favourite {
            git::purple(palette)
        } else {
            dim_ink
        };
        iced::widget::button(icon::<M>(Icon::Star, ink))
            .padding(0.0)
            .on_press_maybe(item.on_favourite)
            .style(theme::close_button(palette))
            .into()
    };
    let name = if item.worktree {
        row![
            Space::new().width(spacing::XL),
            git_branch_icon(dim_ink, size::ICON),
            label(item.name, text_ink),
        ]
        .spacing(spacing::XS)
        .align_y(alignment::Vertical::Center)
        .into()
    } else {
        label(item.name, text_ink)
    };
    let branch = match item.branch {
        _ if item.unreachable => label(lang.text(Msg::RepoRowUnreachable).to_owned(), dim_ink),
        None => nothing(),
        Some(tag) => {
            let ink = if item.cursor {
                palette.cursor_text
            } else if tag.is_default {
                dim_ink
            } else {
                git::orange()
            };
            container(
                text(tag.name)
                    .size(font_size::SMALL)
                    .wrapping(Wrapping::None),
            )
            .padding([0.0, spacing::XS])
            .style(git::state_label(ink))
            .into()
        }
    };
    let chips = |standings: &[Standing], on_press: Option<M>| -> Element<'a, M> {
        let mut line = Row::new()
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center);
        for standing in standings {
            let chip = if item.cursor {
                standing_chip_inked(standing, lang, palette.cursor_text, on_press.clone())
            } else {
                standing_chip(standing, lang, palette, on_press.clone())
            };
            line = line.push(chip);
        }
        line.into()
    };
    let (standing, changes) = if item.unreachable {
        (nothing(), nothing())
    } else {
        (
            chips(&item.standing, item.on_standing.clone()),
            chips(&item.changes, item.on_standing),
        )
    };

    container(
        row![
            fixed(columns.checkbox, checkbox),
            fixed(columns.star, star),
            fixed(columns.name, name),
            cell(
                Length::Fill,
                if columns.location {
                    label(item.location, dim_ink)
                } else {
                    nothing()
                },
            ),
            fixed(columns.branch, branch),
            fixed(columns.standing, standing),
            fixed(columns.changes, changes),
            fixed(columns.fetched, label(item.fetched, dim_ink)),
            fixed(columns.actions, item.actions.unwrap_or_else(nothing)),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(size::REPO_ROW)
    .style(git::marked_row(palette, item.cursor, item.checked))
    .into()
}

/// How many repositories a run ended in each way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    total: u32,
    counts: Vec<(RunOutcome, u32)>,
}

impl RunSummary {
    pub fn of(outcomes: &[RunOutcome]) -> Self {
        let counts = RunOutcome::ALL
            .iter()
            .map(|wanted| {
                let count = outcomes.iter().filter(|outcome| *outcome == wanted).count();
                (*wanted, u32::try_from(count).unwrap_or(u32::MAX))
            })
            .collect();
        Self {
            total: u32::try_from(outcomes.len()).unwrap_or(u32::MAX),
            counts,
        }
    }

    pub const fn total(&self) -> u32 {
        self.total
    }

    pub fn count(&self, wanted: RunOutcome) -> u32 {
        self.counts
            .iter()
            .find(|(outcome, _)| *outcome == wanted)
            .map_or(0, |(_, count)| *count)
    }

    /// The results that happened at least once, in the order of [`RunOutcome::ALL`].
    pub fn shown(&self) -> Vec<(RunOutcome, u32)> {
        self.counts
            .iter()
            .copied()
            .filter(|(_, count)| *count > 0)
            .collect()
    }

    /// "5 repos:" in the language.
    pub fn heading(&self, lang: Language) -> String {
        let msg = if self.total == 1 {
            Msg::GitBannerReposOne
        } else {
            Msg::GitBannerReposOther
        };
        fill_template(lang.text(msg), &[("{count}", &self.total.to_string())])
    }
}

/// The bar of a run's result: how many repositories ended how, the caller's
/// buttons and the close button. It stays until it is closed.
pub fn result_banner<'a, M: Clone + 'a>(
    summary: &RunSummary,
    lang: Language,
    palette: &Palette,
    buttons: Option<Element<'a, M>>,
    on_close: M,
) -> Element<'a, M> {
    let mut line = Row::new()
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center)
        .push(label(summary.heading(lang), palette.text));
    for (outcome, count) in summary.shown() {
        let ink = outcome.color(palette);
        line = line.push(
            row![
                icon::<M>(outcome.icon(), ink),
                label(format!("{count} {}", outcome.label(lang)), ink),
            ]
            .spacing(spacing::XS)
            .align_y(alignment::Vertical::Center),
        );
    }
    line = line.push(Space::new().width(Length::Fill));
    if let Some(extra) = buttons {
        line = line.push(extra);
    }
    line = line.push(kit::button(
        lang.text(Msg::DialogClose),
        Some(on_close),
        Tone::Secondary,
        palette,
    ));
    banner_frame(line, palette)
}

/// The bar of a result in the words of the caller: `heading`, then each
/// result as its sign and text in the ink of `outcome`. No buttons, no close.
pub fn result_strip<'a, M: Clone + 'a>(
    heading: String,
    results: Vec<(RunOutcome, String)>,
    palette: &Palette,
) -> Element<'a, M> {
    let mut line = Row::new()
        .spacing(spacing::L)
        .align_y(alignment::Vertical::Center)
        .push(label(heading, palette.text));
    for (outcome, words) in results {
        let ink = outcome.color(palette);
        line = line.push(
            row![icon::<M>(outcome.icon(), ink), label(words, ink)]
                .spacing(spacing::XS)
                .align_y(alignment::Vertical::Center),
        );
    }
    banner_frame(line, palette)
}

fn banner_frame<'a, M: Clone + 'a>(line: Row<'a, M>, palette: &Palette) -> Element<'a, M> {
    container(
        row![
            container(Space::new())
                .width(BANNER_RULE)
                .height(Length::Fill)
                .style(git::banner_rule(palette)),
            container(line)
                .padding([0.0, spacing::L])
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(alignment::Vertical::Center),
        ]
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(BANNER_HEIGHT)
    .style(git::result_banner(palette))
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_chips_only_exist_for_a_count_above_zero() {
        assert_eq!(repo_standing(0, 0, "origin/main"), vec![Standing::UpToDate]);
        assert_eq!(
            repo_standing(0, 2, "origin/main"),
            vec![Standing::Ahead { count: 2 }]
        );
        let both = repo_standing(3, 1, "origin/main");
        assert_eq!(both.len(), 2);
        assert!(!both.contains(&Standing::UpToDate));
    }

    #[test]
    fn a_clean_repository_has_no_change_chip() {
        assert!(repo_changes(0, 0).is_empty());
        assert_eq!(repo_changes(2, 0), vec![Standing::Changes { count: 2 }]);
        assert_eq!(repo_changes(0, 1), vec![Standing::Conflicts { count: 1 }]);
        assert_eq!(repo_changes(3, 1).len(), 2);
    }

    #[test]
    fn the_summary_counts_each_result_and_hides_the_empty_ones() {
        use RunOutcome::{AlreadyCurrent, Failed, Skipped, Updated};
        let summary = RunSummary::of(&[Updated, Updated, AlreadyCurrent, Skipped, Failed]);
        assert_eq!(summary.total(), 5);
        assert_eq!(summary.count(Updated), 2);
        assert_eq!(summary.count(RunOutcome::Running), 0);
        assert_eq!(
            summary.shown(),
            vec![(Updated, 2), (AlreadyCurrent, 1), (Skipped, 1), (Failed, 1)]
        );
        assert_eq!(summary.heading(Language::English), "5 repos:");
        assert_eq!(
            RunSummary::of(&[Updated]).heading(Language::German),
            "1 Repo:"
        );
    }

    #[test]
    fn a_path_is_cut_at_its_start() {
        assert_eq!(path_tail("~/code/ncrs", 20), "~/code/ncrs");
        assert_eq!(path_tail("~/code/cosmic/ncrs", 10), "\u{2026}smic/ncrs");
        assert_eq!(path_tail("C:\\code\\ncrs", 8), "\u{2026}de\\ncrs");
        assert_eq!(path_tail("abc", 0), "");
        assert_eq!(path_tail("abcd", 1), "\u{2026}");
    }

    #[test]
    fn the_columns_add_up_to_the_design() {
        let columns = RepoColumns::STANDARD;
        let fixed = columns.checkbox
            + columns.star
            + columns.name
            + columns.branch
            + columns.standing
            + columns.changes
            + columns.fetched
            + columns.actions;
        assert_eq!(fixed, 1114.0);
    }
}
