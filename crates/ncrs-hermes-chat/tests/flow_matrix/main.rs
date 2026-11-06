//! Every way through the change of the levels, against the real local model on
//! the test archive. Needs the model: `NCRS_AI_MODELS_DIR` names the folder
//! with it. The model's answers are kept in `target/flow-matrix-answers.tsv`,
//! keyed on what it was asked and on the sources of the prompts, so a run
//! without a change to either does not ask again.

#![cfg(feature = "llama")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

#[path = "../common/mod.rs"]
mod common;
mod rules;

use rules::{
    change_the_condition, create_and_check, only_new, saved_twice_for_the_same_folder,
    saved_twice_with_a_new_level_word,
};

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{Datelike, NaiveDate};
use common::{count, say, Talk};
use ncrs_ai::{
    parse_levels, parse_revision, AiError, ChatIntent, ChatModel, ChatTurn, ContextDocument,
    LazyLocalModel, Levels, LocalModelStatus, Mismatch, Revision, LOCAL_MODEL,
};
use ncrs_hermes_chat::{ChipReply, LevelsCard, Message, Part};
use ncrs_i18n::Msg;
use ncrs_index::{DocumentFields, Kinds, RuleOrigin, RuleSpec};
use ncrs_ui_kit::hermes_kit::MoveRow;

const MODELS_DIR_VARIABLE: &str = "NCRS_AI_MODELS_DIR";
const SENDER: &str = "Cosmicdrift GmbH";
const AREA_PREFIX: &str = "Arbeit/";
const FOUND: [&str; 3] = ["Gehalt", "{year}", "Gehaltsabrechnung_{date}"];
const EXAMPLES_SHOWN: usize = 3;

/// What the prompts are made of: a change in one of them asks the model again.
const PROMPT_SOURCES: [&str; 6] = [
    include_str!("../../../ncrs-ai/src/rules/revise.rs"),
    include_str!("../../../ncrs-ai/src/rules/prompt.rs"),
    include_str!("../../../ncrs-ai/src/rules/grammar.rs"),
    include_str!("../../../ncrs-ai/src/rules/session.rs"),
    include_str!("../../../ncrs-ai/src/rules/annotate.rs"),
    include_str!("../../../ncrs-ai/src/local/lazy.rs"),
];

/// The real model, with its answers to the rule questions kept on disk.
#[derive(Debug)]
struct CachedModel {
    inner: Arc<LazyLocalModel>,
    store: PathBuf,
    answers: Mutex<BTreeMap<String, String>>,
    asked: Mutex<Vec<String>>,
}

impl CachedModel {
    fn new(inner: Arc<LazyLocalModel>) -> Self {
        let store =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/flow-matrix-answers.tsv");
        let answers = std::fs::read_to_string(&store)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(key, answer)| (key.to_owned(), answer.to_owned()))
            .collect();
        Self {
            inner,
            store,
            answers: Mutex::new(answers),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn answer(
        &self,
        question: String,
        ask: impl FnOnce() -> Result<String, AiError>,
    ) -> Result<String, AiError> {
        let mut hasher = DefaultHasher::new();
        PROMPT_SOURCES.hash(&mut hasher);
        question.hash(&mut hasher);
        let key = format!("{:016x}", hasher.finish());
        let known = self.answers.lock().unwrap().get(&key).cloned();
        let answer = match known {
            Some(answer) => answer,
            None => {
                let fresh = ask()?;
                let mut answers = self.answers.lock().unwrap();
                answers.insert(key, fresh.clone());
                let lines: Vec<String> = answers
                    .iter()
                    .map(|(key, answer)| format!("{key}\t{answer}"))
                    .collect();
                std::fs::write(&self.store, lines.join("\n")).unwrap();
                fresh
            }
        };
        self.asked.lock().unwrap().push(answer.clone());
        Ok(answer)
    }

    fn take_asked(&self) -> Vec<String> {
        std::mem::take(&mut *self.asked.lock().unwrap())
    }
}

fn levels_json(levels: &Levels) -> String {
    format!("{{{}}}", levels.json_fields())
}

impl ChatModel for CachedModel {
    fn understand(&self, turn: &ChatTurn) -> Result<ChatIntent, AiError> {
        self.inner.understand(turn)
    }

