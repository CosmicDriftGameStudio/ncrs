//! What a branch or a repository stands at, and what a run did to it, as plain
//! data with the words, the colors and the marks they are drawn with. The
//! chip of a state is a button; the mark of a result is a sign and a word.

use iced::widget::{button, row, text};
use iced::{alignment, Color, Element};
use ncrs_i18n::{fill_template, Language, Msg};

use crate::kit::{icon, Icon};
use crate::palette::Palette;
use crate::theme::git;
use crate::theme::{font_size, spacing};

/// Where a branch stands against its upstream and its working copy. Only a
/// state that has a value is drawn: no "0 ahead".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// Commits the upstream has and the branch has not.
    Behind {
        count: u32,
        upstream: String,
    },
    /// Commits the branch has and the upstream has not.
    Ahead {
        count: u32,
    },
    /// Files with changes that are not committed.
    Changes {
        count: u32,
    },
    Conflicts {
        count: u32,
    },
    UpToDate,
}

impl Standing {
    /// The words, one of the singular and plural texts for a count.
    pub const fn message(&self) -> Msg {
        match self {
            Self::Behind { .. } => Msg::GitStateBehind,
            Self::Ahead { .. } => Msg::GitStateAhead,
            Self::Changes { count: 1 } => Msg::GitStateChangesOne,
            Self::Changes { .. } => Msg::GitStateChangesOther,
            Self::Conflicts { count: 1 } => Msg::GitStateConflictsOne,
            Self::Conflicts { .. } => Msg::GitStateConflictsOther,
            Self::UpToDate => Msg::GitStateUpToDate,
        }
    }

    /// The sign in front of the words; the text font has all of them.
    pub const fn glyph(&self) -> &'static str {
        match self {
            Self::Behind { .. } => "\u{2193}",
            Self::Ahead { .. } => "\u{2191}",
            Self::Changes { .. } => "\u{25cf}",
            Self::Conflicts { .. } => "!",
            Self::UpToDate => "\u{2713}",
        }
    }

    pub fn color(&self, palette: &Palette) -> Color {
        match self {
            Self::Behind { .. } => git::orange(),
            Self::Ahead { .. } => git::purple(palette),
            Self::Changes { .. } => git::cyan(),
            Self::Conflicts { .. } => palette.error,
            Self::UpToDate => palette.dim_text,
        }
    }

    /// The chip's text: the sign, the count where the sign counts, the words.
    pub fn text(&self, lang: Language) -> String {
        let words = lang.text(self.message());
        let glyph = self.glyph();
        match self {
            Self::Behind { count, upstream } => {
                format!(
                    "{glyph}{count} {}",
                    fill_template(words, &[("{upstream}", upstream)])
                )
            }
            Self::Ahead { count } => format!("{glyph}{count} {words}"),
            Self::Changes { count } | Self::Conflicts { count } => {
                format!(
                    "{glyph} {}",
                    fill_template(words, &[("{count}", &count.to_string())])
                )
            }
            Self::UpToDate => format!("{glyph} {words}"),
        }
    }
}

/// The chip of a state. A click sends `on_press`; without one it is a label.
pub fn standing_chip<'a, M: Clone + 'a>(
    standing: &Standing,
    lang: Language,
    palette: &Palette,
    on_press: Option<M>,
) -> Element<'a, M> {
    let ink = standing.color(palette);
    let label = text(standing.text(lang))
        .size(font_size::SMALL)
        .wrapping(iced::widget::text::Wrapping::None);
    match on_press {
        Some(message) => button(label)
            .padding([0.0, spacing::XS])
            .style(git::state_chip(ink))
            .on_press(message)
            .into(),
        None => iced::widget::container(label)
            .padding([0.0, spacing::XS])
            .style(git::state_label(ink))
            .into(),
    }
}

/// What a fetch or pull did to one repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Running,
    Updated,
    AlreadyCurrent,
    Skipped,
    Failed,
}

impl RunOutcome {
    pub const ALL: [Self; 5] = [
        Self::Running,
        Self::Updated,
        Self::AlreadyCurrent,
        Self::Skipped,
        Self::Failed,
    ];

    pub const fn message(self) -> Msg {
        match self {
            Self::Running => Msg::GitOutcomeRunning,
            Self::Updated => Msg::GitOutcomeUpdated,
            Self::AlreadyCurrent => Msg::GitOutcomeAlreadyCurrent,
            Self::Skipped => Msg::GitOutcomeSkipped,
            Self::Failed => Msg::GitOutcomeFailed,
        }
    }

