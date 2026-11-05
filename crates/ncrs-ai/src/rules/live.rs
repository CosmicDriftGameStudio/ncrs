//! The rule matrix against the real model, through the session the app uses.
//! It reports and asserts nothing about the rates: they are measured first.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::integer_division
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use ncrs_index::{DocumentFields, Kinds};

use super::annotate::annotated_paths;
use super::cache::{cache_key, ResultCache};
use super::holdout::holdout_cases;
use super::matrix::{all_cases, archive_kinds, fixtures, Ask, Axis, Case, Expected, Language};
use super::oracle::check_answer;
use super::parse::{parse_generate, parse_pattern};
use super::prompt::{prefix_text, ContextDocument};
use super::session::RuleRequest;
use crate::chat::shard::{parse_shard, select, SHARD_VARIABLE};
use crate::local::{models_dir, LocalModel, LocalModelConfig, RuleSession};
use crate::model_file::LOCAL_MODEL;

const THREADS: u32 = 4;
const CONTEXT_TOKENS: u32 = 4096;
const LIMIT_VARIABLE: &str = "NCRS_RULE_LIMIT";
const CACHE_VARIABLE: &str = "NCRS_RULE_CACHE";

fn default_cache_path() -> PathBuf {
    std::env::var_os(CACHE_VARIABLE).map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/rule-matrix-cache.jsonl"),
        PathBuf::from,
    )
}

fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::Folder => "folder",
        Axis::FileName => "file name",
        Axis::Condition => "condition",
        Axis::Recognize => "recognize",
        Axis::Default => "default",
        Axis::Holdout => "holdout",
    }
}

fn language_name(language: Language) -> &'static str {
    match language {
        Language::German => "de",
        Language::English => "en",
    }
}

fn context_documents() -> Vec<ContextDocument> {
    fixtures()
        .into_iter()
        .map(|fixture| ContextDocument {
            fields: fixture.fields,
            folder: Some(fixture.folder.to_owned()),
        })
        .collect()
}

fn request_of(case: &Case, prefix: &str, kinds: &Kinds) -> RuleRequest {
    match &case.ask {
        Ask::Describe(description) => RuleRequest::generate(prefix, description),
        Ask::Paths { question, paths } => {
            let files: Vec<(String, DocumentFields)> = paths
                .iter()
                .zip(fixtures())
                .map(|(path, fixture)| (path.clone(), fixture.fields))
                .collect();
            RuleRequest::recognize(prefix, question, &annotated_paths(&files, kinds))
        }
    }
}

fn model() -> &'static LocalModel {
    static MODEL: OnceLock<LocalModel> = OnceLock::new();
    MODEL.get_or_init(|| {
        let path = models_dir().join(LOCAL_MODEL.name);
        assert!(
            path.is_file(),
            "{} is missing; set NCRS_AI_MODELS_DIR to the folder with the model",
            path.display()
        );
        LocalModel::load(
            LocalModelConfig {
                path,
                threads: NonZeroU32::new(THREADS).unwrap(),
                context_tokens: NonZeroU32::new(CONTEXT_TOKENS).unwrap(),
            },
            &LOCAL_MODEL,
        )
        .unwrap_or_else(|error| panic!("{error}"))
    })
}