    fn generate_rule(
        &self,
        description: &str,
        documents: &[ContextDocument],
        kinds: &Kinds,
    ) -> Result<RuleSpec, AiError> {
        let spec = self.inner.generate_rule(description, documents, kinds)?;
        self.asked
            .lock()
            .unwrap()
            .push(format!("rule {:?}", spec.conditions.list));
        Ok(spec)
    }

    fn recognize_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
    ) -> Result<Levels, AiError> {
        let raw = self.answer(format!("recognize {files:?} {folder_depth:?}"), || {
            self.inner
                .recognize_levels(files, kinds, folder_depth)
                .map(|levels| levels_json(&levels))
        })?;
        parse_levels(&raw)
    }

    fn recognize_levels_again(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        folder_depth: Option<usize>,
        previous: &Levels,
        misfits: &[Mismatch],
    ) -> Result<Levels, AiError> {
        let question = format!("again {files:?} {folder_depth:?} {previous:?} {misfits:?}");
        let raw = self.answer(question, || {
            self.inner
                .recognize_levels_again(files, kinds, folder_depth, previous, misfits)
                .map(|levels| levels_json(&levels))
        })?;
        parse_levels(&raw)
    }

    fn revise_levels(
        &self,
        files: &[(String, DocumentFields)],
        kinds: &Kinds,
        previous: &Levels,
        sentence: &str,
    ) -> Result<Revision, AiError> {
        let raw = self.answer(format!("revise {files:?} {previous:?} {sentence}"), || {
            self.inner
                .revise_levels(files, kinds, previous, sentence)
                .map(|revision| {
                    format!(
                        "{{{},\"fertig\":{}}}",
                        revision.levels.json_fields(),
                        revision.done
                    )
                })
        })?;
        parse_revision(&raw)
    }

    fn status(&self) -> LocalModelStatus {
        self.inner.status()
    }

    fn refresh(&self) {
        self.inner.refresh();
    }
}

/// A document of the fixture: where it lies below the area, and its date.
#[derive(Debug, Clone)]
struct Doc {
    /// `Gehalt/2023/Gehaltsabrechnung_2023-05-25.pdf` or the name in the inbox.
    in_area: String,
    date: NaiveDate,
    inbox: bool,
}

/// The pattern below the area written for the document's date.
fn render(pattern: &str, date: NaiveDate) -> String {
    let path = pattern
        .replace("{year}", &format!("{:04}", date.year()))
        .replace("{month}", &format!("{:02}", date.month()))
        .replace("{day}", &format!("{:02}", date.day()))
        .replace("{date}", &date.format("%Y-%m-%d").to_string());
    format!("{path}.pdf")
}

/// The test archive with sender and date on the pay slips, and five pay slips
/// in the inbox.
fn fixture(model: &Arc<CachedModel>) -> (Talk, Vec<Doc>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("archive")).unwrap();
    let archive = ncrs_intake::test_archive::test_archive();
    let talk = Talk::in_dir(dir, &["archive"], &["inbox"])
        .with_real_model(Arc::clone(model) as Arc<dyn ChatModel>)
        .with_rules();
    let mut docs = Vec::new();
    for file in &archive.archive {
        let Some(text) = file.text() else { continue };
        if !file.relative_path.starts_with("Arbeit/Gehalt") {
            continue;
        }
        let relative = format!("archive/{}", file.relative_path.display());
        let id = talk.index_document(&relative, text);
        let pay_slip = relative.starts_with("archive/Arbeit/Gehalt/");
        talk.set_fields(
            id,
            &DocumentFields {
                sender: pay_slip.then(|| SENDER.to_owned()),
                date: file.date,
                ..DocumentFields::default()
            },
        );
        if pay_slip {
            docs.push(Doc {
                in_area: relative.trim_start_matches("archive/Arbeit/").to_owned(),
                date: file.date.unwrap(),
                inbox: false,
            });
        }
    }
    for month in 1..=5 {
        let date = NaiveDate::from_ymd_opt(2025, month, 25).unwrap();
        let name = format!("Eingang_{month}.pdf");
        let id = talk.index_inbox_document(
            &format!("inbox/{name}"),
            &common::letter(
                SENDER,
                &date.format("%d.%m.%Y").to_string(),
                "Gehaltsabrechnung",
                month,
            ),
        );
        talk.set_fields(
            id,
            &DocumentFields {
                sender: Some(SENDER.to_owned()),
                date: Some(date),
                ..DocumentFields::default()
            },
        );
        docs.push(Doc {
            in_area: name,
            date,
            inbox: true,
        });
    }
    (talk, docs)
}

