//! The command list, the Lab (Cmd/Ctrl+Shift+P): every command that can be run
//! by name in the current area of the window, found by the start of the words
//! of its plain label or its theme name, and grouped. No widgets: the view
//! reads the rows, the typed text goes to [`CommandPalette::set_query`].

use std::cmp::Ordering;
use std::ops::Range;

use crate::commands::{self, CommandGroup};
use crate::i18n::{Language, Msg};
use crate::keymap::{KeyAction, KeyScope, Keymap};
use crate::messages::SettingsFeature;
use crate::ui::settings::SettingsTarget;

/// Rows shown at once, headers included, in the smallest window; the list
/// scrolls past that.
pub const MIN_VISIBLE_ROWS: usize = 12;

/// Commands listed under "Recent" while the field is empty.
pub const RECENT_LIMIT: usize = 5;

/// What running an entry does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Command(KeyAction),
    /// Opens the settings tab on a page, with the row in view when there is one.
    Setting {
        feature: SettingsFeature,
        row: Option<String>,
    },
}

/// The groups of the list. Not [`CommandGroup`] alone: that one also drives the
/// menu bar, which has no place for settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PaletteGroup {
    Command(CommandGroup),
    /// After every command group, so a command wins a tie.
    Settings,
}

impl PaletteGroup {
    fn label(self) -> Msg {
        match self {
            Self::Command(group) => group.label(),
            Self::Settings => Msg::ActionSettings,
        }
    }

    fn theme_name(self) -> Option<Msg> {
        match self {
            Self::Command(group) => group.theme_name(),
            Self::Settings => None,
        }
    }

