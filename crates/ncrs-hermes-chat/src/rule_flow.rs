//! Opening a folder of the archive and making a filing rule for it: the
//! levels of the folder, what goes into it, the trial and the click that
//! stores the rule. Nothing is saved or moved before that click.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_ai::{pattern_mismatches, ChatModel, ContextDocument, Level, Levels, Part as LevelPart};
use ncrs_filing::chat_work::{ChatWork, FolderReading, RuleSaveFailure, RuleTrial};
use ncrs_filing::rules_work::TryOutcome;
use ncrs_i18n::Msg;
use ncrs_index::{Combinator, Condition, DocumentFields, Kinds, RuleSpec};
use ncrs_intake::{label_text, render_pattern_in, rule_problem, PlaceholderLabel};
use ncrs_ui_kit::hermes_kit::LevelRow;
use ncrs_ui_kit::kit::condition_kind_name;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::learn;
use crate::message::Message;
use crate::state::{Chip, ChipReply, Entry, LevelsCard, Part, ReadFor, State, Step};
use crate::texts::{fill, folder_name, number, rule_problem_text, segments, text};
use crate::understand::{gate, Gate};
use crate::update::{
    access, cancel_chip, chip, chip_labelled, drop_working, fallback, fallback_chips,
    folder_answer, hermes, say, source_chips, work,
};

const DEFAULT_LEVELS: &str = "{year}/{date}_{sender}_{kind}";
/// Files the model sees: the prefix of its prompt grows with them.
const MODEL_FILES: usize = 6;
const CONTEXT_FILES: usize = 12;
const SUGGESTIONS: usize = 3;
const MISFITS_SHOWN: usize = 3;

/// What the model is asked.
#[derive(Debug, Clone)]
pub enum RuleAsk {
    Recognize {
        files: Vec<(String, DocumentFields)>,
        kinds: Kinds,
    },
    Generate {
        description: String,
        documents: Vec<ContextDocument>,
        kinds: Kinds,
    },
}

/// What the model made of it.
#[derive(Debug, Clone)]
pub enum RuleAnswer {
    Levels(Result<Levels, String>),
    Rule(Result<RuleSpec, String>),
    /// The thread that asked the model broke.
    Failed(String),
}

impl RuleAsk {
    pub fn run(&self, model: &dyn ChatModel) -> RuleAnswer {
        match self {
            Self::Recognize { files, kinds } => RuleAnswer::Levels(
                model
                    .recognize_levels(files, kinds)
                    .map_err(|error| error.to_string()),
            ),
            Self::Generate {
                description,
                documents,
                kinds,
            } => RuleAnswer::Rule(
                model
                    .generate_rule(description, documents, kinds)
                    .map_err(|error| error.to_string()),
            ),
        }
    }
}

/// What the next answer of the model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Recognize,
    /// Words that change the levels.
    Levels,
    /// Words that say what goes into the folder.
    Rule,
}

