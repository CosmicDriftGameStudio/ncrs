//! Opening a folder of the archive and making a filing rule for it: the
//! levels of the folder, what goes into it, the trial and the click that
//! stores the rule. Nothing is saved or moved before that click.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_ai::{pattern_mismatches, ContextDocument, Level, Levels, Part as LevelPart};
use ncrs_filing::chat_work::{ChatWork, FolderReading};
use ncrs_i18n::Msg;
use ncrs_index::{Condition, ConditionKind, DocumentFields, Kinds, RuleConditions, RuleSpec};
use ncrs_intake::{label_text, render_pattern_in, rule_problem, PlaceholderLabel};
use ncrs_ui_kit::hermes_kit::LevelRow;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::learn;
use crate::message::Message;
use crate::state::{Chip, ChipReply, Entry, LevelsCard, Part, ReadFor, RuleMoves, State, Step};
use crate::texts::{fill, folder_name, number, rule_problem_text, segments, text};
use crate::understand::{gate, Gate};
use crate::update::{
    access, ask_read_folder, cancel_chip, chip, chip_labelled, drop_working, fallback,
    fallback_chips, hermes, say, work,
};

mod ask;
mod change;

pub use ask::{RuleAnswer, RuleAsk};
pub use change::{levels_change, levels_discard, levels_more};

const DEFAULT_LEVELS: &str = "{year}/{date}_{sender}_{kind}";
/// Files the model sees: the prefix of its prompt grows with them.
const MODEL_FILES: usize = 6;
const CONTEXT_FILES: usize = 12;
const SUGGESTIONS: usize = 3;
const MISFITS_SHOWN: usize = 3;
/// Different values the levels card shows per level.
const FOLDER_VALUES: usize = 3;
const FILE_VALUES: usize = 2;

#[derive(Debug, Clone)]
pub struct RuleFlow {
    /// The folder, and the path below its target with a closing `/`.
    folder: String,
    prefix: String,
    /// The folder is not known yet: the description names it (flow D).
    free: bool,
    levels: Levels,
    /// The levels as the model found them (or the standard): what "discard"
    /// goes back to and what a changed level is marked against.
    found: Levels,
    /// The user is changing the levels in words.
    changing: bool,
    /// The files of the folder: path below the folder, and fields.
    files: Vec<(String, DocumentFields)>,
    /// The rule the model made from a description; its levels can still change.
    candidate: Option<RuleSpec>,
    /// The rule on screen, waiting for the click.
    proposed: Option<RuleSpec>,
    /// What the trial of the proposal would move.
    moves: RuleMoves,
    /// Read once when the flow starts.
    kinds: Kinds,
}

impl RuleFlow {
    fn new(folder: String, prefix: String, kinds: Kinds) -> Self {
        Self {
            folder,
            prefix,
            free: false,
            levels: default_levels(),
            found: default_levels(),
            changing: false,
            files: Vec::new(),
            candidate: None,
            proposed: None,
            moves: RuleMoves::default(),
            kinds,
        }
    }

    /// Without levels the files get their automatic name in the folder
    /// itself, which is the prefix alone.
    fn pattern(&self) -> String {
        if self.levels.count() == 0 {
            return self.prefix.clone();
        }
        format!("{}{}", self.prefix, self.levels.pattern())
    }

    fn written_files(&self) -> Vec<(String, DocumentFields)> {
        self.files
            .iter()
            .map(|(path, fields)| (format!("{}{path}", self.prefix), fields.clone()))
            .collect()
    }
}

fn default_levels() -> Levels {
    Levels::from_pattern(DEFAULT_LEVELS).unwrap_or(Levels {
        folders: Vec::new(),
        file: None,
    })
}

fn sample<T: Clone>(items: &[T], most: usize) -> Vec<T> {
    if items.len() <= most {
        return items.to_vec();
    }
    (0..most)
        .filter_map(|position| {
            items
                .get((position * items.len()).checked_div(most)?)
                .cloned()
        })
        .collect()
}

fn kinds_of(ctx: &Ctx) -> Kinds {
    access(ctx)
        .and_then(|access| access.kinds().ok())
        .unwrap_or_else(|| learn::word_kinds(ctx))
}

