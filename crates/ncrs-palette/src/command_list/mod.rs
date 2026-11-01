//! The command list, the Lab (Cmd/Ctrl+Shift+P): every command that can be run
//! by name in the current area of the window, found by the start of the words
//! of its plain label or its theme name, and grouped. No widgets: the view
//! reads the rows, the typed text goes to [`CommandPalette::set_query`].

use std::cmp::Ordering;
use std::ops::Range;

use ncrs_config::commands::{self, CommandGroup};
use ncrs_config::keymap::{KeyAction, KeyScope, Keymap};
use ncrs_config::{SettingsFeature, SettingsTarget};
use ncrs_i18n::{FuturamaName, Language, Msg};

pub mod action;
pub mod ctx;
pub mod update;
pub mod view;

pub use action::CommandPaletteAction;
pub use ctx::{CommandPaletteCtx, HermesCommands};
pub use update::{close, resize, update};

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

    fn theme_name(self) -> Option<FuturamaName> {
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
                        .and_then(|name| self.lang.futurama_name(name)),
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
            theme_name: command
                .theme_name
                .and_then(|name| lang.futurama_name(name))
                .map(str::to_owned),
            shortcut: keymap.shortcut_label_of(command.action),
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
mod tests;