fn levels_card(talk: &Talk) -> &LevelsCard {
    talk.last_parts()
        .iter()
        .find_map(|part| match part {
            Part::Levels(card) => Some(card),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no card of levels: {:?}", talk.last_texts()))
}

/// What the shown values of a level look like: digits stand for any digit.
fn shape_of_pattern(level: &str) -> String {
    level
        .replace("{date}", "9999-99-99")
        .replace("{year}", "9999")
        .replace("{month}", "99")
        .replace("{day}", "99")
}

fn shape_of_text(text: &str) -> Vec<String> {
    text.split(", ")
        .map(|name| {
            name.strip_suffix(".pdf")
                .unwrap_or(name)
                .chars()
                .map(|ch| if ch.is_ascii_digit() { '9' } else { ch })
                .collect()
        })
        .collect()
}

/// The card shows exactly `expected` as levels, and marks the ones that are
/// not what was found.
fn check_levels(card: &LevelsCard, expected: &[&str]) {
    assert_eq!(card.rows.len(), expected.len(), "{:?}", card.rows);
    for (index, level) in expected.iter().enumerate() {
        let row = &card.rows[index];
        let shape = shape_of_pattern(level);
        for seen in shape_of_text(&row.text) {
            assert_eq!(seen, shape, "level {} of {expected:?}: {row:?}", index + 1);
        }
        let changed = FOUND.get(index) != Some(level);
        assert_eq!(
            row.before.is_some(),
            changed,
            "level {}: {row:?}",
            index + 1
        );
    }
}

fn expect_chips(talk: &Talk, expected: &[Msg]) {
    let mut wanted: Vec<String> = expected.iter().map(|msg| say(*msg, &[])).collect();
    let mut shown = talk.labels();
    wanted.sort();
    shown.sort();
    assert_eq!(shown, wanted, "chips");
}

const FOUND_CHIPS: [Msg; 3] = [
    Msg::HermesChatLevelsKeep,
    Msg::HermesChatLevelsChange,
    Msg::HermesChatLevelsDefault,
];
const CHANGE_CHIPS: [Msg; 4] = [
    Msg::HermesChatLevelsApply,
    Msg::HermesChatLevelsMore,
    Msg::HermesChatLevelsDiscard,
    Msg::HermesChatLevelsDone,
];

/// The row is "new path" over "before: old path" of one of `docs` written by
/// `pattern`.
fn check_example(row: &MoveRow, docs: &[&Doc], pattern: &str) {
    let doc = docs
        .iter()
        .find(|doc| row.before.ends_with(&doc.in_area))
        .unwrap_or_else(|| panic!("no document for {row:?}"));
    assert!(row.to.ends_with(&render(pattern, doc.date)), "{row:?}");
}

/// After a change: the levels marked, what applying does to the archive's
/// files, the buttons, and no word of fitting.
fn check_change(talk: &Talk, docs: &[Doc], pattern: &str) {
    let levels: Vec<&str> = pattern.split('/').collect();
    let card = levels_card(talk);
    check_levels(card, &levels);
    assert!(card.footer.is_none(), "{:?}", card.footer);
    expect_chips(talk, &CHANGE_CHIPS);
    let joined = talk.last_texts().join("\n");
    assert!(
        !joined.contains("passt bei") && !joined.contains(" soll "),
        "{joined}"
    );
    let moving: Vec<&Doc> = docs
        .iter()
        .filter(|doc| !doc.inbox && render(pattern, doc.date) != doc.in_area)
        .collect();
    let Some(block) = card.moves.as_ref() else {
        assert!(moving.is_empty(), "no renames in the card: {joined}");
        return;
    };
    assert_eq!(
        block.header,
        say(
            Msg::HermesChatLevelsRenames,
            &[("count", &count(moving.len()))]
        )
    );
    let shown = moving.len().min(EXAMPLES_SHOWN);
    assert_eq!(block.rows.len(), shown, "{:?}", block.rows);
    for row in &block.rows {
        check_example(row, &moving, pattern);
    }
    let more = say(
        Msg::HermesChatRuleExamplesMore,
        &[("count", &count(moving.len() - shown))],
    );
    assert_eq!(block.more, (moving.len() > shown).then_some(more));
}

/// The archive documents `pattern` writes to another path.
fn archive_moves<'a>(docs: &'a [Doc], pattern: &str) -> Vec<&'a Doc> {
    docs.iter()
        .filter(|doc| !doc.inbox && render(pattern, doc.date) != doc.in_area)
        .collect()
}