/// Reads a folder of the archive for its levels. `false` when the folder is
/// not below a target.
pub fn open(state: &mut State, ctx: &Ctx, folder: PathBuf) -> bool {
    let Some(target) = ctx.targets.iter().find(|target| folder.starts_with(target)) else {
        return false;
    };
    let below = segments(folder.strip_prefix(target).unwrap_or(Path::new("")));
    let prefix = if below.is_empty() {
        String::new()
    } else {
        format!("{}/", below.join("/"))
    };
    state.session.rule = Some(RuleFlow::new(
        folder_name(ctx, &folder),
        prefix,
        kinds_of(ctx),
    ));
    learn::read_for(state, ctx, folder, ReadFor::Open);
    true
}

pub fn folder_read(state: &mut State, ctx: &Ctx, reading: &FolderReading) {
    if ctx.targets.contains(&reading.root) {
        learn::offer_duplicates(state, ctx, reading);
    }
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    flow.files = reading
        .files
        .iter()
        .map(|file| (segments(&file.relative_path).join("/"), file.fields.clone()))
        .collect();
    if reading.files.is_empty() {
        let sentence = fill(
            ctx.lang,
            Msg::HermesChatFolderEmpty,
            &[("folder", &flow.folder)],
        );
        let mut parts = vec![Part::Text(sentence)];
        parts.extend(levels_parts(state, ctx, false).0);
        hermes(state, parts, levels_chips(ctx, false));
        state.step = Step::RuleLevels;
        return;
    }
    if flow.prefix.is_empty() && !reading.structure.areas.is_empty() {
        let areas = &reading.structure.areas;
        let sentence = if areas.len() == 1 {
            text(ctx.lang, Msg::HermesChatAreaHere)
        } else {
            fill(
                ctx.lang,
                Msg::HermesChatAreasHere,
                &[("count", &number(areas.len()))],
            )
        };
        let mut chips: Vec<Chip> = areas
            .iter()
            .map(|area| {
                chip_labelled(
                    area.folder.clone(),
                    ChipReply::Folder(reading.root.join(&area.folder)),
                    false,
                )
            })
            .collect();
        chips.push(cancel_chip(ctx));
        state.session.rule = None;
        state.step = Step::ReadFolder;
        let mut parts = vec![Part::Text(sentence)];
        parts.extend(learn::ai_progress(ctx, reading));
        hermes(state, parts, chips);
        return;
    }
    let files = sample(&flow.files, MODEL_FILES);
    let all = flow.files.clone();
    let kinds = flow.kinds.clone();
    ask_model(state, ctx, RuleAsk::Recognize { files, all, kinds });
}

/// Asks the model. Without one (AI off, model missing, none at all) the usual
/// offer is said instead and the flow is over, so nothing of it is left to
/// answer a later sentence.
fn ask_model(state: &mut State, ctx: &Ctx, ask: RuleAsk) -> bool {
    let usable = gate(state, ctx, None) == Gate::Usable;
    if usable && ctx.model.is_none() {
        fallback(state, ctx);
    }
    if !usable || ctx.model.is_none() {
        state.session.rule = None;
        state.step = Step::Idle;
        return false;
    }
    state
        .thread
        .push(Entry::Working(text(ctx.lang, Msg::HermesChatThinking)));
    state.rule_ask = Some(ask);
    state.fresh_rule_ask = true;
    true
}

/// The task that asks the model on a blocking thread.
pub fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh_rule_ask) {
        return Task::none();
    }
    let Some(ask) = state.rule_ask.clone() else {
        return Task::none();
    };
    let Some(model) = ctx.model.clone() else {
        state.rule_ask = None;
        drop_working(state);
        return Task::none();
    };
    Task::perform(
        async move { tokio::task::spawn_blocking(move || ask.run(model.as_ref())).await },
        |joined| match joined {
            Ok(answer) => Message::RuleAnswered(answer),
            Err(error) => Message::RuleAnswered(RuleAnswer::Failed(error.to_string())),
        },
    )
}