    fn beta_mark(self) -> Option<Msg> {
        match self {
            Self::Command(CommandGroup::Hermes) => Some(Msg::HermesBeta),
            Self::Command(_) | Self::Settings => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub target: Target,
    pub group: PaletteGroup,
    pub label: String,
    pub theme_name: Option<String>,
    /// The first key bound to the action, spelled for this platform.
    pub shortcut: Option<String>,
}

impl Entry {
    pub fn command(&self) -> Option<KeyAction> {
        match self.target {
            Target::Command(action) => Some(action),
            Target::Setting { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub label: &'static str,
    pub theme_name: Option<&'static str>,
    pub beta_mark: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Position in the list of entries.
    pub entry: usize,
    /// Byte ranges into the label: the start of each matched word.
    pub label_highlight: Vec<Range<usize>>,
    /// Byte ranges into the theme name.
    pub theme_highlight: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A heading; the cursor skips it.
    Header(Header),
    Hit(Hit),
}

/// How well a query fits a command, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// The label (or theme name) starts with the whole query.
    Start,
    /// Every query word begins a word, in the order they were typed.
    InOrder,
    AnyOrder,
    /// Only the config name matches, which finds a command whose label was
    /// renamed.
    ConfigName,
}

#[derive(Debug)]
pub struct CommandPalette {
    lang: Language,
    query: String,
    entries: Vec<Entry>,
    /// Most recent first.
    recent: Vec<KeyAction>,
    rows: Vec<Row>,
    /// Position in `rows` of the highlighted hit.
    cursor: usize,
    /// The first row in view.
    top: usize,
    /// Rows in view; follows the size of the window.
    visible_rows: usize,
}

impl CommandPalette {
    /// The commands that work in `scope`, labelled in `lang`, with the keys of
    /// `keymap`. `recent` are the commands run last, most recent first.
    pub fn new(scope: KeyScope, lang: Language, keymap: &Keymap, recent: &[KeyAction]) -> Self {
        let mut palette = Self {
            lang,
            query: String::new(),
            entries: entries(scope, lang, keymap),
            recent: recent.to_vec(),
            rows: Vec::new(),
            cursor: 0,
            top: 0,
            visible_rows: MIN_VISIBLE_ROWS,
        };
        palette.rank();
        palette
    }

    /// Drops the commands that act on a file in the Hermes column, which is
    /// not up.
    pub fn without_hermes_file_commands(mut self) -> Self {
        self.entries
            .retain(|entry| !entry.command().is_some_and(KeyAction::acts_on_hermes_file));
        self.rank();
        self
    }

    /// Drops the Hermes commands on a file that need configured locations,
    /// keeping those that set a folder's role.
    pub fn without_hermes_location_commands(mut self) -> Self {
        self.entries.retain(|entry| {
            !entry
                .command()
                .is_some_and(|action| action.acts_on_hermes_file() && !action.sets_hermes_role())
        });
        self.rank();
        self
    }

    /// Drops one command that cannot run for the row under the cursor.
    pub fn without_command(mut self, action: KeyAction) -> Self {
        self.entries.retain(|entry| entry.command() != Some(action));
        self.rank();
        self
    }

    /// Adds the settings pages and settings, found by a query only.
    pub fn with_settings(mut self, targets: Vec<SettingsTarget>) -> Self {
        self.entries.extend(targets.into_iter().map(|target| Entry {
            target: Target::Setting {
                feature: target.feature,
                row: target.row,
            },
            group: PaletteGroup::Settings,
            label: target.label,
            theme_name: target.theme_name,
            shortcut: None,
        }));
        self.rank();
        self
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn hits(&self) -> impl Iterator<Item = &Hit> {
        self.rows.iter().filter_map(|row| match row {
            Row::Hit(hit) => Some(hit),
            Row::Header(_) => None,
        })
    }

    pub fn entry(&self, hit: &Hit) -> Option<&Entry> {
        self.entries.get(hit.entry)
    }

    /// The position in [`Self::rows`] of the highlighted hit.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The rows in view, by position in [`Self::rows`].
    pub fn visible(&self) -> Range<usize> {
        self.top..(self.top + self.visible_rows).min(self.rows.len())
    }

    pub fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows.max(1);
        self.keep_cursor_in_view();
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.rank();
    }

    /// Moves over hits, going round at either end; a header is not a place to
    /// stop.
    pub fn move_cursor(&mut self, delta: isize) {
        let places: Vec<usize> = self.hit_positions().collect();
        let Ok(count) = isize::try_from(places.len()) else {
            return;
        };
        if count == 0 {
            return;
        }
        let current = places
            .iter()
            .position(|&place| place == self.cursor)
            .and_then(|position| isize::try_from(position).ok())
            .unwrap_or(0);
        let target = current
            .wrapping_add(delta.rem_euclid(count))
            .rem_euclid(count);
        if let Some(&place) = usize::try_from(target)
            .ok()
            .and_then(|index| places.get(index))
        {
            self.cursor = place;
            self.keep_cursor_in_view();
        }
    }

    /// Moves the cursor to the row at `index`, if it is a hit.
    pub fn select(&mut self, index: usize) {
        if matches!(self.rows.get(index), Some(Row::Hit(_))) {
            self.cursor = index;
            self.keep_cursor_in_view();
        }
    }

    pub fn selected_target(&self) -> Option<Target> {
        let Some(Row::Hit(hit)) = self.rows.get(self.cursor) else {
            return None;
        };
        self.entry(hit).map(|entry| entry.target.clone())
    }

    fn hit_positions(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(place, row)| matches!(row, Row::Hit(_)).then_some(place))
    }

    /// Keeps the heading of a group in view with its first hit.
    fn keep_cursor_in_view(&mut self) {
        let under_header = self
            .cursor
            .checked_sub(1)
            .and_then(|above| self.rows.get(above))
            .is_some_and(|row| matches!(row, Row::Header(_)));
        let wanted_top = self.cursor - usize::from(under_header);
        if wanted_top < self.top {
            self.top = wanted_top;
        } else if self.cursor >= self.top + self.visible_rows {
            self.top = self.cursor + 1 - self.visible_rows;
        }
    }

    fn rank(&mut self) {
        let words = query_words(&self.query);
        self.rows = if words.is_empty() {
            self.overview()
        } else {
            self.matches(&words)
        };
        let first = self.hit_positions().next().unwrap_or(0);
        self.cursor = first;
        self.top = 0;
    }

    /// The recent commands, then every command by group.
    fn overview(&self) -> Vec<Row> {
        let recent: Vec<Row> = self
            .recent
            .iter()
            .filter_map(|&action| {
                self.entries
                    .iter()
                    .position(|entry| entry.command() == Some(action))
            })
            .take(RECENT_LIMIT)
            .map(|entry| Row::Hit(plain_hit(entry)))
            .collect();
        let mut rows = Vec::new();
        if !recent.is_empty() {
            rows.push(Row::Header(Header {
                label: self.lang.text(Msg::CommandPaletteRecent),
                theme_name: None,
                beta_mark: None,
            }));
            rows.extend(recent);
        }
        let order: Vec<usize> = CommandGroup::ALL
            .iter()
            .map(|&group| PaletteGroup::Command(group))
            .flat_map(|group| {
                let mut ids: Vec<usize> = (0..self.entries.len())
                    .filter(|&entry| self.entries.get(entry).is_some_and(|e| e.group == group))
                    .collect();
                ids.sort_by(|&first, &second| self.by_group_and_label(first, second));
                ids
            })
            .collect();
        self.push_grouped(
            &mut rows,
            order.into_iter().map(|entry| (entry, plain_hit(entry))),
        );
        rows
    }

    /// The commands the query finds, best tier first.
    fn matches(&self, words: &[Vec<char>]) -> Vec<Row> {
        let mut found: Vec<(Tier, Hit)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(entry, candidate)| {
                let found = match_words(candidate, words, &self.query)?;
                Some((
                    found.tier,
                    Hit {
                        entry,
                        label_highlight: found.label_highlight,
                        theme_highlight: found.theme_highlight,
                    },
                ))
            })
            .collect();
        // A group is listed once: the groups with the best hit first, then in
        // their fixed order; inside a group by tier, then by label.
        let group_of = |entry: usize| self.entries.get(entry).map(|candidate| candidate.group);
        let best_tier_of = |group: Option<PaletteGroup>| {
            found
                .iter()
                .filter(|(_, hit)| group_of(hit.entry) == group)
                .map(|(tier, _)| *tier)
                .min()
        };
        let mut best: Vec<(Option<PaletteGroup>, Option<Tier>)> = Vec::new();
        for (_, hit) in &found {
            let group = group_of(hit.entry);
            if !best.iter().any(|(seen, _)| *seen == group) {
                best.push((group, best_tier_of(group)));
            }
        }
        let rank_of = |entry: usize| {
            let group = group_of(entry);
            best.iter()
                .find(|(seen, _)| *seen == group)
                .and_then(|(_, tier)| *tier)
        };
        found.sort_by(|(first_tier, first), (second_tier, second)| {
            rank_of(first.entry)
                .cmp(&rank_of(second.entry))
                .then_with(|| group_of(first.entry).cmp(&group_of(second.entry)))
                .then_with(|| first_tier.cmp(second_tier))
                .then_with(|| self.by_group_and_label(first.entry, second.entry))
        });
        let mut rows = Vec::new();
        self.push_grouped(
            &mut rows,
            found.into_iter().map(|(_, hit)| (hit.entry, hit)),
        );
        rows
    }

    /// Appends the hits, with a heading wherever the group changes.
    fn push_grouped(&self, rows: &mut Vec<Row>, hits: impl Iterator<Item = (usize, Hit)>) {
        let mut group = None;
        for (entry, hit) in hits {
            let Some(candidate) = self.entries.get(entry) else {
                continue;
            };
            if group != Some(candidate.group) {
                group = Some(candidate.group);
                rows.push(Row::Header(Header {
                    label: self.lang.text(candidate.group.label()),
                    theme_name: candidate
                        .group
                        .theme_name()
                        .map(|name| self.lang.text(name)),
                    beta_mark: candidate.group.beta_mark().map(|name| self.lang.text(name)),
                }));
            }
            rows.push(Row::Hit(hit));
        }
    }

    /// Group order, then the label; the position in the list settles the rest.
    fn by_group_and_label(&self, first: usize, second: usize) -> Ordering {
        let (Some(a), Some(b)) = (self.entries.get(first), self.entries.get(second)) else {
            return Ordering::Equal;
        };
        a.group
            .cmp(&b.group)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| first.cmp(&second))
    }
}

fn plain_hit(entry: usize) -> Hit {
    Hit {
        entry,
        label_highlight: Vec::new(),
        theme_highlight: Vec::new(),
    }
}

/// A character as the query is compared: lowercase, and an umlaut as its base
/// letter, so "losch" finds "Löschen".
fn fold(character: char) -> char {
    let lower = character.to_lowercase().next().unwrap_or(character);
    match lower {
        'ä' => 'a',
        'ö' => 'o',
        'ü' => 'u',
        other => other,
    }
}

/// The words of the query, folded. Words end at whitespace and punctuation.
fn query_words(query: &str) -> Vec<Vec<char>> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.chars().map(fold).collect())
        .collect()
}