#[derive(Debug, Clone)]
pub struct RuleFlow {
    /// The folder, and the path below its target with a closing `/`.
    folder: String,
    prefix: String,
    /// The folder is not known yet: the description names it (flow D).
    free: bool,
    levels: Levels,
    /// The files of the folder: path below the folder, and fields.
    files: Vec<(String, DocumentFields)>,
    /// The rule the model made from a description; its levels can still change.
    candidate: Option<RuleSpec>,
    /// The rule on screen, waiting for the click.
    proposed: Option<RuleSpec>,
    stage: Stage,
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
            files: Vec::new(),
            candidate: None,
            proposed: None,
            stage: Stage::Recognize,
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
        parts.extend(levels_parts(state, ctx));
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
    flow.stage = Stage::Recognize;
    let files = sample(&flow.files, MODEL_FILES);
    let kinds = flow.kinds.clone();
    ask_model(state, ctx, RuleAsk::Recognize { files, kinds });
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
    let stage = state.session.rule.as_ref()?.stage;
    match (answer, stage) {
        (RuleAnswer::Levels(Ok(levels)), _) => {
            if let Some(flow) = state.session.rule.as_mut() {
                flow.levels = levels;
            }
            show_levels(state, ctx);
        }
        (RuleAnswer::Rule(Ok(spec)), Stage::Levels) => levels_from_words(state, ctx, &spec),
        (RuleAnswer::Rule(Ok(spec)), _) => generated(state, ctx, spec),
        (
            RuleAnswer::Levels(Err(reason))
            | RuleAnswer::Rule(Err(reason))
            | RuleAnswer::Failed(reason),
            _,
        ) => {
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

fn sample_fields(ctx: &Ctx, flow: &RuleFlow, kinds: &Kinds) -> (DocumentFields, String) {
    let complete = |fields: &DocumentFields| {
        render_pattern_in(&flow.levels.pattern(), fields, kinds).is_some()
    };
    match flow
        .files
        .iter()
        .find(|(_, fields)| complete(fields))
        .or_else(|| flow.files.first())
    {
        Some((path, fields)) => (
            fields.clone(),
            path.rsplit('/').next().unwrap_or(path).to_owned(),
        ),
        None => (
            DocumentFields {
                sender: Some(label_text(ctx.lang, PlaceholderLabel::Sender).to_owned()),
                kind: Some(label_text(ctx.lang, PlaceholderLabel::Kind).to_owned()),
                date: Some(ctx.now.date_naive()),
                ..DocumentFields::default()
            },
            String::new(),
        ),
    }
}

fn level_rows(ctx: &Ctx, flow: &RuleFlow, kinds: &Kinds) -> Vec<LevelRow> {
    let (fields, file_name) = sample_fields(ctx, flow, kinds);
    let value = |level: &Level| {
        render_pattern_in(&level.text(), &fields, kinds)
            .map_or_else(|| "–".to_owned(), |path| segments(&path).join("/"))
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
        })
        .collect();
    let number = rows.len() + 1;
    rows.extend(match &flow.levels.file {
        Some(level) => Some(LevelRow {
            number,
            text: value(level),
            role: fill(
                ctx.lang,
                Msg::HermesChatLevelRoleFile,
                &[("parts", &words(ctx, level).join(", "))],
            ),
        }),
        None => Some(LevelRow {
            number,
            text: file_name,
            role: text(ctx.lang, Msg::HermesChatLevelRoleAutomatic),
        }),
    });
    rows
}

fn levels_card(ctx: &Ctx, flow: &RuleFlow, kinds: &Kinds, footer: Option<String>) -> LevelsCard {
    let key = if flow.files.is_empty() {
        Msg::HermesChatLevelsSuggested
    } else {
        Msg::HermesChatLevelsFound
    };
    LevelsCard {
        title: fill(
            ctx.lang,
            key,
            &[("count", &number(flow.levels.count().max(1)))],
        ),
        rows: level_rows(ctx, flow, kinds),
        footer,
    }
}

/// The card of the levels, and under it the files that do not follow them.
fn levels_parts(state: &State, ctx: &Ctx) -> Vec<Part> {
    let Some(flow) = state.session.rule.as_ref() else {
        return Vec::new();
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
    let mut parts = vec![Part::Levels(levels_card(ctx, flow, &kinds, footer))];
    if !misfits.is_empty() {
        let lines: Vec<String> = misfits
            .iter()
            .take(MISFITS_SHOWN)
            .map(|misfit| match &misfit.written {
                Some(shown) => fill(
                    ctx.lang,
                    Msg::HermesChatLevelsMisfit,
                    &[("file", &misfit.file), ("written", shown)],
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
    parts
}

fn show_levels(state: &mut State, ctx: &Ctx) {
    let parts = levels_parts(state, ctx);
    let deviates = parts.len() > 1;
    state.step = Step::RuleLevels;
    hermes(state, parts, levels_chips(ctx, deviates));
}

pub fn levels_change(state: &mut State, ctx: &Ctx) {
    let Some(flow) = state.session.rule.as_ref() else {
        return;
    };
    let kinds = flow.kinds.clone();
    let mut parts = vec![Part::Levels(levels_card(ctx, flow, &kinds, None))];
    parts.push(Part::Text(text(ctx.lang, Msg::HermesChatLevelsChangeHint)));
    let mut chips: Vec<Chip> = (0..flow.levels.count())
        .map(|position| {
            chip_labelled(
                fill(
                    ctx.lang,
                    Msg::HermesChatLevelDrop,
                    &[("number", &number(position + 1))],
                ),
                ChipReply::LevelDrop(position),
                false,
            )
        })
        .collect();
    chips.push(chip(
        ctx,
        Msg::HermesChatLevelsDefault,
        ChipReply::LevelsDefault,
        false,
    ));
    chips.push(cancel_chip(ctx));
    state.step = Step::RuleLevels;
    hermes(state, parts, chips);
}

pub fn level_drop(state: &mut State, ctx: &Ctx, position: usize) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    if position < flow.levels.folders.len() {
        flow.levels.folders.remove(position);
    } else {
        flow.levels.file = None;
    }
    show_levels(state, ctx);
}

pub fn levels_default(state: &mut State, ctx: &Ctx) {
    if let Some(flow) = state.session.rule.as_mut() {
        flow.levels = default_levels();
    }
    show_levels(state, ctx);
}

/// The levels of a sentence: the pattern of the rule the model wrote for it,
/// without the folder in front when it names this one.
fn levels_from_words(state: &mut State, ctx: &Ctx, spec: &RuleSpec) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    match levels_below(&spec.filing_pattern, &flow.prefix) {
        Some(levels) => {
            flow.levels = levels;
            show_levels(state, ctx);
        }
        None => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatRuleNotLevels),
                Vec::new(),
            );
            state.step = Step::RuleLevels;
        }
    }
}

/// The levels of `pattern` below the folder: the leading fixed levels that
/// spell the path of the folder (`prefix`, from its target) are cut off. A
/// model that left out the front of the path is read as well: `Bank/{year}`
/// below `Archiv/Bank/`.
fn levels_below(pattern: &str, prefix: &str) -> Option<Levels> {
    let mut levels = Levels::from_pattern(pattern)?;
    let path: Vec<&str> = prefix.split('/').filter(|part| !part.is_empty()).collect();
    let matching = |start: usize| {
        levels
            .folders
            .iter()
            .zip(path.iter().skip(start))
            .take_while(|(level, segment)| {
                matches!(level.parts.as_slice(), [LevelPart::Fixed(name)] if name.eq_ignore_ascii_case(segment))
            })
            .count()
    };
    let cut = (0..path.len()).map(matching).max().unwrap_or(0);
    levels.folders.drain(..cut);
    (levels.count() > 0).then_some(levels)
}

/// The levels stay; next the user says what goes into the folder, or the
/// rule the model wrote is shown again.
pub fn levels_kept(state: &mut State, ctx: &Ctx) {
    let candidate = state
        .session
        .rule
        .as_ref()
        .and_then(|flow| flow.candidate.clone());
    match candidate {
        Some(spec) => propose(state, ctx, spec),
        None => ask_what(state, ctx, None),
    }
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
    flow.stage = Stage::Rule;
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
    ask_rule(state, ctx, description, Stage::Rule);
}

pub fn typed(state: &mut State, ctx: &Ctx, typed: &str) {
    let stage = if state.step == Step::RuleLevels {
        Stage::Levels
    } else {
        Stage::Rule
    };
    ask_rule(state, ctx, typed, stage);
}

fn ask_rule(state: &mut State, ctx: &Ctx, description: &str, stage: Stage) -> bool {
    let Some(flow) = state.session.rule.as_mut() else {
        return false;
    };
    flow.stage = stage;
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
    flow.stage = Stage::Rule;
    state.session.rule = Some(flow);
    if ask_rule(state, ctx, description, Stage::Rule) {
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

fn condition_words(ctx: &Ctx, spec: &RuleSpec, kinds: &Kinds) -> String {
    let items: Vec<String> = spec
        .conditions
        .list
        .iter()
        .map(|condition: &Condition| {
            let value = match condition.kind {
                ncrs_index::ConditionKind::Kind => kinds.kind_word(&condition.value).into_owned(),
                _ => condition.value.clone(),
            };
            let item = format!(
                "{}: {value}",
                text(ctx.lang, condition_kind_name(condition.kind))
            );
            if condition.negated {
                fill(
                    ctx.lang,
                    Msg::HermesChatRuleConditionNot,
                    &[("condition", &item)],
                )
            } else {
                item
            }
        })
        .collect();
    if items.len() < 2 {
        return items.join("");
    }
    let msg = match spec.conditions.combinator {
        Combinator::AllOf => Msg::HermesChatRuleConditionAll,
        Combinator::AnyOf => Msg::HermesChatRuleConditionAny,
    };
    fill(ctx.lang, msg, &[("list", &items.join(", "))])
}

fn example_line(ctx: &Ctx, document: &ncrs_filing::rules_work::TriedDocument) -> String {
    let after = match &document.after {
        TryOutcome::Filed(path) | TryOutcome::Lies(path) => segments(path).join("/"),
        TryOutcome::Review(_) => text(ctx.lang, Msg::HermesChatRuleExampleReview),
    };
    fill(
        ctx.lang,
        Msg::HermesChatRuleExample,
        &[
            ("from", &segments(&document.path).join("/")),
            ("to", &after),
        ],
    )
}

pub fn trial_done(state: &mut State, ctx: &Ctx, tried: Result<RuleTrial, String>) {
    let trial = match tried {
        Ok(trial) => trial,
        Err(reason) => {
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                fallback_chips(ctx),
            );
            return;
        }
    };
    let Some(flow) = state.session.rule.as_ref() else {
        return;
    };
    let Some(spec) = flow.proposed.clone() else {
        return;
    };
    if trial.matches == 0 {
        ask_what(
            state,
            ctx,
            Some(text(ctx.lang, Msg::HermesChatRuleNoneMatch)),
        );
        return;
    }
    let kinds = flow.kinds.clone();
    let sentence = fill(
        ctx.lang,
        Msg::HermesChatRuleProposal,
        &[
            ("count", &number(trial.matches)),
            ("conditions", &condition_words(ctx, &spec, &kinds)),
            ("pattern", &flow.pattern()),
        ],
    );
    let mut parts = vec![
        Part::Text(sentence),
        Part::Levels(levels_card(ctx, flow, &kinds, None)),
    ];
    if !trial.examples.is_empty() {
        let lines: Vec<String> = trial
            .examples
            .iter()
            .map(|document| example_line(ctx, document))
            .collect();
        parts.push(Part::Text(lines.join("\n")));
    }
    let change = if flow.candidate.is_some() {
        ChipReply::LevelsChange
    } else {
        ChipReply::RuleEdit
    };
    let chips = vec![
        chip(ctx, Msg::HermesChatRuleCreate, ChipReply::RuleCreate, true),
        chip(ctx, Msg::HermesChatLevelsChange, change, false),
        cancel_chip(ctx),
    ];
    state.step = Step::RuleWhat;
    hermes(state, parts, chips);
}

/// The click on "Create rule".
pub fn create(state: &mut State) {
    let Some(spec) = state
        .session
        .rule
        .as_ref()
        .and_then(|flow| flow.proposed.clone())
    else {
        return;
    };
    work(state, ChatWork::SaveRule { spec }, None);
}

pub fn stored(state: &mut State, ctx: &Ctx, stored: Result<(), RuleSaveFailure>) {
    match stored {
        Ok(()) => {
            state.session.rule = None;
            state.step = Step::Idle;
            let mut chips = Vec::new();
            if !ctx.sources.is_empty() {
                chips.push(chip(
                    ctx,
                    Msg::HermesChatRuleFileNow,
                    ChipReply::RuleNow,
                    true,
                ));
            }
            chips.extend(fallback_chips(ctx));
            say(state, text(ctx.lang, Msg::HermesChatRuleStored), chips);
        }
        Err(RuleSaveFailure::Refused(problem)) => {
            let reason = fill(
                ctx.lang,
                Msg::HermesChatRuleRefused,
                &[("reason", &rule_problem_text(ctx.lang, &problem))],
            );
            ask_what(state, ctx, Some(reason));
        }
        Err(RuleSaveFailure::Failed(reason)) => say(
            state,
            fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
            fallback_chips(ctx),
        ),
    }
}

/// "File now": the usual tidy-up of the source folders.
pub fn file_now(state: &mut State, ctx: &Ctx) -> Option<Action> {
    state.step = Step::TidyFolder;
    match ctx.sources.as_slice() {
        [only] => folder_answer(state, ctx, only.clone(), false),
        _ => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatAskTidyFolder),
                source_chips(state, ctx),
            );
            None
        }
    }
}