pub fn answered(state: &mut State, ctx: &Ctx, answer: RuleAnswer) -> Option<Action> {
    state.rule_ask = None;
    drop_working(state);
    state.session.rule.as_ref()?;
    match answer {
        RuleAnswer::Levels(Ok(levels)) => {
            if let Some(flow) = state.session.rule.as_mut() {
                flow.levels = levels.clone();
                flow.found = levels;
                flow.changing = false;
            }
            show_levels_checked(state, ctx, true);
        }
        RuleAnswer::Revised(Ok(revision)) => change::revised(state, ctx, revision),
        RuleAnswer::Revised(Err(_)) => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatRuleNotLevels),
                Vec::new(),
            );
            levels_change(state, ctx);
        }
        RuleAnswer::Rule(Ok(spec)) => generated(state, ctx, spec),
        RuleAnswer::Levels(Err(reason))
        | RuleAnswer::Rule(Err(reason))
        | RuleAnswer::Failed(reason) => {
            state.session.rule = None;
            state.step = Step::Idle;
            if gate(state, ctx, None) == Gate::Usable {
                say(
                    state,
                    fill(ctx.lang, Msg::HermesChatModelFailed, &[("reason", &reason)]),
                    fallback_chips(ctx),
                );
            }
        }
    }
    None
}

// The levels.

/// Below this share of fitting files a recognized pattern is not offered as it is.
const FIT_AT_LEAST_PERCENT: usize = 80;

fn levels_chips(ctx: &Ctx, with_default: bool) -> Vec<Chip> {
    let keep = if with_default {
        Msg::HermesChatLevelsKeep
    } else {
        Msg::HermesChatLevelsCreate
    };
    let mut chips = vec![
        chip(ctx, keep, ChipReply::LevelsKeep, true),
        chip(
            ctx,
            Msg::HermesChatLevelsChange,
            ChipReply::LevelsChange,
            false,
        ),
    ];
    if with_default {
        chips.push(chip(
            ctx,
            Msg::HermesChatLevelsDefault,
            ChipReply::LevelsDefault,
            false,
        ));
    }
    chips
}

fn field_word(ctx: &Ctx, name: &str) -> String {
    let label = match name {
        "year" => PlaceholderLabel::Year,
        "month" => PlaceholderLabel::Month,
        "date" => PlaceholderLabel::Date,
        "number" => PlaceholderLabel::Number,
        "sender" => PlaceholderLabel::Sender,
        "kind" => PlaceholderLabel::Kind,
        "day" => return text(ctx.lang, Msg::HermesChatLevelRoleDay),
        "recipient" => return text(ctx.lang, Msg::HermesChatLevelRoleRecipient),
        other => return other.to_owned(),
    };
    label_text(ctx.lang, label).to_owned()
}

fn words(ctx: &Ctx, level: &Level) -> Vec<String> {
    level
        .parts
        .iter()
        .map(|part| match part {
            LevelPart::Fixed(fixed) => fixed.clone(),
            LevelPart::Placeholder(name) => field_word(ctx, name),
        })
        .collect()
}

fn is_fixed(level: &Level) -> bool {
    level
        .parts
        .iter()
        .all(|part| matches!(part, LevelPart::Fixed(_)))
}

/// What a folder of no files shows: the words of the fields, no real value.
fn placeholder_fields(ctx: &Ctx) -> DocumentFields {
    DocumentFields {
        sender: Some(label_text(ctx.lang, PlaceholderLabel::Sender).to_owned()),
        kind: Some(label_text(ctx.lang, PlaceholderLabel::Kind).to_owned()),
        date: Some(ctx.now.date_naive()),
        ..DocumentFields::default()
    }
}

/// Different values of a level from the files, in the order of the files.
fn level_values(ctx: &Ctx, flow: &RuleFlow, kinds: &Kinds, level: &Level, most: usize) -> String {
    let render = |fields: &DocumentFields| {
        render_pattern_in(&level.text(), fields, kinds).map(|path| segments(&path).join("/"))
    };
    let mut values: Vec<String> = Vec::new();
    for (_, fields) in &flow.files {
        if let Some(value) = render(fields) {
            if !values.contains(&value) {
                values.push(value);
            }
        }
        if values.len() == most {
            break;
        }
    }
    if flow.files.is_empty() {
        values.extend(render(&placeholder_fields(ctx)));
    }
    if values.is_empty() {
        "–".to_owned()
    } else {
        values.join(", ")
    }
}

/// The names the files have now, when no file name level of its own is set.
fn file_names(flow: &RuleFlow) -> String {
    let mut names: Vec<&str> = Vec::new();
    for (path, _) in &flow.files {
        let name = path.rsplit('/').next().unwrap_or(path);
        if !names.contains(&name) {
            names.push(name);
        }
        if names.len() == FILE_VALUES {
            break;
        }
    }
    names.join(", ")
}