/// The dry run: counts, examples (the inbox first, then the archive), the
/// button to create the rule. The archive renames are the ones the change
/// preview announced.
fn check_trial(talk: &Talk, docs: &[Doc], pattern: &str) {
    let inbox: Vec<&Doc> = docs.iter().filter(|doc| doc.inbox).collect();
    let in_archive = docs.len() - inbox.len();
    let renamed = archive_moves(docs, pattern);
    let moves = inbox.len() + renamed.len();
    let card = talk.proposal().expect("the proposal card");
    assert!(
        card.facts.iter().any(|fact| fact.value
            == say(
                Msg::HermesChatProposalMatches,
                &[("count", &count(docs.len()))]
            )),
        "matches: {:?}",
        card.facts
    );
    let shown = moves.min(EXAMPLES_SHOWN);
    let more = say(
        Msg::HermesChatRuleExamplesMore,
        &[("count", &count(moves - shown))],
    );
    let counts = say(
        Msg::HermesChatRuleTrialCounts,
        &[
            ("right", &count(in_archive - renamed.len())),
            ("moves", &count(moves)),
            ("inbox", &count(inbox.len())),
        ],
    );
    let rows = card
        .moves
        .as_ref()
        .map(|block| block.rows.as_slice())
        .unwrap_or_default();
    assert_eq!(rows.len(), shown, "{rows:?}");
    let sources: Vec<&Doc> = inbox
        .iter()
        .copied()
        .chain(renamed.iter().copied())
        .collect();
    for (row, doc) in rows.iter().zip(&sources) {
        check_example(row, &[doc], pattern);
    }
    let shown_more = card.moves.as_ref().and_then(|block| block.more.clone());
    assert_eq!(shown_more, (moves > shown).then_some(more));
    assert_eq!(card.counts, Some(counts));
    assert!(
        talk.state
            .chips()
            .iter()
            .any(|chip| chip.reply == ChipReply::RuleCreate),
        "{:?}",
        talk.labels()
    );
}

fn has_levels_card(talk: &Talk) -> bool {
    talk.last_parts()
        .iter()
        .any(|part| matches!(part, Part::Levels(_)))
}

/// Opens the folder the way the app does, by a typed sentence, and checks the
/// levels found.
fn entered(model: &Arc<CachedModel>, english: bool) -> (Talk, Vec<Doc>) {
    let (mut talk, docs) = fixture(model);
    talk.send(Message::Open);
    talk.type_text(if english {
        "open /Arbeit"
    } else {
        "öffne /Arbeit"
    });
    assert!(
        has_levels_card(&talk),
        "{:?} {:?}",
        talk.last_texts(),
        talk.labels()
    );
    check_levels(levels_card(&talk), &FOUND);
    expect_chips(&talk, &FOUND_CHIPS);
    (talk, docs)
}

fn found_pattern() -> String {
    FOUND.join("/")
}

struct Change {
    name: &'static str,
    german: &'static str,
    english: &'static str,
    pattern: &'static str,
}

const RENAME: (&str, &str) = ("nenne Ebene 1 Lohn statt Gehalt", "rename level 1 to Lohn");
const MONTH: (&str, &str) = (
    "Jahr und Monat zusammen in Ebene 2, mit Bindestrich",
    "put year and month together in level 2, with a dash",
);
const DATE_FIRST: (&str, &str) = ("Datum vorne bei Ebene 3", "put the date first in level 3");