#[derive(Debug, Clone)]
struct Word {
    range: Range<usize>,
    in_theme: bool,
}

fn words_of(text: &str, in_theme: bool, into: &mut Vec<Word>) {
    let mut start = None;
    for (at, character) in text.char_indices() {
        match (character.is_alphanumeric(), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                into.push(Word {
                    range: from..at,
                    in_theme,
                });
                start = None;
            }
            (true, Some(_)) | (false, None) => {}
        }
    }
    if let Some(from) = start {
        into.push(Word {
            range: from..text.len(),
            in_theme,
        });
    }
}

/// How many bytes of `word` the query word covers, when it begins the word.
fn prefix_end(word: &str, query_word: &[char]) -> Option<usize> {
    let mut characters = word.char_indices();
    let mut end = 0;
    for &wanted in query_word {
        let (at, character) = characters.next()?;
        if fold(character) != wanted {
            return None;
        }
        end = at + character.len_utf8();
    }
    Some(end)
}

fn starts_with_folded(text: &str, query: &[char]) -> bool {
    let mut characters = text.chars().map(fold);
    query
        .iter()
        .all(|&wanted| characters.next() == Some(wanted))
}

/// Which words each query word begins, and how many bytes of them.
type Assignment = Vec<(usize, usize)>;

fn word_text<'a>(entry: &'a Entry, word: &Word) -> &'a str {
    let text = if word.in_theme {
        entry.theme_name.as_deref().unwrap_or_default()
    } else {
        &entry.label
    };
    text.get(word.range.clone()).unwrap_or_default()
}

/// Each query word on a later word than the one before.
fn assign_in_order(entry: &Entry, words: &[Word], query: &[Vec<char>]) -> Option<Assignment> {
    let mut assignment = Assignment::new();
    let mut from = 0;
    for query_word in query {
        let (at, end) =
            words.iter().enumerate().skip(from).find_map(|(at, word)| {
                Some((at, prefix_end(word_text(entry, word), query_word)?))
            })?;
        assignment.push((at, end));
        from = at + 1;
    }
    Some(assignment)
}

/// Each query word on a word of its own, in any order.
fn assign_in_any_order(entry: &Entry, words: &[Word], query: &[Vec<char>]) -> Option<Assignment> {
    let mut assignment = Assignment::new();
    for query_word in query {
        let (at, end) = words.iter().enumerate().find_map(|(at, word)| {
            let taken = assignment.iter().any(|&(used, _)| used == at);
            if taken {
                return None;
            }
            Some((at, prefix_end(word_text(entry, word), query_word)?))
        })?;
        assignment.push((at, end));
    }
    Some(assignment)
}

struct Match {
    tier: Tier,
    label_highlight: Vec<Range<usize>>,
    theme_highlight: Vec<Range<usize>>,
}