/// `baseline`: the levels a changed level is marked against.
fn level_rows(
    ctx: &Ctx,
    flow: &RuleFlow,
    kinds: &Kinds,
    baseline: Option<&Levels>,
) -> Vec<LevelRow> {
    let value = |level: &Level| level_values(ctx, flow, kinds, level, FOLDER_VALUES);
    let before = |old: Option<&Level>, now: Option<&Level>, shown: &dyn Fn(&Level) -> String| {
        baseline.filter(|_| old != now).map(|_| {
            fill(
                ctx.lang,
                Msg::HermesChatLevelBefore,
                &[("value", &old.map_or_else(|| "–".to_owned(), shown))],
            )
        })
    };
    let mut rows: Vec<LevelRow> = flow
        .levels
        .folders
        .iter()
        .enumerate()
        .map(|(position, level)| LevelRow {
            number: position + 1,
            text: value(level),
            role: if is_fixed(level) {
                text(ctx.lang, Msg::HermesChatLevelRoleFixed)
            } else {
                words(ctx, level).join(", ")
            },
            before: before(
                baseline.and_then(|found| found.folders.get(position)),
                Some(level),
                &value,
            ),
        })
        .collect();
    let number = rows.len() + 1;
    let file_value = |level: &Level| level_values(ctx, flow, kinds, level, FILE_VALUES);
    let old_file = baseline.and_then(|found| found.file.as_ref());
    rows.push(match &flow.levels.file {
        Some(level) => LevelRow {
            number,
            text: file_value(level),
            role: fill(
                ctx.lang,
                Msg::HermesChatLevelRoleFile,
                &[("parts", &words(ctx, level).join(", "))],
            ),
            before: before(old_file, Some(level), &file_value),
        },
        None => LevelRow {
            number,
            text: file_names(flow),
            role: text(ctx.lang, Msg::HermesChatLevelRoleAutomatic),
            before: before(old_file, None, &file_value),
        },
    });
    rows
}

fn levels_card(
    ctx: &Ctx,
    flow: &RuleFlow,
    kinds: &Kinds,
    footer: Option<String>,
    baseline: Option<&Levels>,
) -> LevelsCard {
    let key = if baseline.is_some() {
        Msg::HermesChatLevelsChanged
    } else if flow.files.is_empty() {
        Msg::HermesChatLevelsSuggested
    } else {
        Msg::HermesChatLevelsFound
    };
    let rows = level_rows(ctx, flow, kinds, baseline);
    LevelsCard {
        title: fill(ctx.lang, key, &[("count", &number(rows.len()))]),
        rows,
        footer,
    }
}

/// The card of the levels, and under it the files that do not follow them.
fn levels_parts(state: &State, ctx: &Ctx, poor_fit: bool) -> (Vec<Part>, bool) {
    let Some(flow) = state.session.rule.as_ref() else {
        return (Vec::new(), false);
    };
    let kinds = flow.kinds.clone();
    let written = flow.written_files();
    let misfits = pattern_mismatches(&flow.pattern(), &written, &kinds);
    let footer = (!misfits.is_empty()).then(|| {
        fill(
            ctx.lang,
            Msg::HermesChatLevelsFit,
            &[
                ("fitting", &number(written.len() - misfits.len())),
                ("total", &number(written.len())),
            ],
        )
    });
    let mut parts = vec![Part::Levels(levels_card(ctx, flow, &kinds, footer, None))];
    let fitting = written.len() - misfits.len();
    let poor = poor_fit && fitting * 100 < written.len() * FIT_AT_LEAST_PERCENT;
    if poor {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatLevelsPoor,
            &[
                ("fitting", &number(fitting)),
                ("total", &number(written.len())),
            ],
        )));
    }
    if !misfits.is_empty() {
        let lines: Vec<String> = misfits
            .iter()
            .take(MISFITS_SHOWN)
            .map(|misfit| match &misfit.written {
                Some(shown) => fill(
                    ctx.lang,
                    Msg::HermesChatLevelsMisfit,
                    &[
                        ("file", &misfit.file),
                        ("written", &with_extension_of(shown, &misfit.file)),
                    ],
                ),
                None => fill(
                    ctx.lang,
                    Msg::HermesChatLevelsMisfitNone,
                    &[("file", &misfit.file)],
                ),
            })
            .collect();
        parts.push(Part::Text(lines.join("\n")));
    }
    (parts, poor)
}