const CHANGES: [Change; 7] = [
    Change {
        name: "1 rename",
        german: RENAME.0,
        english: RENAME.1,
        pattern: "Lohn/{year}/Gehaltsabrechnung_{date}",
    },
    Change {
        name: "2 year-month",
        german: MONTH.0,
        english: MONTH.1,
        pattern: "Gehalt/{year}-{month}/Gehaltsabrechnung_{date}",
    },
    Change {
        name: "3 date first",
        german: DATE_FIRST.0,
        english: DATE_FIRST.1,
        pattern: "Gehalt/{year}/{date}_Gehaltsabrechnung",
    },
    Change {
        name: "1+2",
        german: "nenne Ebene 1 Lohn und mach Jahr und Monat zusammen in Ebene 2, mit Bindestrich",
        english: "rename level 1 to Lohn and put year and month together in level 2, with a dash",
        pattern: "Lohn/{year}-{month}/Gehaltsabrechnung_{date}",
    },
    Change {
        name: "1+3",
        german: "nenne Ebene 1 Lohn und setz das Datum vorne bei Ebene 3",
        english: "rename level 1 to Lohn and put the date first in level 3",
        pattern: "Lohn/{year}/{date}_Gehaltsabrechnung",
    },
    Change {
        name: "2+3",
        german: "Jahr und Monat zusammen in Ebene 2, mit Bindestrich, und das Datum vorne bei Ebene 3",
        english: "put year and month together in level 2, with a dash, and the date first in level 3",
        pattern: "Gehalt/{year}-{month}/{date}_Gehaltsabrechnung",
    },
    Change {
        name: "1+2+3",
        german: "nenne Ebene 1 Lohn, mach Jahr und Monat zusammen in Ebene 2, mit Bindestrich, und setz das Datum vorne bei Ebene 3",
        english: "rename level 1 to Lohn, put year and month together in level 2, with a dash, and the date first in level 3",
        pattern: "Lohn/{year}-{month}/{date}_Gehaltsabrechnung",
    },
];

const LEAVE_THE_REST: (&str, &str) = ("den Rest so lassen", "leave the rest as it is");
const NOTHING_MORE: (&str, &str) = ("fertig", "that is all");

fn pick(english: bool, pair: (&'static str, &'static str)) -> &'static str {
    if english {
        pair.1
    } else {
        pair.0
    }
}

fn change_then_apply(model: &Arc<CachedModel>, change: &Change, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    expect_chips(&talk, &[Msg::HermesChatLevelsDone]);
    assert!(!has_levels_card(&talk), "only the hint, no card again");
    assert_eq!(
        talk.last_texts(),
        vec![say(Msg::HermesChatLevelsChangeHint, &[])]
    );
    talk.type_text(pick(english, (change.german, change.english)));
    check_change(&talk, &docs, change.pattern);
    talk.press(&ChipReply::LevelsApply);
    check_trial(&talk, &docs, change.pattern);
    create_and_check(&mut talk, &docs, change.pattern);
}

fn looks_right(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsKeep);
    check_trial(&talk, &docs, &found_pattern());
    create_and_check(&mut talk, &docs, &found_pattern());
}

fn standard(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsDefault);
    assert_eq!(levels_card(&talk).rows.len(), 2);
    expect_chips(&talk, &CHANGE_CHIPS);
    let joined = talk.last_texts().join("\n");
    assert!(
        !joined.contains("passt bei") && !joined.contains(" soll "),
        "{joined}"
    );
    talk.press(&ChipReply::LevelsApply);
    let card = talk.proposal().expect("the proposal card");
    assert!(
        card.facts.iter().any(|fact| fact.value
            == say(
                Msg::HermesChatProposalMatches,
                &[("count", &count(docs.len()))]
            )),
        "{:?}",
        card.facts
    );
    talk.press(&ChipReply::RuleCreate);
    assert_eq!(
        talk.rules()[0].spec.filing_pattern,
        format!("{AREA_PREFIX}{{year}}/{{date}}_{{sender}}_{{kind}}")
    );
}

fn fertig_at_once(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    expect_chips(&talk, &[Msg::HermesChatLevelsDone]);
    talk.press(&ChipReply::LevelsDone);
    check_trial(&talk, &docs, &found_pattern());
    create_and_check(&mut talk, &docs, &found_pattern());
}