/// The shard of the chat evaluation, then an even sample of `NCRS_RULE_LIMIT`
/// cases across all axes.
fn chosen_cases() -> Vec<Case> {
    let mut cases = all_cases();
    cases.extend(holdout_cases());
    if let Ok(value) = std::env::var(SHARD_VARIABLE) {
        let shard = parse_shard(&value).unwrap_or_else(|message| panic!("{message}"));
        cases = select(cases, shard);
    }
    if let Some(limit) = std::env::var(LIMIT_VARIABLE)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|limit| *limit > 0 && *limit < cases.len())
    {
        // The same number from every axis (fewer where an axis is smaller),
        // spread evenly over the axis.
        let per_axis = limit.div_ceil(Axis::ALL.len());
        cases = Axis::ALL
            .into_iter()
            .flat_map(|axis| {
                let of_axis: Vec<&Case> = cases.iter().filter(|case| case.axis == axis).collect();
                let taken = per_axis.min(of_axis.len());
                (0..taken)
                    .map(|n| of_axis[n * of_axis.len() / taken].clone())
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    cases
}

struct Outcome {
    case: Case,
    raw: String,
    cached: bool,
    millis: u128,
    verdict: Result<(), String>,
}

#[derive(Default)]
struct Tally(BTreeMap<String, (usize, usize)>);

impl Tally {
    fn add(&mut self, key: String, passed: bool) {
        let entry = self.0.entry(key).or_default();
        entry.0 += usize::from(passed);
        entry.1 += 1;
    }

    fn write(&self, title: &str, out: &mut String) {
        let _ = writeln!(out, "{title}");
        for (key, (passed, total)) in &self.0 {
            let _ = writeln!(out, "  {key}: {passed}/{total} ({}%)", passed * 100 / total);
        }
    }
}

fn placeholders_in(pattern: &str) -> Vec<&str> {
    let mut names: Vec<&str> = pattern
        .split('{')
        .skip(1)
        .filter_map(|s| s.split('}').next())
        .collect();
    names.sort_unstable();
    names
}

/// A coarse name for what went wrong, by comparing the pattern the model
/// wrote with the expected one.
fn failure_kind(outcome: &Outcome) -> &'static str {
    let (parsed, wanted) = match &outcome.case.expected {
        Expected::Rule(spec) => (
            parse_generate(&outcome.raw).map(|rule| rule.filing_pattern),
            spec.filing_pattern.clone(),
        ),
        Expected::Pattern(pattern) => (parse_pattern(&outcome.raw), pattern.clone()),
    };
    let Ok(got) = parsed else {
        return "answer is not parsed";
    };
    if got == wanted {
        "pattern right, conditions wrong"
    } else if got.trim_end_matches('/') == wanted.trim_end_matches('/') {
        "trailing slash wrong (folder or file name)"
    } else if placeholders_in(&got) == placeholders_in(&wanted) {
        "right placeholders, wrong order, separator or literal"
    } else if got.matches('/').count() == wanted.matches('/').count() {
        "wrong placeholder"
    } else {
        "wrong number of parts"
    }
}

fn mean(millis: &[u128]) -> u128 {
    millis
        .iter()
        .sum::<u128>()
        .checked_div(u128::try_from(millis.len()).unwrap())
        .unwrap_or_default()
}

fn report(outcomes: &[Outcome], setup_millis: u128) -> String {
    let (mut axes, mut languages, mut placeholders, mut kinds) = (
        Tally::default(),
        Tally::default(),
        Tally::default(),
        Tally::default(),
    );
    for outcome in outcomes {
        let case = &outcome.case;
        let passed = outcome.verdict.is_ok();
        axes.add(axis_name(case.axis).to_owned(), passed);
        languages.add(language_name(case.language).to_owned(), passed);
        if case.axis != Axis::Condition {
            for name in &case.placeholders {
                placeholders.add(format!("{{{name}}}"), passed);
            }
        }
        for kind in &case.conditions {
            if case.axis == Axis::Condition {
                kinds.add(kind.as_str().to_owned(), passed);
            }
        }
    }
    let cold: Vec<u128> = outcomes
        .iter()
        .filter(|o| !o.cached)
        .map(|o| o.millis)
        .collect();
    let warm: Vec<u128> = outcomes
        .iter()
        .filter(|o| o.cached)
        .map(|o| o.millis)
        .collect();
    let passed = outcomes.iter().filter(|o| o.verdict.is_ok()).count();

    let mut out = String::new();
    let _ = writeln!(
        out,
        "rule matrix: {passed}/{} right ({}%)",
        outcomes.len(),
        passed * 100 / outcomes.len().max(1)
    );
    let _ = writeln!(
        out,
        "time: model and prefix {setup_millis} ms; cold {} cases mean {} ms; cache {} cases mean {} ms",
        cold.len(),
        mean(&cold),
        warm.len(),
        mean(&warm)
    );
    axes.write("per axis", &mut out);
    placeholders.write("per placeholder (folder, file name, recognize)", &mut out);
    kinds.write("per condition kind", &mut out);
    languages.write("per language", &mut out);
    let mut kinds_of_failure: BTreeMap<&str, (usize, &Outcome)> = BTreeMap::new();
    for outcome in outcomes.iter().filter(|o| o.verdict.is_err()) {
        kinds_of_failure
            .entry(failure_kind(outcome))
            .or_insert((0, outcome))
            .0 += 1;
    }
    let mut ranked: Vec<_> = kinds_of_failure.into_iter().collect();
    ranked.sort_by_key(|(_, (count, _))| std::cmp::Reverse(*count));
    let _ = writeln!(out, "failure kinds");
    for (name, (count, example)) in ranked {
        let _ = writeln!(
            out,
            "  {count}x {name}\n    e.g. {}\n    got:    {}\n    wanted: {}",
            example.case.asked(),
            example.raw,
            example.case.expected_text()
        );
    }
    let _ = writeln!(out, "failures");
    for outcome in outcomes.iter().filter(|o| o.verdict.is_err()) {
        let case = &outcome.case;
        let _ = writeln!(
            out,
            "FAIL [{} {}] {}\n  got:    {}\n  wanted: {}\n  why:    {}",
            axis_name(case.axis),
            language_name(case.language),
            case.asked(),
            outcome.raw,
            case.expected_text(),
            outcome.verdict.as_ref().err().map_or("", String::as_str)
        );
    }
    out
}

#[test]
#[ignore = "needs the AI model: set NCRS_AI_MODELS_DIR to the folder with it"]
fn with_model_rule_matrix_reports() {
    let cases = chosen_cases();
    assert!(!cases.is_empty(), "the shard has no cases");
    let documents = context_documents();
    let kinds = archive_kinds();
    let prefix = prefix_text(&documents, &kinds);
    let mut cache = ResultCache::open(&default_cache_path());

    let mut outcomes = Vec::new();
    let mut setup_millis = 0;
    for axis in Axis::ALL {
        // One session per group: the prefix is decoded once for the group.
        let mut session: Option<RuleSession<'static>> = None;
        for case in cases.iter().filter(|case| case.axis == axis) {
            let request = request_of(case, &prefix, &kinds);
            let key = cache_key(&LOCAL_MODEL, &request);
            let (raw, cached, millis) = if let Some(raw) = cache.get(&key) {
                let started = Instant::now();
                (raw.to_owned(), true, started.elapsed().as_millis())
            } else {
                if session.is_none() {
                    let started = Instant::now();
                    session = Some(
                        RuleSession::new(model(), &documents, &kinds)
                            .unwrap_or_else(|error| panic!("the session did not start: {error}")),
                    );
                    setup_millis += started.elapsed().as_millis();
                    println!(
                        "prefix: {} tokens",
                        session.as_ref().map_or(0, RuleSession::prefix_tokens)
                    );
                }
                let started = Instant::now();
                let raw = session
                    .as_mut()
                    .unwrap()
                    .answer(&request)
                    .unwrap_or_else(|error| {
                        panic!("the model failed on {}: {error}", case.asked())
                    });
                let millis = started.elapsed().as_millis();
                cache.put(&key, &raw).unwrap();
                (raw, false, millis)
            };
            let verdict = check_answer(case, &raw);
            println!(
                "case [{} {}] {millis} ms {} {}",
                axis_name(case.axis),
                language_name(case.language),
                if cached { "cache" } else { "cold" },
                if verdict.is_ok() { "ok" } else { "miss" }
            );
            outcomes.push(Outcome {
                case: case.clone(),
                raw,
                cached,
                millis,
                verdict,
            });
        }
    }

    let text = report(&outcomes, setup_millis);
    println!("{text}");
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/rule-matrix-report.txt");
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::fs::write(&file, &text).unwrap();
}