/// How `entry` matches the query and what to highlight, or `None` when it
/// does not.
fn match_words(entry: &Entry, query: &[Vec<char>], typed: &str) -> Option<Match> {
    let mut words = Vec::new();
    words_of(&entry.label, false, &mut words);
    if let Some(theme_name) = entry.theme_name.as_deref() {
        words_of(theme_name, true, &mut words);
    }
    let in_order = assign_in_order(entry, &words, query);
    let Some(assignment) = in_order
        .clone()
        .or_else(|| assign_in_any_order(entry, &words, query))
    else {
        // Whole-word prefixes, not fuzzy: a fuzzy name match would also find
        // unrelated commands. There is nothing to highlight in the label.
        let named = entry
            .command()
            .is_some_and(|action| words_start_name(typed, action.name()));
        return named.then(|| Match {
            tier: Tier::ConfigName,
            label_highlight: Vec::new(),
            theme_highlight: Vec::new(),
        });
    };

    let whole: Vec<char> = typed.trim().chars().map(fold).collect();
    let tier = if starts_with_folded(&entry.label, &whole)
        || entry
            .theme_name
            .as_deref()
            .is_some_and(|theme_name| starts_with_folded(theme_name, &whole))
    {
        Tier::Start
    } else if in_order.is_some() {
        Tier::InOrder
    } else {
        Tier::AnyOrder
    };

    let mut label = Vec::new();
    let mut theme = Vec::new();
    for (at, end) in assignment {
        let Some(word) = words.get(at) else {
            continue;
        };
        let range = word.range.start..word.range.start + end;
        if word.in_theme {
            theme.push(range);
        } else {
            label.push(range);
        }
    }
    label.sort_by_key(|range| range.start);
    theme.sort_by_key(|range| range.start);
    Some(Match {
        tier,
        label_highlight: label,
        theme_highlight: theme,
    })
}

/// Every word of `query` begins a word of the config `name` (`quick_open`).
fn words_start_name(query: &str, name: &str) -> bool {
    let lowered = query.to_lowercase();
    let mut words = lowered.split_whitespace().peekable();
    words.peek().is_some() && words.all(|word| name.split('_').any(|part| part.starts_with(word)))
}