fn done_sentence_without_change(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, NOTHING_MORE));
    check_trial(&talk, &docs, &found_pattern());
    create_and_check(&mut talk, &docs, &found_pattern());
}

fn change_then_discard(model: &Arc<CachedModel>, english: bool) {
    let change = &CHANGES[0];
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, (change.german, change.english)));
    check_change(&talk, &docs, change.pattern);
    talk.press(&ChipReply::LevelsDiscard);
    check_levels(levels_card(&talk), &FOUND);
    expect_chips(&talk, &FOUND_CHIPS);
    talk.press(&ChipReply::LevelsKeep);
    check_trial(&talk, &docs, &found_pattern());
    create_and_check(&mut talk, &docs, &found_pattern());
}

fn change_then_leave_the_rest(model: &Arc<CachedModel>, english: bool) {
    let change = &CHANGES[0];
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, (change.german, change.english)));
    check_change(&talk, &docs, change.pattern);
    talk.type_text(pick(english, LEAVE_THE_REST));
    check_trial(&talk, &docs, change.pattern);
    create_and_check(&mut talk, &docs, change.pattern);
}

fn change_then_change_more(model: &Arc<CachedModel>, english: bool) {
    let (mut talk, docs) = entered(model, english);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(pick(english, RENAME));
    check_change(&talk, &docs, CHANGES[0].pattern);
    talk.press(&ChipReply::LevelsMore);
    talk.type_text(pick(english, DATE_FIRST));
    let pattern = "Lohn/{year}/{date}_Gehaltsabrechnung";
    check_change(&talk, &docs, pattern);
    talk.press(&ChipReply::LevelsApply);
    check_trial(&talk, &docs, pattern);
    create_and_check(&mut talk, &docs, pattern);
}

/// How much of Marc's two pay slips the index knows.
#[derive(Debug, Clone, Copy)]
enum Indexed {
    Both,
    First,
    Neither,
}

const MARC_SLIPS: [(&str, u32, u32, u32); 2] = [
    ("Gehalt/2023/Gehaltsabrechnung_2023-05-25.pdf", 2023, 5, 25),
    ("Gehalt/2024/Gehaltsabrechnung_2024-12-25.pdf", 2024, 12, 25),
];

/// An Arbeit folder with two pay slips only, as in the app of the report; the
/// index knows `indexed` of them, the others lie on the disk alone.
fn marc_fixture(model: &Arc<CachedModel>, indexed: Indexed) -> (Talk, Vec<Doc>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("archive")).unwrap();
    let talk = Talk::in_dir(dir, &["archive"], &["inbox"])
        .with_real_model(Arc::clone(model) as Arc<dyn ChatModel>)
        .with_rules();
    let mut docs = Vec::new();
    for (position, (in_area, year, month, day)) in MARC_SLIPS.into_iter().enumerate() {
        let date = NaiveDate::from_ymd_opt(year as i32, month, day).unwrap();
        let relative = format!("archive/Arbeit/{in_area}");
        let text = common::letter(
            SENDER,
            &date.format("%d.%m.%Y").to_string(),
            "Gehaltsabrechnung",
            position as u32 + 1,
        );
        let known = match indexed {
            Indexed::Both => true,
            Indexed::First => position == 0,
            Indexed::Neither => false,
        };
        if known {
            let id = talk.index_document(&relative, &text);
            talk.set_fields(
                id,
                &DocumentFields {
                    sender: Some(SENDER.to_owned()),
                    date: Some(date),
                    ..DocumentFields::default()
                },
            );
        } else {
            talk.write(&relative, &text);
        }
        docs.push(Doc {
            in_area: in_area.to_owned(),
            date,
            inbox: false,
        });
    }
    (talk, docs)
}

const BARE_LEVEL: &str = "3";
const DATE_FIRST_AT_3: (&str, &str) = (
    "mach bei 3 das datum vorne hin",
    "put the date first in level 3",
);