/// `checked`: the levels are the model's, so a pattern that fits too few
/// files is not offered as it is.
fn show_levels_checked(state: &mut State, ctx: &Ctx, checked: bool) {
    let (parts, poor) = levels_parts(state, ctx, checked);
    let with_default = state
        .session
        .rule
        .as_ref()
        .is_some_and(|flow| !flow.files.is_empty());
    state.step = Step::RuleLevels;
    let chips = if poor {
        vec![chip(
            ctx,
            Msg::HermesChatLevelsChange,
            ChipReply::LevelsChange,
            true,
        )]
    } else {
        levels_chips(ctx, with_default)
    };
    hermes(state, parts, chips);
}

/// The standard is a change of the levels found like any other: it is shown
/// marked against them, with what applying it does and the way back.
pub fn levels_default(state: &mut State, ctx: &Ctx) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    flow.levels = default_levels();
    flow.changing = true;
    change::show_change(state, ctx, false);
}

/// The levels are taken as they stand: the rule the model wrote is shown
/// again, else a rule for what the files of the folder share, else the user
/// says what goes into the folder.
pub fn levels_kept(state: &mut State, ctx: &Ctx) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    flow.changing = false;
    match flow.candidate.clone().or_else(|| shared_by_files(flow)) {
        Some(spec) => propose(state, ctx, spec),
        None => ask_what(state, ctx, None),
    }
}

/// A rule for the documents that have the sender (else the kind) most of the
/// files of the folder have.
fn shared_by_files(flow: &RuleFlow) -> Option<RuleSpec> {
    let fields = || flow.files.iter().map(|(_, found)| found);
    let (kind, value) = match counted(fields().filter_map(|found| found.sender.clone())) {
        Some(sender) => (ConditionKind::Sender, sender),
        None => (
            ConditionKind::Kind,
            counted(fields().filter_map(|found| found.kind.clone()))?,
        ),
    };
    Some(RuleSpec {
        name: None,
        conditions: RuleConditions::all_of(vec![Condition::new(kind, value)]),
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: std::collections::BTreeSet::new(),
        filing_pattern: String::new(),
        confirmed: false,
        enabled: false,
    })
}

// What goes into the folder.

fn counted(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|first, second| first.1.cmp(&second.1).then_with(|| second.0.cmp(&first.0)))
        .map(|(value, _)| value)
}

fn suggestions(ctx: &Ctx, flow: &RuleFlow, kinds: &Kinds) -> Vec<Chip> {
    let all_fields = || flow.files.iter().map(|(_, found)| found);
    let top_kind = counted(all_fields().filter_map(|found| found.kind.clone()));
    let top_sender = counted(all_fields().filter_map(|found| found.sender.clone()));
    let both = counted(all_fields().filter_map(|found| {
        Some(format!(
            "{}\u{0}{}",
            found.kind.as_ref()?,
            found.sender.as_ref()?
        ))
    }));
    let mut labels: Vec<String> = Vec::new();
    if let Some(common_kind) = &top_kind {
        labels.push(fill(
            ctx.lang,
            Msg::HermesChatRuleSuggestKind,
            &[("kind", &kinds.kind_word(common_kind))],
        ));
    }
    if let Some(common_sender) = &top_sender {
        labels.push(fill(
            ctx.lang,
            Msg::HermesChatRuleSuggestSender,
            &[("sender", common_sender)],
        ));
    }
    if let Some((pair_kind, pair_sender)) =
        both.as_deref().and_then(|pair| pair.split_once('\u{0}'))
    {
        labels.push(fill(
            ctx.lang,
            Msg::HermesChatRuleSuggestBoth,
            &[
                ("kind", &kinds.kind_word(pair_kind)),
                ("sender", pair_sender),
            ],
        ));
    }
    labels.dedup();
    labels
        .into_iter()
        .take(SUGGESTIONS)
        .map(|label| chip_labelled(label.clone(), ChipReply::RuleWhat(label), false))
        .collect()
}