/// The commands that work in `scope`. The Lab does not list itself.
fn entries(scope: KeyScope, lang: Language, keymap: &Keymap) -> Vec<Entry> {
    commands::all()
        .filter(|command| command.action != KeyAction::CommandPalette)
        .filter(|command| command.action.works_in(scope))
        .map(|command| Entry {
            target: Target::Command(command.action),
            group: PaletteGroup::Command(command.group),
            label: lang.text(command.label).to_owned(),
            theme_name: command.theme_name.map(|name| lang.text(name).to_owned()),
            shortcut: keymap.shortcut_label(&command.action.message()),
        })
        .collect()
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn palette(scope: KeyScope, lang: Language) -> CommandPalette {
        CommandPalette::new(scope, lang, &Keymap::default(), &[])
    }

    fn asked(lang: Language, query: &str) -> CommandPalette {
        let mut palette = palette(KeyScope::Panel, lang);
        palette.set_query(query.to_owned());
        palette
    }

    fn labels(palette: &CommandPalette) -> Vec<&str> {
        palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap().label.as_str())
            .collect()
    }

    fn found(lang: Language, query: &str) -> Vec<KeyAction> {
        let palette = asked(lang, query);
        palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap().command().unwrap())
            .collect()
    }

    fn actions_of(palette: &CommandPalette) -> Vec<KeyAction> {
        palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap().command().unwrap())
            .collect()
    }

    fn headers(palette: &CommandPalette) -> Vec<&'static str> {
        palette
            .rows()
            .iter()
            .filter_map(|row| match row {
                Row::Header(header) => Some(header.label),
                Row::Hit(_) => None,
            })
            .collect()
    }

    #[test]
    fn se_lists_the_search_first_and_not_sort_by_size() {
        let palette = asked(Language::English, "se");
        let labels = labels(&palette);
        assert!(labels[..2].contains(&"Search in files"), "{labels:?}");
        assert!(!labels.contains(&"Sort by size"), "{labels:?}");
        assert!(!labels.contains(&"Switch panel"), "{labels:?}");
    }

    #[test]
    fn a_theme_name_finds_the_command() {
        assert_eq!(found(Language::English, "smell"), [KeyAction::Search]);
        assert!(found(Language::English, "dark matter").contains(&KeyAction::QuickOpen));
    }

    #[test]
    fn the_theme_name_is_marked_where_it_matched() {
        let palette = asked(Language::English, "smell");
        let hit = palette.hits().next().unwrap();
        let theme_name = palette.entry(hit).unwrap().theme_name.clone().unwrap();
        let marked: Vec<&str> = hit
            .theme_highlight
            .iter()
            .map(|range| &theme_name[range.clone()])
            .collect();
        assert_eq!(marked, ["Smell"]);
        assert!(hit.label_highlight.is_empty());
    }

    #[test]
    fn the_config_name_finds_the_dark_matter_drive() {
        assert!(found(Language::English, "quick open").contains(&KeyAction::QuickOpen));
        assert!(found(Language::English, "quick op").contains(&KeyAction::QuickOpen));
    }

    #[test]
    fn a_hit_through_the_config_name_marks_nothing() {
        let palette = asked(Language::English, "quick open");
        let hit = palette
            .hits()
            .find(|hit| palette.entry(hit).unwrap().command().unwrap() == KeyAction::QuickOpen)
            .unwrap();
        assert!(hit.label_highlight.is_empty());
        assert!(hit.theme_highlight.is_empty());
    }

    #[test]
    fn a_word_only_matches_at_its_start() {
        let actions = found(Language::English, "ize");
        assert!(actions.is_empty(), "{actions:?}");
        assert_eq!(
            found(Language::English, "insp"),
            [KeyAction::FilingStatus, KeyAction::HermesInspect]
        );
    }

    #[test]
    fn german_labels_match_in_german() {
        let palette = asked(Language::German, "such");
        assert!(labels(&palette).contains(&"In Dateien suchen"));
        assert_eq!(
            labels(&asked(Language::German, "zu datei")),
            ["Zu Datei springen"]
        );
    }

    #[test]
    fn losch_finds_loeschen_without_breaking_the_marks() {
        for typed in ["losch", "lösch"] {
            let palette = asked(Language::German, typed);
            let labels = labels(&palette);
            assert!(labels.contains(&"Löschen"), "{typed}: {labels:?}");
            for hit in palette.hits() {
                let entry = palette.entry(hit).unwrap();
                for range in &hit.label_highlight {
                    assert!(entry.label.get(range.clone()).is_some(), "{range:?}");
                }
            }
            let delete = palette
                .hits()
                .find(|hit| palette.entry(hit).unwrap().label == "Löschen")
                .unwrap();
            assert_eq!(delete.label_highlight, vec![0.."Lösch".len()]);
        }
    }

    #[test]
    fn the_tiers_are_the_whole_label_then_words_in_order_then_another_order() {
        let palette = palette(KeyScope::Panel, Language::English);
        let entry = palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap())
            .find(|entry| entry.command().unwrap() == KeyAction::Move)
            .unwrap();
        let tier_of = |query: &str| match_words(entry, &query_words(query), query).map(|m| m.tier);
        assert_eq!(tier_of("move to"), Some(Tier::Start));
        assert_eq!(tier_of("to other"), Some(Tier::InOrder));
        assert_eq!(tier_of("other to"), Some(Tier::AnyOrder));
        assert_eq!(tier_of("move"), Some(Tier::Start));
        assert_eq!(tier_of("quick open"), None);
        assert!(Tier::Start < Tier::InOrder);
        assert!(Tier::InOrder < Tier::AnyOrder);
        assert!(Tier::AnyOrder < Tier::ConfigName);
    }

    #[test]
    fn a_better_tier_is_listed_before_a_worse_one() {
        // "Fetch color theme…" has a word that starts with "co" but does not
        // start with it, so it comes after the labels of its group that do.
        // Across groups the order is by group, so only its group counts.
        let asked_co = asked(Language::English, "co");
        let entries: Vec<_> = asked_co
            .hits()
            .map(|hit| asked_co.entry(hit).unwrap())
            .collect();
        let color_group = entries
            .iter()
            .find(|entry| entry.label.contains("color"))
            .unwrap()
            .group;
        let labels: Vec<&str> = entries
            .iter()
            .filter(|entry| entry.group == color_group)
            .map(|entry| entry.label.as_str())
            .collect();
        let color = labels
            .iter()
            .position(|label| label.contains("color"))
            .unwrap();
        let last_start = labels
            .iter()
            .rposition(|label| label.to_lowercase().starts_with("co"))
            .unwrap();
        assert!(last_start < color, "{labels:?}");
    }

    #[test]
    fn hits_are_grouped_under_a_header() {
        let palette = asked(Language::English, "s");
        let mut group_of_hit = None;
        let mut headers_seen = 0;
        for row in palette.rows() {
            match row {
                Row::Header(_) => {
                    headers_seen += 1;
                    group_of_hit = None;
                }
                Row::Hit(hit) => {
                    let group = palette.entry(hit).unwrap().group;
                    assert!(
                        group_of_hit.is_none_or(|before| before == group),
                        "a hit of another group under the same header"
                    );
                    group_of_hit = Some(group);
                }
            }
        }
        assert!(headers_seen > 1);
        assert!(matches!(palette.rows().first(), Some(Row::Header(_))));
    }

    #[test]
    fn no_group_header_repeats() {
        for (lang, query) in [
            (Language::German, "st"),
            (Language::English, "st"),
            (Language::English, "s"),
        ] {
            let palette = asked(lang, query);
            let seen = headers(&palette);
            for (index, label) in seen.iter().enumerate() {
                assert!(!seen[..index].contains(label), "{query}: {seen:?}");
            }
        }
    }

    #[test]
    fn the_group_with_the_best_hit_comes_first_then_the_group_order() {
        for lang in [Language::English, Language::German] {
            for query in ["s", "st", "pa", "in", "e"] {
                let palette = asked(lang, query);
                let words = query_words(query);
                // The group of each hit with the best tier any hit of it has.
                let mut listed: Vec<(Tier, PaletteGroup)> = Vec::new();
                for hit in palette.hits() {
                    let entry = palette.entry(hit).unwrap();
                    let tier = match_words(entry, &words, query).unwrap().tier;
                    match listed.iter_mut().find(|(_, group)| *group == entry.group) {
                        Some(known) => known.0 = known.0.min(tier),
                        None => listed.push((tier, entry.group)),
                    }
                }
                let mut sorted = listed.clone();
                sorted.sort();
                assert_eq!(listed, sorted, "{lang:?} {query:?}");
            }
        }
    }

    #[test]
    fn a_later_group_with_a_better_hit_is_listed_first() {
        // "in" is a word of "Search in files" (Search group) and starts the
        // theme names "Inspection" and "Inventory" (Hermes group, after Search
        // in the fixed order).
        let palette = asked(Language::English, "in");
        let position_of = |group| {
            palette
                .hits()
                .position(|hit| palette.entry(hit).unwrap().group == group)
        };
        let hermes = position_of(PaletteGroup::Command(CommandGroup::Hermes));
        let search = position_of(PaletteGroup::Command(CommandGroup::Search));
        assert!(hermes.is_some());
        assert!(hermes < search, "{hermes:?} {search:?}");
        assert!(palette
            .hits()
            .any(|hit| palette.entry(hit).unwrap().command().unwrap() == KeyAction::Search));
    }

    #[test]
    fn the_cursor_and_select_skip_headers() {
        let mut palette = asked(Language::English, "s");
        assert!(matches!(
            palette.rows().get(palette.cursor()),
            Some(Row::Hit(_))
        ));
        palette.select(0);
        assert!(matches!(
            palette.rows().get(palette.cursor()),
            Some(Row::Hit(_))
        ));
        for _ in 0..palette.hits().count() {
            palette.move_cursor(1);
            assert!(matches!(
                palette.rows().get(palette.cursor()),
                Some(Row::Hit(_))
            ));
        }
        assert_eq!(palette.cursor(), 1, "a full round ends on the first hit");
        assert_eq!(palette.visible().start, 0, "its header is in view");
    }

    #[test]
    fn an_empty_query_lists_every_command_by_group_in_a_fixed_order() {
        let palette = palette(KeyScope::Panel, Language::English);
        assert_eq!(
            headers(&palette),
            ["File", "Edit", "View", "Go", "Search", "Filing", "Help"]
        );
        let Some(Row::Header(first)) = palette.rows().first() else {
            panic!("a header comes first");
        };
        assert_eq!(first.label, "File");
        let Some(Row::Header(hermes)) = palette
            .rows()
            .iter()
            .find(|row| matches!(row, Row::Header(header) if header.label == "Filing"))
        else {
            panic!("the Hermes group");
        };
        assert_eq!(hermes.theme_name, Some("Hermes"));
        assert_eq!(hermes.beta_mark, Some("Beta"));
        let marked = palette
            .rows()
            .iter()
            .filter(|row| matches!(row, Row::Header(header) if header.beta_mark.is_some()))
            .count();
        assert_eq!(marked, 1, "only the Hermes group carries the mark");
        let mut again = super::tests::palette(KeyScope::Panel, Language::English);
        again.set_query(String::new());
        assert_eq!(palette.rows(), again.rows());
    }

    #[test]
    fn recent_commands_head_the_empty_list_most_recent_first() {
        let keymap = Keymap::default();
        let recent = [KeyAction::Copy, KeyAction::Mkdir, KeyAction::Search];
        let palette = CommandPalette::new(KeyScope::Panel, Language::English, &keymap, &recent);
        let Some(Row::Header(first)) = palette.rows().first() else {
            panic!("a header comes first");
        };
        assert_eq!(first.label, "Recent");
        let after_header: Vec<KeyAction> = palette
            .rows()
            .iter()
            .skip(1)
            .take(3)
            .map(|row| match row {
                Row::Hit(hit) => palette.entry(hit).unwrap().command().unwrap(),
                Row::Header(_) => panic!("three hits come first"),
            })
            .collect();
        assert_eq!(after_header, recent);
        assert_eq!(headers(&palette).get(1), Some(&"File"));
        let again = palette
            .hits()
            .filter(|hit| palette.entry(hit).unwrap().command().unwrap() == KeyAction::Copy)
            .count();
        assert_eq!(again, 2, "a recent command is also in its group");
        assert_eq!(
            palette.selected_target(),
            Some(Target::Command(KeyAction::Copy)),
            "the most recent one is highlighted"
        );
    }

    #[test]
    fn no_more_than_five_recent_commands_are_listed() {
        let recent = [
            KeyAction::Copy,
            KeyAction::Move,
            KeyAction::Mkdir,
            KeyAction::Open,
            KeyAction::Edit,
            KeyAction::View,
            KeyAction::Quit,
        ];
        let palette = CommandPalette::new(
            KeyScope::Panel,
            Language::English,
            &Keymap::default(),
            &recent,
        );
        let before_groups = palette
            .rows()
            .iter()
            .skip(1)
            .take_while(|row| matches!(row, Row::Hit(_)))
            .count();
        assert_eq!(before_groups, RECENT_LIMIT);
    }

    #[test]
    fn a_query_has_no_recent_header() {
        let mut palette = CommandPalette::new(
            KeyScope::Panel,
            Language::English,
            &Keymap::default(),
            &[KeyAction::Copy],
        );
        palette.set_query("copy".to_owned());
        assert!(!headers(&palette).contains(&"Recent"));
    }

    #[test]
    fn the_lab_does_not_list_itself() {
        assert!(!actions_of(&palette(KeyScope::Panel, Language::English))
            .contains(&KeyAction::CommandPalette));
    }

    #[test]
    fn the_search_lists_its_own_actions_and_not_the_panel_ones() {
        let search = palette(KeyScope::Search, Language::English);
        let actions = actions_of(&search);
        assert!(actions.contains(&KeyAction::SearchRegex));
        assert!(actions.contains(&KeyAction::Quit));
        assert!(!actions.contains(&KeyAction::Mkdir));
        let panel = palette(KeyScope::Panel, Language::English);
        assert!(!actions_of(&panel).contains(&KeyAction::SearchRegex));
    }

    fn shortcuts_match(keymap: &Keymap) {
        let mut seen: Vec<KeyAction> = Vec::new();
        for scope in [KeyScope::Panel, KeyScope::Search] {
            let palette = CommandPalette::new(scope, Language::English, keymap, &[]);
            let actions = actions_of(&palette);
            for (index, action) in actions.iter().enumerate() {
                assert!(!actions[..index].contains(action), "{action:?} twice");
            }
            for hit in palette.hits() {
                let entry = palette.entry(hit).unwrap();
                assert_eq!(
                    entry.shortcut,
                    keymap.shortcut_label(&entry.command().unwrap().message()),
                    "{}",
                    entry.command().unwrap().name()
                );
                if !seen.contains(&entry.command().unwrap()) {
                    seen.push(entry.command().unwrap());
                }
            }
        }
        for command in commands::all() {
            assert!(
                seen.contains(&command.action) || command.action == KeyAction::CommandPalette,
                "{} is in no list",
                command.action.name()
            );
        }
    }

    #[test]
    fn every_command_is_listed_once_with_the_shortcut_of_the_keymap() {
        shortcuts_match(&Keymap::default());
    }

    #[test]
    fn an_override_shows_in_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[keys]\nfiling_status = \"F11\"\ncopy = \"F12\"\n").unwrap();
        let keymap = (*crate::config::load(&path).unwrap().keymap).clone();
        shortcuts_match(&keymap);
        let palette = CommandPalette::new(KeyScope::Panel, Language::English, &keymap, &[]);
        let shortcut_of = |action| {
            palette
                .hits()
                .map(|hit| palette.entry(hit).unwrap())
                .find(|entry| entry.command().unwrap() == action)
                .unwrap()
                .shortcut
                .clone()
        };
        assert_eq!(shortcut_of(KeyAction::FilingStatus).as_deref(), Some("F11"));
        assert_eq!(shortcut_of(KeyAction::Copy).as_deref(), Some("F12"));
    }

    #[test]
    fn a_new_query_puts_the_cursor_back_on_the_first_hit() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.move_cursor(5);
        assert_ne!(palette.cursor(), 1);
        palette.set_query("e".to_owned());
        assert!(matches!(
            palette.rows().get(palette.cursor()),
            Some(Row::Hit(_))
        ));
        assert_eq!(palette.cursor(), 1);
    }

    #[test]
    fn the_cursor_stays_in_view_and_inside_the_list() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        assert_eq!(palette.cursor(), 1);
        palette.move_cursor(-1);
        let last_hit = palette.rows().len() - 1;
        assert_eq!(palette.cursor(), last_hit, "up from the first goes round");
        let visible = palette.visible();
        assert!(visible.contains(&palette.cursor()), "{visible:?}");
        assert_eq!(visible.len(), MIN_VISIBLE_ROWS);
        palette.move_cursor(1);
        assert_eq!(palette.cursor(), 1, "down from the last goes round");
        assert_eq!(palette.visible().start, 0);
    }

    #[test]
    fn the_cursor_goes_round_at_both_ends_and_survives_an_empty_list() {
        let mut palette = asked(Language::English, "s");
        let hits = palette.hits().count();
        for _ in 0..hits {
            palette.move_cursor(1);
        }
        assert_eq!(palette.cursor(), 1, "a full round ends where it began");
        let mut nothing = asked(Language::English, "zzzzzzzzqq");
        assert_eq!(nothing.hits().count(), 0);
        nothing.move_cursor(1);
        nothing.move_cursor(-1);
    }

    #[test]
    fn selecting_a_header_or_past_the_end_changes_nothing() {
        let mut palette = palette(KeyScope::Panel, Language::English);
        palette.select(3);
        let at = palette.cursor();
        palette.select(10_000);
        assert_eq!(palette.cursor(), at);
        let header = palette
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Header(_)))
            .unwrap();
        assert_eq!(header, 0);
        palette.select(header);
        assert_eq!(palette.cursor(), at);
    }

    #[test]
    fn the_entry_carries_the_first_key_of_its_action() {
        let palette = palette(KeyScope::Panel, Language::English);
        let key_of = |action| {
            palette
                .hits()
                .map(|hit| palette.entry(hit).unwrap())
                .find(|entry| entry.command().unwrap() == action)
                .unwrap()
                .shortcut
                .clone()
        };
        assert_eq!(key_of(KeyAction::Copy).as_deref(), Some("F5"));
        assert_eq!(key_of(KeyAction::Open).as_deref(), Some("Enter"));
    }

    #[test]
    fn the_hermes_file_commands_are_listed_only_while_the_column_is_up() {
        let listed = |palette: &CommandPalette| {
            palette.hits().any(|hit| {
                palette.entry(hit).unwrap().command().unwrap() == KeyAction::HermesProcessAgain
            })
        };
        let with = palette(KeyScope::Panel, Language::English);
        assert!(listed(&with));
        let without = with.without_hermes_file_commands();
        assert!(!listed(&without));
        assert!(without
            .hits()
            .any(|hit| without.entry(hit).unwrap().command().unwrap() == KeyAction::FilingStatus));
    }

    #[test]
    fn without_locations_only_the_role_commands_stay_among_the_hermes_file_commands() {
        let palette =
            palette(KeyScope::Panel, Language::English).without_hermes_location_commands();
        let actions: Vec<KeyAction> = palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap().command().unwrap())
            .collect();
        assert!(actions.contains(&KeyAction::HermesSetInbox));
        assert!(actions.contains(&KeyAction::HermesSetFilingLocation));
        assert!(actions.contains(&KeyAction::HermesSetArchive));
        assert!(!actions.contains(&KeyAction::HermesProcessAgain));
        assert!(!actions.contains(&KeyAction::HermesExclude));
    }

    #[test]
    fn one_command_can_be_dropped_for_the_row_under_the_cursor() {
        let palette = palette(KeyScope::Panel, Language::English)
            .without_command(KeyAction::HermesProcessAgain);
        let actions: Vec<KeyAction> = palette
            .hits()
            .map(|hit| palette.entry(hit).unwrap().command().unwrap())
            .collect();
        assert!(!actions.contains(&KeyAction::HermesProcessAgain));
        assert!(actions.contains(&KeyAction::HermesExclude));
    }

    fn setting_targets(lang: Language) -> Vec<SettingsTarget> {
        let (page, viewer) = match lang {
            Language::English => ("Settings: Programs", "Viewer (F3)"),
            Language::German => ("Einstellungen: Programme", "Betrachter (F3)"),
        };
        vec![
            SettingsTarget {
                feature: SettingsFeature::Programs,
                row: None,
                label: page.to_owned(),
                theme_name: None,
            },
            SettingsTarget {
                feature: SettingsFeature::Programs,
                row: Some(viewer.to_owned()),
                label: viewer.to_owned(),
                theme_name: None,
            },
        ]
    }

    fn with_settings(lang: Language, query: &str) -> CommandPalette {
        let mut palette = palette(KeyScope::Panel, lang).with_settings(setting_targets(lang));
        palette.set_query(query.to_owned());
        palette
    }

    fn setting_hits(palette: &CommandPalette) -> Vec<(SettingsFeature, Option<String>)> {
        palette
            .hits()
            .filter_map(|hit| match &palette.entry(hit).unwrap().target {
                Target::Setting { feature, row } => Some((*feature, row.clone())),
                Target::Command(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_settings_page_is_found_by_its_name_in_both_languages() {
        for (lang, query) in [
            (Language::English, "settings prog"),
            (Language::German, "einstellungen prog"),
        ] {
            let palette = with_settings(lang, query);
            assert!(
                setting_hits(&palette).contains(&(SettingsFeature::Programs, None)),
                "{lang:?}"
            );
        }
    }

    #[test]
    fn one_setting_is_found_by_its_label_in_both_languages() {
        for (lang, query, label) in [
            (Language::English, "viewer", "Viewer (F3)"),
            (Language::German, "betrachter", "Betrachter (F3)"),
        ] {
            let palette = with_settings(lang, query);
            assert_eq!(
                setting_hits(&palette),
                [(SettingsFeature::Programs, Some(label.to_owned()))],
                "{lang:?}"
            );
            assert_eq!(headers(&palette), [lang.text(Msg::ActionSettings)]);
        }
    }

    #[test]
    fn a_command_comes_before_a_setting_of_the_same_tier() {
        let targets = vec![SettingsTarget {
            feature: SettingsFeature::Programs,
            row: Some("Search depth".to_owned()),
            label: "Search depth".to_owned(),
            theme_name: None,
        }];
        let mut palette = palette(KeyScope::Panel, Language::English).with_settings(targets);
        palette.set_query("search".to_owned());
        let first = palette.hits().next().unwrap();
        assert!(palette.entry(first).unwrap().command().is_some());
        let last = palette.hits().last().unwrap();
        assert_eq!(palette.entry(last).unwrap().group, PaletteGroup::Settings);
    }

    #[test]
    fn settings_are_not_in_the_overview_and_recent_is_untouched() {
        let recent = [KeyAction::Copy];
        let with = CommandPalette::new(
            KeyScope::Panel,
            Language::English,
            &Keymap::default(),
            &recent,
        )
        .with_settings(setting_targets(Language::English));
        let without = CommandPalette::new(
            KeyScope::Panel,
            Language::English,
            &Keymap::default(),
            &recent,
        );
        assert_eq!(with.rows(), without.rows());
        assert!(setting_hits(&with).is_empty());
        assert_eq!(headers(&with)[0], "Recent");
    }

    #[test]
    fn a_setting_is_never_found_through_a_config_name() {
        let mut palette = with_settings(Language::English, "viewer");
        palette.set_query("quick open".to_owned());
        assert!(setting_hits(&palette).is_empty());
    }

    #[test]
    fn running_a_setting_gives_its_target() {
        let palette = with_settings(Language::English, "viewer");
        assert_eq!(
            palette.selected_target(),
            Some(Target::Setting {
                feature: SettingsFeature::Programs,
                row: Some("Viewer (F3)".to_owned()),
            })
        );
    }
}