    /// An icon, not a text glyph: the pause and the cross come as color emoji
    /// on a Mac, which ignore the ink.
    pub const fn icon(self) -> Icon {
        match self {
            Self::Running => Icon::Loader,
            Self::Updated | Self::AlreadyCurrent => Icon::CircleCheck,
            Self::Skipped => Icon::CirclePause,
            Self::Failed => Icon::CircleX,
        }
    }

    pub fn color(self, palette: &Palette) -> Color {
        match self {
            Self::Running => git::cyan(),
            Self::Updated => palette.success,
            Self::AlreadyCurrent => palette.dim_text,
            Self::Skipped => git::orange(),
            Self::Failed => palette.error,
        }
    }

    pub fn label(self, lang: Language) -> &'static str {
        lang.text(self.message())
    }
}

/// The sign and the word of a result; never the sign alone.
pub fn outcome_mark<'a, M: 'a>(
    outcome: RunOutcome,
    lang: Language,
    palette: &Palette,
) -> Element<'a, M> {
    let ink = outcome.color(palette);
    row![
        icon(outcome.icon(), ink),
        text(outcome.label(lang)).size(font_size::SMALL).color(ink),
    ]
    .spacing(spacing::XS)
    .align_y(alignment::Vertical::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::git::purple;
    use ncrs_core::palette::ThemePreset;

    const LANGUAGES: [Language; 2] = [Language::English, Language::German];

    fn every_standing() -> [Standing; 7] {
        [
            Standing::Behind {
                count: 2,
                upstream: "origin/main".to_owned(),
            },
            Standing::Ahead { count: 3 },
            Standing::Changes { count: 1 },
            Standing::Changes { count: 4 },
            Standing::Conflicts { count: 1 },
            Standing::Conflicts { count: 2 },
            Standing::UpToDate,
        ]
    }

    #[test]
    fn every_standing_has_a_sign_and_a_word_in_both_languages() {
        for standing in every_standing() {
            for lang in LANGUAGES {
                let words = lang.text(standing.message());
                assert!(!words.is_empty(), "{standing:?}");
                let shown = standing.text(lang);
                assert!(shown.starts_with(standing.glyph()), "{shown}");
                assert!(!shown.contains('{'), "{shown}");
            }
        }
    }

    #[test]
    fn a_count_of_one_takes_the_singular() {
        assert_eq!(
            Standing::Changes { count: 1 }.text(Language::English),
            "\u{25cf} 1 local change"
        );
        assert_eq!(
            Standing::Changes { count: 2 }.text(Language::English),
            "\u{25cf} 2 local changes"
        );
        assert_eq!(
            Standing::Conflicts { count: 1 }.text(Language::English),
            "! 1 conflict"
        );
    }

    #[test]
    fn behind_names_the_upstream_and_ahead_counts() {
        let behind = Standing::Behind {
            count: 2,
            upstream: "origin/main".to_owned(),
        };
        assert_eq!(
            behind.text(Language::English),
            "\u{2193}2 behind origin/main"
        );
        assert_eq!(
            Standing::Ahead { count: 5 }.text(Language::German),
            "\u{2191}5 voraus"
        );
    }

    #[test]
    fn every_result_has_a_word_and_an_icon_of_its_own() {
        for lang in LANGUAGES {
            let words: Vec<&str> = RunOutcome::ALL
                .iter()
                .map(|outcome| outcome.label(lang))
                .collect();
            assert!(words.iter().all(|word| !word.is_empty()));
            for (place, word) in words.iter().enumerate() {
                assert!(!words[place + 1..].contains(word), "{word}");
            }
        }
        let palette = ThemePreset::Cosmic.palette();
        assert_ne!(
            RunOutcome::Updated.color(&palette),
            RunOutcome::AlreadyCurrent.color(&palette)
        );
        assert_ne!(RunOutcome::Skipped.icon(), RunOutcome::Failed.icon());
    }

    #[test]
    fn ahead_is_purple() {
        let palette = ThemePreset::Cosmic.palette();
        assert_eq!(
            Standing::Ahead { count: 1 }.color(&palette),
            purple(&palette)
        );
    }
}
