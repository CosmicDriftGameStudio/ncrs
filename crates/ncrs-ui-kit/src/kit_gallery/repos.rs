//! The repository list pictures: rows with worktrees, favourites, unreachable
//! folders and every standing, the marks of a result and the result bar.

use iced::widget::{column, container, Column, Space};
use iced::{Element, Length, Size};
use iced_test::Simulator;
use ncrs_i18n::Language;

use super::matches_picture;
use crate::git_state::{outcome_mark, RunOutcome};
use crate::kit::{self, Check, Tone};
use crate::palette::{Palette, ThemePreset};
use crate::repo_row::{
    path_tail, repo_changes, repo_header, repo_row, repo_standing, result_banner, BranchTag,
    RepoColumns, RepoRow, RunSummary,
};
use crate::snapshot_support::ReferenceSnapshot as _;
use crate::theme::{self, spacing};

type Cell<'a> = Element<'a, ()>;

const LIST: Size = Size::new(1740.0, 940.0);
const MARKS: Size = Size::new(520.0, 150.0);
const BANNERS: Size = Size::new(1200.0, 120.0);
const LOCATION_CHARS: usize = 44;

/// What a row of the table says; the rest follows from it.
struct Sample {
    name: &'static str,
    path: &'static str,
    branch: &'static str,
    behind: u32,
    ahead: u32,
    changes: u32,
    conflicts: u32,
    fetched: &'static str,
    favourite: bool,
    worktree: bool,
    unreachable: bool,
}

const fn sample(name: &'static str, path: &'static str, branch: &'static str) -> Sample {
    Sample {
        name,
        path,
        branch,
        behind: 0,
        ahead: 0,
        changes: 0,
        conflicts: 0,
        fetched: "today 13:02",
        favourite: false,
        worktree: false,
        unreachable: false,
    }
}

fn samples() -> Vec<Sample> {
    vec![
        Sample {
            favourite: true,
            ahead: 2,
            changes: 3,
            ..sample("ncrs", "~/code/ncrs", "feat/1-8-0")
        },
        Sample {
            worktree: true,
            changes: 1,
            ..sample("1-8-0", "~/code/ncrs/.wt/1-8-0", "feat/1-8-0")
        },
        Sample {
            worktree: true,
            ..sample("hermes", "~/code/ncrs/.wt/hermes", "main")
        },
        Sample {
            favourite: true,
            behind: 4,
            ..sample("kumiko", "~/code/cosmicdriftgamestudio/kumiko", "main")
        },
        Sample {
            worktree: true,
            behind: 4,
            ..sample(
                "kumiko-366",
                "~/code/cosmicdriftgamestudio/kumiko/.wt/366",
                "feat/366",
            )
        },
        Sample {
            favourite: true,
            ..sample("solon", "~/code/cosmicdriftgamestudio/solon", "main")
        },
        Sample {
            conflicts: 2,
            changes: 5,
            ..sample("infra", "~/code/cosmicdriftgamestudio/infra", "fix/etcd")
        },
        sample("ncrs-web", "~/code/ncrs-web", "main"),
        Sample {
            behind: 1,
            ..sample(
                "agent-config",
                "~/code/cosmicdriftgamestudio/agent-config",
                "main",
            )
        },
        Sample {
            ahead: 1,
            ..sample("space-conquest", "~/code/space-conquest", "master")
        },
        Sample {
            changes: 12,
            fetched: "yesterday",
            ..sample("dotfiles", "~/code/dotfiles", "main")
        },
        Sample {
            fetched: "3 days ago",
            ..sample(
                "glasscube",
                "~/code/cosmicdriftgamestudio/glasscube",
                "main",
            )
        },
        Sample {
            behind: 9,
            ahead: 3,
            ..sample("iced-fork", "~/code/forks/iced", "ncrs-patches")
        },
        sample("tiny-skia", "~/code/forks/tiny-skia", "master"),
        Sample {
            fetched: "never",
            ..sample("scratch", "~/code/scratch", "main")
        },
        Sample {
            unreachable: true,
            fetched: "last week",
            ..sample("old-prototype", "~/code/old/prototype", "main")
        },
        Sample {
            unreachable: true,
            fetched: "last month",
            ..sample("usb-backup", "/Volumes/Backup/code/archive", "main")
        },
        sample("astro-site", "~/code/astro-site", "main"),
        Sample {
            changes: 1,
            ..sample("blog", "~/code/blog", "draft/winter")
        },
        sample("pdfium-build", "~/code/pdfium-build", "main"),
        Sample {
            behind: 2,
            ..sample("homebrew-tap", "~/code/homebrew-tap", "main")
        },
        sample(
            "winget-pkgs",
            "C:\\Users\\marc\\code\\winget-pkgs",
            "master",
        ),
        Sample {
            ahead: 4,
            ..sample("aur-ncrs", "~/code/aur-ncrs", "master")
        },
        sample("notes", "~/Documents/notes", "main"),
        Sample {
            changes: 2,
            ..sample("trailer", "~/code/ncrs/doku/trailer", "main")
        },
        sample(
            "kumiko-studio",
            "~/code/cosmicdriftgamestudio/kumiko-studio",
            "main",
        ),
        Sample {
            worktree: true,
            ..sample(
                "plans",
                "~/code/cosmicdriftgamestudio/kumiko-studio/.wt/plans",
                "docs/plans",
            )
        },
        Sample {
            behind: 1,
            ahead: 1,
            ..sample("fw", "~/code/cosmicdriftgamestudio/fw", "main")
        },
        sample(
            "ci-images",
            "~/code/cosmicdriftgamestudio/ci-images",
            "main",
        ),
        sample("sandbox", "~/code/sandbox", "experiment"),
        sample("learn-rust", "~/code/learn/rust", "main"),
        Sample {
            changes: 7,
            ..sample("shaders", "~/code/shaders", "wip")
        },
        sample("website-old", "~/code/website-old", "gh-pages"),
    ]
}