fn ask_what(state: &mut State, ctx: &Ctx, lead: Option<String>) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    if flow.folder.is_empty() {
        // No folder was ever named, so there is nothing to ask "what goes into".
        state.session.rule = None;
        ask_read_folder(state, ctx, None);
        return;
    }
    flow.proposed = None;
    let kinds = flow.kinds.clone();
    let question = fill(
        ctx.lang,
        Msg::HermesChatRuleWhat,
        &[("folder", &flow.folder)],
    );
    let sentence = match lead {
        Some(opening) => format!("{opening} {question}"),
        None => question,
    };
    let mut chips = state
        .session
        .rule
        .as_ref()
        .map(|current| suggestions(ctx, current, &kinds))
        .unwrap_or_default();
    chips.push(cancel_chip(ctx));
    state.step = Step::RuleWhat;
    say(state, sentence, chips);
}

pub fn edit(state: &mut State, ctx: &Ctx) {
    ask_what(state, ctx, None);
}

/// Words about what goes into the folder, typed or taken from a button.
pub fn described(state: &mut State, ctx: &Ctx, description: &str) {
    ask_rule(state, ctx, description);
}

/// Typed words: while the levels are shown they change the levels, else they
/// say what goes into the folder.
pub fn typed(state: &mut State, ctx: &Ctx, typed: &str) {
    if state.step == Step::RuleLevels {
        change::revise(state, ctx, typed);
    } else {
        ask_rule(state, ctx, typed);
    }
}

fn ask_rule(state: &mut State, ctx: &Ctx, description: &str) -> bool {
    let Some(flow) = state.session.rule.as_mut() else {
        return false;
    };
    let kinds = flow.kinds.clone();
    let documents = sample(&flow.files, CONTEXT_FILES)
        .into_iter()
        .map(|(path, fields)| ContextDocument {
            fields,
            folder: Path::new(&path)
                .parent()
                .map(|folder| segments(folder).join("/"))
                .filter(|folder| !folder.is_empty()),
        })
        .collect();
    ask_model(
        state,
        ctx,
        RuleAsk::Generate {
            description: description.to_owned(),
            documents,
            kinds,
        },
    )
}

/// A description with no folder in it: the rule names the folder itself.
pub fn from_description(state: &mut State, ctx: &Ctx, description: &str) {
    let mut flow = RuleFlow::new(String::new(), String::new(), kinds_of(ctx));
    flow.free = true;
    state.session.rule = Some(flow);
    if ask_rule(state, ctx, description) {
        state.step = Step::RuleWhat;
    }
}

/// The model wrote a rule: its folder and levels are taken when no folder was
/// open, its conditions always.
fn generated(state: &mut State, ctx: &Ctx, spec: RuleSpec) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    if flow.free {
        let Some(mut levels) = Levels::from_pattern(&spec.filing_pattern) else {
            state.session.rule = None;
            state.step = Step::Idle;
            say(
                state,
                text(ctx.lang, Msg::HermesChatRuleNotLevels),
                fallback_chips(ctx),
            );
            return;
        };
        let named = match levels.folders.first().map(|level| level.parts.as_slice()) {
            Some([LevelPart::Fixed(name)]) => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = named {
            levels.folders.remove(0);
            flow.prefix = format!("{name}/");
            flow.folder = name;
        }
        flow.levels = if levels.count() > 0 {
            levels
        } else {
            default_levels()
        };
        flow.free = false;
        flow.candidate = Some(spec.clone());
    }
    propose(state, ctx, spec);
}

/// The rule with the confirmed levels as its pattern: refused with a reason,
/// or tried against the documents.
fn propose(state: &mut State, ctx: &Ctx, mut spec: RuleSpec) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    spec.filing_pattern = flow.pattern();
    spec.confirmed = true;
    spec.enabled = true;
    if let Some(problem) = rule_problem(&spec) {
        let reason = fill(
            ctx.lang,
            Msg::HermesChatRuleRefused,
            &[("reason", &rule_problem_text(ctx.lang, &problem))],
        );
        ask_what(state, ctx, Some(reason));
        return;
    }
    flow.proposed = Some(spec.clone());
    work(state, ChatWork::RuleTrial { spec }, None);
}

mod trial;

pub use trial::{create, file_now, stored, trial_done};

/// What a pattern writes has no extension; the file it is compared with has.
fn with_extension_of(written: &str, file: &str) -> String {
    let extension = file
        .rsplit_once('/')
        .map_or(file, |(_, name)| name)
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension);
    if extension.is_empty() {
        written.to_owned()
    } else {
        format!("{written}.{extension}")
    }
}
