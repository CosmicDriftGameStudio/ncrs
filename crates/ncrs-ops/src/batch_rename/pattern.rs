//! The new names of a batch rename: the pattern with its tokens, find and
//! replace, case, and the clashes between the names. Pure: nothing here
//! reads a file or the clock.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use regex::Regex;

use crate::rename_flow::{validate_name, NameProblem};

/// What a token can need of an entry that the entry may not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Modified,
    Sender,
    DocumentDate,
    Kind,
    Amount,
}

/// Everything the tokens read from one entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryFacts {
    /// The name without the extension; the whole name of a folder.
    pub stem: String,
    /// With the dot; empty for a folder or a name without one.
    pub extension: String,
    pub modified: Option<NaiveDate>,
    /// From the picture; `{date taken}` falls back to `modified`.
    pub taken: Option<NaiveDate>,
    pub sender: Option<String>,
    pub kind: Option<String>,
    pub document_date: Option<NaiveDate>,
    /// Written as `12.34 EUR`.
    pub amount: Option<String>,
}

impl EntryFacts {
    pub fn old_name(&self) -> String {
        format!("{}{}", self.stem, self.extension)
    }
}

/// Splits a file name at its last dot; a leading dot is no extension.
pub fn split_name(name: &str, is_dir: bool) -> (String, String) {
    match name.rfind('.') {
        Some(dot) if !is_dir && dot > 0 => {
            let (stem, extension) = name.split_at(dot);
            (stem.to_owned(), extension.to_owned())
        }
        _ => (name.to_owned(), String::new()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Case {
    #[default]
    Keep,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    pub pattern: &'a str,
    pub start: u64,
    pub digits: usize,
    pub find: &'a str,
    pub replace: &'a str,
    pub regex: bool,
    pub case: Case,
}

/// The rules of the file system the names are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NameRules {
    /// Windows names: the stricter set of characters.
    pub windows: bool,
    /// Two names that differ in case only are one name.
    pub fold_case: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece {
    Literal(String),
    /// Characters `first` to `last`, counted from 1.
    Name(Option<(usize, usize)>),
    Counter,
    Modified,
    Taken,
    Extension,
    Sender,
    DocumentDate,
    Kind,
    Amount,
    Year,
    Month,
    Day,
}

fn piece_of(token: &str) -> Option<Piece> {
    let simple = match token {
        "name" => Piece::Name(None),
        "counter" => Piece::Counter,
        "date modified" => Piece::Modified,
        "date taken" => Piece::Taken,
        "extension" => Piece::Extension,
        "sender" => Piece::Sender,
        "document date" => Piece::DocumentDate,
        "kind" => Piece::Kind,
        "amount" => Piece::Amount,
        "year" => Piece::Year,
        "month" => Piece::Month,
        "day" => Piece::Day,
        _ => return range_of(token),
    };
    Some(simple)
}

fn range_of(token: &str) -> Option<Piece> {
    let (first_text, last_text) = token.strip_prefix("name:")?.split_once('-')?;
    let first: usize = first_text.trim().parse().ok()?;
    let last: usize = last_text.trim().parse().ok()?;
    (first >= 1 && last >= first).then_some(Piece::Name(Some((first, last))))
}

/// The pieces of a pattern. A `{...}` that is no token stays as it is.
fn parse(pattern: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        let (before, from_brace) = rest.split_at(open);
        literal.push_str(before);
        let stop = from_brace
            .char_indices()
            .skip(1)
            .find(|(_, character)| matches!(character, '{' | '}'));
        match stop {
            Some((close, '}')) => {
                let (braced, after) = from_brace.split_at(close + 1);
                let inner = braced
                    .strip_prefix('{')
                    .and_then(|text| text.strip_suffix('}'))
                    .unwrap_or_default();
                match piece_of(inner) {
                    Some(piece) => {
                        if !literal.is_empty() {
                            pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                        }
                        pieces.push(piece);
                    }
                    None => literal.push_str(braced),
                }
                rest = after;
            }
            _ => {
                literal.push('{');
                rest = from_brace.split_at(1).1;
            }
        }
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    pieces
}

fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn piece_text(piece: &Piece, facts: &EntryFacts, counter: &str) -> Result<String, Field> {
    let document_date = || facts.document_date.ok_or(Field::DocumentDate);
    Ok(match piece {
        Piece::Literal(text) => text.clone(),
        Piece::Name(None) => facts.stem.clone(),
        Piece::Name(Some((first, last))) => facts
            .stem
            .chars()
            .skip(first - 1)
            .take(last - first + 1)
            .collect(),
        Piece::Counter => counter.to_owned(),
        Piece::Modified => iso(facts.modified.ok_or(Field::Modified)?),
        Piece::Taken => iso(facts.taken.or(facts.modified).ok_or(Field::Modified)?),
        Piece::Extension => facts.extension.clone(),
        Piece::Sender => facts.sender.clone().ok_or(Field::Sender)?,
        Piece::DocumentDate => iso(document_date()?),
        Piece::Kind => facts.kind.clone().ok_or(Field::Kind)?,
        Piece::Amount => facts.amount.clone().ok_or(Field::Amount)?,
        Piece::Year => document_date()?.format("%Y").to_string(),
        Piece::Month => document_date()?.format("%m").to_string(),
        Piece::Day => document_date()?.format("%d").to_string(),
    })
}

/// Find and replace, as set in the dialog.
enum Finder {
    Nothing,
    Plain { find: String, replace: String },
    Expression { regex: Regex, replace: String },
}

impl Finder {
    fn apply(&self, name: String) -> String {
        match self {
            Finder::Nothing => name,
            Finder::Plain { find, replace } => name.replace(find.as_str(), replace),
            Finder::Expression { regex, replace } => {
                regex.replace_all(&name, replace.as_str()).into_owned()
            }
        }
    }
}

/// The finder, or why the expression is not one. An empty find does nothing.
fn finder_of(options: &Options<'_>) -> (Finder, Option<String>) {
    if options.find.is_empty() {
        return (Finder::Nothing, None);
    }
    let replace = options.replace.to_owned();
    if !options.regex {
        let plain = Finder::Plain {
            find: options.find.to_owned(),
            replace,
        };
        return (plain, None);
    }
    match Regex::new(options.find) {
        Ok(regex) => (Finder::Expression { regex, replace }, None),
        Err(error) => (Finder::Nothing, Some(regex_reason(&error.to_string()))),
    }
}

/// The last line of a regex error, which says what is wrong; the lines
/// before it repeat the expression with marks.
fn regex_reason(message: &str) -> String {
    let last = message.lines().last().unwrap_or(message).trim();
    last.strip_prefix("error:")
        .unwrap_or(last)
        .trim()
        .to_owned()
}

fn change_case(name: String, case: Case) -> String {
    match case {
        Case::Keep => name,
        Case::Lower => name.to_lowercase(),
        Case::Upper => name.to_uppercase(),
    }
}

fn render_with(
    pieces: &[Piece],
    finder: &Finder,
    options: &Options<'_>,
    facts: &EntryFacts,
    position: usize,
) -> Result<String, Field> {
    let number = options
        .start
        .saturating_add(u64::try_from(position).unwrap_or(u64::MAX));
    let counter = format!("{number:0>width$}", width = options.digits);
    let mut name = String::new();
    for piece in pieces {
        name.push_str(&piece_text(piece, facts, &counter)?);
    }
    Ok(change_case(finder.apply(name), options.case))
}

/// The new name of the entry at `position`; an invalid expression finds and
/// replaces nothing.
pub fn render(options: &Options<'_>, facts: &EntryFacts, position: usize) -> Result<String, Field> {
    let (finder, _) = finder_of(options);
    render_with(&parse(options.pattern), &finder, options, facts, position)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Missing(Field),
    Name(NameProblem),
    /// Another entry gets the same name; the line is counted from 1.
    SameAs(usize),
    /// An entry of the folder that stays has this name.
    Exists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRow {
    pub old_name: String,
    /// Empty when a field is missing.
    pub new_name: String,
    pub problem: Option<Problem>,
}

impl PreviewRow {
    pub fn is_renamed(&self) -> bool {
        self.problem.is_none() && self.new_name != self.old_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preview {
    pub rows: Vec<PreviewRow>,
    pub regex_error: Option<String>,
}

impl Preview {
    /// The rows that cannot go on as they are.
    pub fn clashes(&self) -> usize {
        self.rows.iter().filter(|row| row.problem.is_some()).count()
    }

    pub fn can_apply(&self) -> bool {
        self.regex_error.is_none() && self.clashes() == 0
    }

    /// The old and new name of every entry that gets another one.
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.rows
            .iter()
            .filter(|row| row.is_renamed())
            .map(|row| (row.old_name.clone(), row.new_name.clone()))
            .collect()
    }
}

fn key_of(name: &str, rules: NameRules) -> String {
    if rules.fold_case {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
}

/// The new names of `entries`, in the order given, and what is wrong with
/// each. `folder_names` are all the names of the folder, the entries' own
/// included. An unchanged name is no clash.
pub fn preview(
    options: &Options<'_>,
    entries: &[EntryFacts],
    folder_names: &[String],
    rules: NameRules,
) -> Preview {
    let (finder, regex_error) = finder_of(options);
    let pieces = parse(options.pattern);
    let mut rows: Vec<PreviewRow> = entries
        .iter()
        .enumerate()
        .map(|(position, facts)| {
            let old_name = facts.old_name();
            match render_with(&pieces, &finder, options, facts, position) {
                Err(field) => PreviewRow {
                    old_name,
                    new_name: String::new(),
                    problem: Some(Problem::Missing(field)),
                },
                Ok(new_name) => {
                    let problem = validate_name(&new_name, rules.windows)
                        .err()
                        .map(Problem::Name);
                    PreviewRow {
                        old_name,
                        new_name,
                        problem,
                    }
                }
            }
        })
        .collect();
    mark_clashes(&mut rows, folder_names, rules);
    Preview { rows, regex_error }
}

fn mark_clashes(rows: &mut [PreviewRow], folder_names: &[String], rules: NameRules) {
    let leaving: HashSet<String> = rows
        .iter()
        .filter(|row| row.new_name != row.old_name)
        .map(|row| key_of(&row.old_name, rules))
        .collect();
    let occupied: HashSet<String> = folder_names
        .iter()
        .map(|name| key_of(name, rules))
        .filter(|key| !leaving.contains(key))
        .collect();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (position, row) in rows.iter().enumerate() {
        if row.is_renamed() {
            by_name
                .entry(key_of(&row.new_name, rules))
                .or_default()
                .push(position);
        }
    }
    for (position, row) in rows.iter_mut().enumerate() {
        if !row.is_renamed() {
            continue;
        }
        let key = key_of(&row.new_name, rules);
        let other = by_name
            .get(&key)
            .and_then(|group| group.iter().find(|&&member| member != position));
        row.problem = match other {
            Some(member) => Some(Problem::SameAs(member + 1)),
            None => occupied.contains(&key).then_some(Problem::Exists),
        };
    }
}

/// One rename of an entry of the folder, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub from: String,
    pub to: String,
}

/// The order that makes every rename find its target free: a name another
/// entry is about to leave is taken after that one has left, and a cycle is
/// broken by moving one entry to a temporary name first. `taken` are the
/// names of the folder, which a temporary name must not be.
pub fn plan(pairs: &[(String, String)], taken: &[String], rules: NameRules) -> Vec<Step> {
    let mut pending: Vec<(String, String)> = pairs
        .iter()
        .filter(|(from, to)| from != to)
        .cloned()
        .collect();
    let mut used: HashSet<String> = taken.iter().map(|name| key_of(name, rules)).collect();
    for (from, to) in pairs {
        used.insert(key_of(from, rules));
        used.insert(key_of(to, rules));
    }
    let mut steps = Vec::new();
    let mut spare: usize = 0;
    while !pending.is_empty() {
        let free = pending.iter().position(|(_, target)| {
            let wanted = key_of(target, rules);
            !pending
                .iter()
                .any(|(source, _)| key_of(source, rules) == wanted)
        });
        if let Some(index) = free {
            let (from, to) = pending.remove(index);
            steps.push(Step { from, to });
            continue;
        }
        let temporary = loop {
            spare += 1;
            let candidate = format!(".ncrs-rename-{spare}");
            if !used.contains(&key_of(&candidate, rules)) {
                break candidate;
            }
        };
        if let Some((from, _)) = pending.first_mut() {
            steps.push(Step {
                from: std::mem::replace(from, temporary.clone()),
                to: temporary,
            });
        }
    }
    steps
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const LOCAL: NameRules = NameRules {
        windows: false,
        fold_case: false,
    };
    const FOLDING: NameRules = NameRules {
        windows: false,
        fold_case: true,
    };

    fn day(year: i32, month: u32, date: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, date).unwrap()
    }

    fn facts(name: &str) -> EntryFacts {
        let (stem, extension) = split_name(name, false);
        EntryFacts {
            stem,
            extension,
            ..EntryFacts::default()
        }
    }

    fn options(pattern: &str) -> Options<'_> {
        Options {
            pattern,
            start: 1,
            digits: 3,
            find: "",
            replace: "",
            regex: false,
            case: Case::Keep,
        }
    }

    fn names_of(preview: &Preview) -> Vec<&str> {
        preview
            .rows
            .iter()
            .map(|row| row.new_name.as_str())
            .collect()
    }

    #[test]
    fn the_name_and_the_extension_tokens_rebuild_the_old_name() {
        let result = render(&options("{name}{extension}"), &facts("scan.final.pdf"), 0);
        assert_eq!(result, Ok("scan.final.pdf".to_owned()));
    }

    #[test]
    fn a_leading_dot_and_a_folder_have_no_extension() {
        assert_eq!(
            split_name(".env", false),
            (".env".to_owned(), String::new())
        );
        assert_eq!(split_name("v1.2", true), ("v1.2".to_owned(), String::new()));
    }

    #[test]
    fn part_of_the_name_counts_characters_from_one() {
        let entry = facts("holiday-2026.jpg");
        assert_eq!(
            render(&options("{name:1-7}{extension}"), &entry, 0),
            Ok("holiday.jpg".to_owned())
        );
        assert_eq!(
            render(&options("{name:9-12}"), &entry, 0),
            Ok("2026".to_owned())
        );
        assert_eq!(
            render(&options("{name:5-99}"), &entry, 0),
            Ok("day-2026".to_owned())
        );
    }

    #[test]
    fn a_range_that_is_no_range_stays_literal() {
        let entry = facts("a.txt");
        for pattern in ["{name:0-3}", "{name:4-2}", "{name:x-y}", "{nope}", "{name"] {
            assert_eq!(render(&options(pattern), &entry, 0), Ok(pattern.to_owned()));
        }
    }

    #[test]
    fn a_brace_before_a_token_is_literal() {
        assert_eq!(
            render(&options("{a{name}"), &facts("x.txt"), 0),
            Ok("{ax".to_owned())
        );
    }

    #[test]
    fn the_counter_starts_where_told_and_pads_to_the_digits() {
        let entry = facts("a.txt");
        let mut counted = options("{counter}");
        counted.start = 8;
        assert_eq!(render(&counted, &entry, 0), Ok("008".to_owned()));
        assert_eq!(render(&counted, &entry, 3), Ok("011".to_owned()));
        counted.digits = 1;
        assert_eq!(render(&counted, &entry, 3), Ok("11".to_owned()));
        counted.digits = 0;
        assert_eq!(render(&counted, &entry, 0), Ok("8".to_owned()));
    }

    #[test]
    fn dates_are_iso_and_taken_falls_back_to_modified() {
        let mut entry = facts("a.jpg");
        entry.modified = Some(day(2026, 3, 9));
        assert_eq!(
            render(&options("{date modified}"), &entry, 0),
            Ok("2026-03-09".to_owned())
        );
        assert_eq!(
            render(&options("{date taken}"), &entry, 0),
            Ok("2026-03-09".to_owned())
        );
        entry.taken = Some(day(2025, 12, 24));
        assert_eq!(
            render(&options("{date taken}"), &entry, 0),
            Ok("2025-12-24".to_owned())
        );
    }

    #[test]
    fn the_document_tokens_read_the_fields() {
        let mut entry = facts("scan.pdf");
        entry.sender = Some("Bank".to_owned());
        entry.kind = Some("Invoice".to_owned());
        entry.document_date = Some(day(2026, 1, 5));
        entry.amount = Some("12.34 EUR".to_owned());
        let pattern = "{year}-{month}-{day}_{document date}_{sender}_{kind}_{amount}{extension}";
        assert_eq!(
            render(&options(pattern), &entry, 0),
            Ok("2026-01-05_2026-01-05_Bank_Invoice_12.34 EUR.pdf".to_owned())
        );
    }

    #[test]
    fn a_missing_field_is_named() {
        let entry = facts("scan.pdf");
        assert_eq!(render(&options("{sender}"), &entry, 0), Err(Field::Sender));
        assert_eq!(
            render(&options("{year}"), &entry, 0),
            Err(Field::DocumentDate)
        );
        assert_eq!(render(&options("{kind}"), &entry, 0), Err(Field::Kind));
        assert_eq!(render(&options("{amount}"), &entry, 0), Err(Field::Amount));
        assert_eq!(
            render(&options("{date modified}"), &entry, 0),
            Err(Field::Modified)
        );
        let shown = preview(
            &options("{sender}"),
            &[entry],
            &["scan.pdf".to_owned()],
            LOCAL,
        );
        assert_eq!(shown.rows[0].problem, Some(Problem::Missing(Field::Sender)));
        assert!(!shown.can_apply());
    }

    #[test]
    fn plain_find_and_replace_works_on_the_result_of_the_pattern() {
        let mut changed = options("{name}{extension}");
        changed.find = "old";
        changed.replace = "new";
        assert_eq!(
            render(&changed, &facts("old-old.txt"), 0),
            Ok("new-new.txt".to_owned())
        );
        changed.find = ".";
        assert_eq!(
            render(&changed, &facts("a.txt"), 0),
            Ok("anewtxt".to_owned())
        );
    }

    #[test]
    fn a_regular_expression_may_use_groups() {
        let mut changed = options("{name}{extension}");
        changed.find = r"(\d+)-(\d+)";
        changed.replace = "$2-$1";
        changed.regex = true;
        assert_eq!(
            render(&changed, &facts("12-34.txt"), 0),
            Ok("34-12.txt".to_owned())
        );
    }

    #[test]
    fn an_invalid_expression_is_reported_and_replaces_nothing() {
        let mut changed = options("{name}{extension}");
        changed.find = "(";
        changed.regex = true;
        let shown = preview(&changed, &[facts("a.txt")], &["a.txt".to_owned()], LOCAL);
        assert!(shown.regex_error.is_some());
        assert!(!shown.can_apply());
        assert_eq!(names_of(&shown), vec!["a.txt"]);
        changed.regex = false;
        let plain = preview(&changed, &[facts("a.txt")], &["a.txt".to_owned()], LOCAL);
        assert_eq!(plain.regex_error, None);
    }

    #[test]
    fn case_comes_last_and_covers_the_whole_name() {
        let mut changed = options("{name}{extension}");
        changed.find = "a";
        changed.replace = "B";
        changed.case = Case::Lower;
        assert_eq!(
            render(&changed, &facts("AaA.TXT"), 0),
            Ok("aba.txt".to_owned())
        );
        changed.case = Case::Upper;
        assert_eq!(
            render(&changed, &facts("AaA.txt"), 0),
            Ok("ABA.TXT".to_owned())
        );
    }

    #[test]
    fn an_unchanged_name_is_fine_and_skipped() {
        let shown = preview(
            &options("{name}{extension}"),
            &[facts("a.txt"), facts("b.txt")],
            &["a.txt".to_owned(), "b.txt".to_owned()],
            LOCAL,
        );
        assert_eq!(shown.clashes(), 0);
        assert!(shown.pairs().is_empty());
    }

    #[test]
    fn two_entries_with_one_name_point_at_each_other() {
        let shown = preview(
            &options("same{extension}"),
            &[facts("a.txt"), facts("b.txt"), facts("c.pdf")],
            &["a.txt".to_owned(), "b.txt".to_owned(), "c.pdf".to_owned()],
            LOCAL,
        );
        assert_eq!(shown.rows[0].problem, Some(Problem::SameAs(2)));
        assert_eq!(shown.rows[1].problem, Some(Problem::SameAs(1)));
        assert_eq!(shown.rows[2].problem, None);
        assert_eq!(shown.clashes(), 2);
        assert!(!shown.can_apply());
    }

    #[test]
    fn a_name_of_an_entry_that_stays_exists() {
        let shown = preview(
            &options("other.txt"),
            &[facts("a.txt")],
            &["a.txt".to_owned(), "other.txt".to_owned()],
            LOCAL,
        );
        assert_eq!(shown.rows[0].problem, Some(Problem::Exists));
    }

    #[test]
    fn a_name_of_an_entry_that_is_renamed_away_is_free() {
        let mut first = options("{name}{extension}");
        first.find = "a";
        first.replace = "b";
        let shown = preview(
            &first,
            &[facts("a.txt"), facts("b.txt")],
            &["a.txt".to_owned(), "b.txt".to_owned()],
            LOCAL,
        );
        assert_eq!(shown.rows[0].new_name, "b.txt");
        assert_eq!(shown.rows[0].problem, Some(Problem::Exists));
        let swap = preview(
            &options("{counter}"),
            &[facts("002"), facts("001")],
            &["002".to_owned(), "001".to_owned()],
            LOCAL,
        );
        assert_eq!(swap.clashes(), 0);
        assert_eq!(names_of(&swap), vec!["001", "002"]);
    }

    #[test]
    fn names_that_differ_in_case_clash_only_where_the_file_system_folds_case() {
        let entries = [facts("a.txt"), facts("b.txt")];
        let folder = ["a.txt".to_owned(), "b.txt".to_owned(), "Doc.txt".to_owned()];
        let mut pattern = options("doc.txt");
        let local = preview(&pattern, &entries, &folder, LOCAL);
        assert_eq!(local.rows[0].problem, Some(Problem::SameAs(2)));
        assert_eq!(local.rows[1].problem, Some(Problem::SameAs(1)));
        pattern.pattern = "DOC.txt";
        let one = preview(&pattern, &entries[..1], &folder, LOCAL);
        assert_eq!(one.rows[0].problem, None);
        let folding = preview(&pattern, &entries[..1], &folder, FOLDING);
        assert_eq!(folding.rows[0].problem, Some(Problem::Exists));
        let both = preview(&options("X.txt"), &entries, &folder, FOLDING);
        assert_eq!(both.rows[0].problem, Some(Problem::SameAs(2)));
        let mixed = [facts("Y.txt"), facts("y.txt")];
        let case_only = preview(
            &options("{name}{extension}"),
            &mixed,
            &["Y.txt".to_owned(), "y.txt".to_owned()],
            FOLDING,
        );
        assert_eq!(case_only.clashes(), 0);
    }

    #[test]
    fn a_change_of_case_only_is_no_clash_with_itself() {
        let mut upper = options("{name}{extension}");
        upper.case = Case::Upper;
        let shown = preview(&upper, &[facts("a.txt")], &["a.txt".to_owned()], FOLDING);
        assert_eq!(shown.rows[0].new_name, "A.TXT");
        assert_eq!(shown.rows[0].problem, None);
    }

    #[test]
    fn a_name_the_system_refuses_is_a_problem() {
        let shown = preview(
            &options("a/b"),
            &[facts("a.txt")],
            &["a.txt".to_owned()],
            LOCAL,
        );
        assert_eq!(
            shown.rows[0].problem,
            Some(Problem::Name(NameProblem::Separator))
        );
        let windows = NameRules {
            windows: true,
            fold_case: true,
        };
        let colon = preview(
            &options("a:b"),
            &[facts("a.txt")],
            &["a.txt".to_owned()],
            windows,
        );
        assert_eq!(
            colon.rows[0].problem,
            Some(Problem::Name(NameProblem::Character(':')))
        );
        let empty = preview(
            &options(""),
            &[facts("a.txt")],
            &["a.txt".to_owned()],
            LOCAL,
        );
        assert_eq!(
            empty.rows[0].problem,
            Some(Problem::Name(NameProblem::Empty))
        );
    }

    fn pair(from: &str, to: &str) -> (String, String) {
        (from.to_owned(), to.to_owned())
    }

    fn step(from: &str, to: &str) -> Step {
        Step {
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    #[test]
    fn independent_renames_keep_their_order() {
        let steps = plan(
            &[pair("a", "x"), pair("b", "y")],
            &["a".into(), "b".into()],
            LOCAL,
        );
        assert_eq!(steps, vec![step("a", "x"), step("b", "y")]);
    }

    #[test]
    fn a_target_that_is_another_source_waits_for_it() {
        let steps = plan(
            &[pair("a", "b"), pair("b", "c")],
            &["a".into(), "b".into()],
            LOCAL,
        );
        assert_eq!(steps, vec![step("b", "c"), step("a", "b")]);
    }

    #[test]
    fn a_swap_goes_through_a_temporary_name() {
        let steps = plan(
            &[pair("a", "b"), pair("b", "a")],
            &["a".into(), "b".into()],
            LOCAL,
        );
        assert_eq!(
            steps,
            vec![
                step("a", ".ncrs-rename-1"),
                step("b", "a"),
                step(".ncrs-rename-1", "b"),
            ]
        );
    }

    #[test]
    fn a_temporary_name_avoids_the_names_of_the_folder() {
        let taken = [".ncrs-rename-1".to_owned(), "a".to_owned(), "b".to_owned()];
        let steps = plan(&[pair("a", "b"), pair("b", "a")], &taken, LOCAL);
        assert_eq!(steps.first(), Some(&step("a", ".ncrs-rename-2")));
    }

    #[test]
    fn a_ring_of_three_is_broken_once() {
        let steps = plan(
            &[pair("a", "b"), pair("b", "c"), pair("c", "a")],
            &["a".into(), "b".into(), "c".into()],
            LOCAL,
        );
        assert_eq!(steps.len(), 4);
        assert_eq!(steps.first(), Some(&step("a", ".ncrs-rename-1")));
    }

    #[test]
    fn undoing_the_pairs_reverses_every_one() {
        let done = [pair("a", "b"), pair("b", "c")];
        let back: Vec<(String, String)> = done
            .iter()
            .map(|(from, to)| (to.clone(), from.clone()))
            .collect();
        let steps = plan(&back, &["b".into(), "c".into()], LOCAL);
        assert_eq!(steps, vec![step("b", "a"), step("c", "b")]);
    }
}