fn list<'a>(lang: Language, palette: &Palette) -> Cell<'a> {
    let columns = RepoColumns::STANDARD;
    let mut rows = Column::new();
    for (place, item) in samples().into_iter().enumerate() {
        let is_default = matches!(item.branch, "main" | "master");
        rows = rows.push(repo_row(
            RepoRow {
                name: item.name.to_owned(),
                location: path_tail(item.path, LOCATION_CHARS),
                branch: Some(BranchTag {
                    name: item.branch.to_owned(),
                    is_default,
                }),
                standing: repo_standing(item.behind, item.ahead, "origin/main"),
                changes: repo_changes(item.changes, item.conflicts),
                fetched: item.fetched.to_owned(),
                favourite: item.favourite,
                checked: matches!(place, 3 | 4),
                cursor: place == 5,
                worktree: item.worktree,
                unreachable: item.unreachable,
                on_toggle: None,
                on_favourite: None,
                on_standing: None,
                actions: (place == 6).then(|| {
                    kit::button(
                        lang.text(ncrs_i18n::Msg::DialogClose),
                        None,
                        Tone::Secondary,
                        palette,
                    )
                }),
            },
            columns,
            lang,
            palette,
        ));
    }
    column![
        repo_header(columns, lang, palette, Check::Mixed, None),
        rows
    ]
    .into()
}

fn marks<'a>(lang: Language, palette: &Palette) -> Cell<'a> {
    let mut marks = Column::new().spacing(spacing::M);
    for outcome in RunOutcome::ALL {
        marks = marks.push(outcome_mark(outcome, lang, palette));
    }
    marks.into()
}

fn banners<'a>(lang: Language, palette: &Palette) -> Cell<'a> {
    use RunOutcome::{AlreadyCurrent, Failed, Skipped, Updated};
    let several = RunSummary::of(&[Updated, Updated, AlreadyCurrent, Skipped, Failed]);
    let one = RunSummary::of(&[Updated]);
    column![
        result_banner(
            &several,
            lang,
            palette,
            Some(kit::button(
                "Show errors",
                Some(()),
                Tone::Secondary,
                palette
            )),
            ()
        ),
        result_banner(&one, lang, palette, None, ()),
    ]
    .spacing(spacing::M)
    .into()
}

fn padded(content: Cell<'_>) -> Cell<'_> {
    container(content).padding(spacing::M).into()
}

fn framed<'a>(content: Cell<'a>, palette: &Palette) -> Cell<'a> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(spacing::M)
        .style(theme::panel(palette))
        .into()
}

fn matches(name: &str, size: Size, content: Cell<'_>) {
    let matches = Simulator::with_size(
        iced::Settings {
            default_font: crate::APP_FONT,
            ..iced::Settings::default()
        },
        size,
        content,
    )
    .snapshot(&iced::Theme::Dark)
    .unwrap()
    .matches_reference_image(format!("tests/snapshots/{name}.png"))
    .unwrap();
    assert!(matches, "{name} differs from its image");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dracula() -> Palette {
        ThemePreset::Dracula.palette()
    }

    #[test]
    fn the_repository_list_looks_the_same() {
        let palette = dracula();
        super::matches_picture("repo-row", LIST, padded(list(Language::English, &palette)));
    }

    #[test]
    fn the_marks_of_a_result_look_the_same() {
        let palette = dracula();
        super::matches_picture(
            "outcome-marks",
            MARKS,
            padded(marks(Language::English, &palette)),
        );
    }

    #[test]
    fn the_result_bar_looks_the_same() {
        let palette = dracula();
        super::matches_picture(
            "result-banner",
            BANNERS,
            padded(banners(Language::English, &palette)),
        );
    }

    #[test]
    fn the_repository_pieces_in_classic() {
        let palette = ThemePreset::Classic.palette();
        let all: Cell<'_> = iced::widget::column![
            marks(Language::English, &palette),
            Space::new().height(spacing::M),
            banners(Language::English, &palette),
            Space::new().height(spacing::M),
            list(Language::English, &palette),
        ]
        .into();
        matches(
            "git-kit-repos-classic",
            Size::new(1740.0, 1200.0),
            framed(all, &palette),
        );
    }
}