/// The path of the report: the archive opened, the area picked, "Change",
/// a bare "3", then the sentence; the trial counts the files the preview
/// listed whatever the index knows.
fn marc_path(model: &Arc<CachedModel>, english: bool, indexed: Indexed) {
    let (mut talk, docs) = marc_fixture(model, indexed);
    talk.send(Message::Open);
    let archive_root = talk.path("archive").display().to_string();
    talk.type_text(&if english {
        format!("open {archive_root}")
    } else {
        format!("öffne {archive_root}")
    });
    talk.press(&ChipReply::Folder(talk.path("archive/Arbeit")));
    check_levels(levels_card(&talk), &FOUND);
    expect_chips(&talk, &FOUND_CHIPS);
    talk.press(&ChipReply::LevelsChange);
    talk.type_text(BARE_LEVEL);
    assert_eq!(
        talk.last_texts(),
        vec![say(Msg::HermesChatLevelsAskLevel, &[("level", "3")])]
    );
    expect_chips(&talk, &[Msg::HermesChatLevelsDone]);
    talk.type_text(pick(english, DATE_FIRST_AT_3));
    let pattern = "Gehalt/{year}/{date}_Gehaltsabrechnung";
    check_change(&talk, &docs, pattern);
    talk.press(&ChipReply::LevelsApply);
    check_trial(&talk, &docs, pattern);
    create_and_check(&mut talk, &docs, pattern);
}

fn message_of(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_default()
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_level_change_matrix() {
    let folder = PathBuf::from(std::env::var_os(MODELS_DIR_VARIABLE).expect(MODELS_DIR_VARIABLE));
    let model_file = folder.join(LOCAL_MODEL.name);
    assert!(model_file.is_file(), "{} is missing", model_file.display());
    let local = LazyLocalModel::shared(
        model_file,
        LOCAL_MODEL,
        NonZeroU32::new(4).unwrap(),
        NonZeroU32::new(4096).unwrap(),
    );
    let model = Arc::new(CachedModel::new(Arc::clone(&local)));

    type Path = Box<dyn Fn(&Arc<CachedModel>, bool)>;
    let mut cases: Vec<(String, Path)> = Vec::new();
    for change in CHANGES {
        cases.push((
            format!("change {}", change.name),
            Box::new(move |model, english| change_then_apply(model, &change, english)),
        ));
    }
    let paths: [(&str, fn(&Arc<CachedModel>, bool)); 15] = [
        ("save, no: only new ones", only_new),
        ("change the condition in a sentence", change_the_condition),
        (
            "save twice for the same folder",
            saved_twice_for_the_same_folder,
        ),
        (
            "save twice for the same folder, new level word",
            saved_twice_with_a_new_level_word,
        ),
        ("report path, index knows both", |model, english| {
            marc_path(model, english, Indexed::Both);
        }),
        ("report path, index knows one", |model, english| {
            marc_path(model, english, Indexed::First);
        }),
        ("report path, index knows none", |model, english| {
            marc_path(model, english, Indexed::Neither);
        }),
        ("looks right", looks_right),
        ("use the standard", standard),
        ("change, done at once", fertig_at_once),
        ("change, done by sentence", done_sentence_without_change),
        ("change, discard", change_then_discard),
        ("change, leave the rest", change_then_leave_the_rest),
        ("change, change more", change_then_change_more),
        ("change, apply (rename)", |model, english| {
            change_then_apply(model, &CHANGES[0], english);
        }),
    ];
    for (name, path) in paths {
        cases.push((name.to_owned(), Box::new(path)));
    }

    let mut table = Vec::new();
    let mut failed = 0;
    for (name, run) in &cases {
        for english in [false, true] {
            model.take_asked();
            let outcome = catch_unwind(AssertUnwindSafe(|| run(&model, english)));
            let language = if english { "en" } else { "de" };
            let answers = model.take_asked().join(" | ");
            let verdict = match outcome {
                Ok(()) => "pass".to_owned(),
                Err(panic) => {
                    failed += 1;
                    format!("FAIL {}", message_of(&*panic))
                }
            };
            table.push(format!(
                "{name} [{language}]: {verdict}\n    model: {answers}"
            ));
        }
    }
    println!("\n{}\n", table.join("\n"));
    drop(model);
    local.shutdown();
    assert_eq!(failed, 0, "{failed} of {} runs failed", table.len());
}
