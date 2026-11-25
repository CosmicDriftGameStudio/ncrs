//! Recognising the order of a folder: an overview of its areas, the levels
//! the model finds, changing them, and the click that hands them to the rule
//! suggestions. Nothing is saved or moved here.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use ncrs_ai::{pattern_mismatches, Level, Levels, ModelProblem, Part as LevelPart};
use ncrs_filing::chat_work::FolderReading;
use ncrs_i18n::Msg;
use ncrs_index::{DocumentFields, Kinds};
use ncrs_intake::{label_text, render_pattern_in, PlaceholderLabel};
use ncrs_ui_kit::hermes_kit::LevelRow;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::learn;
use crate::message::Message;
use crate::state::{Chip, ChipReply, Entry, LevelsCard, Part, State, Step};
use crate::texts::{
    areas_card, fill, folder_name, number, problem_text, segments, text, SHOWN_AREAS,
};
use crate::understand::{gate, Gate};
use crate::update::{
    access, chip, chip_labelled, drop_working, fallback, fallback_chips, hermes, say,
};

mod ask;
mod change;

pub use ask::{RuleAnswer, RuleAsk};
pub use change::{levels_change, levels_discard, levels_more};

const DEFAULT_LEVELS: &str = "{year}/{date}_{sender}_{kind}";
/// Files the model sees: the prefix of its prompt grows with them.
const MODEL_FILES: usize = 6;
const MISFITS_SHOWN: usize = 3;
/// Different values the levels card shows per level.
const FOLDER_VALUES: usize = 3;
const FILE_VALUES: usize = 2;
/// Below this share of fitting files a recognized pattern is not offered as it is.
const FIT_AT_LEAST_PERCENT: usize = 80;

/// The folder whose order is being recognised. Patterns and paths are
/// relative to it.
#[derive(Debug, Clone)]
pub struct RuleFlow {
    /// The folder's name, and where it lies.
    folder: String,
    root: PathBuf,
    levels: Levels,
    /// The levels as the model found them (or the standard): what "discard"
    /// goes back to and what a changed level is marked against.
    found: Levels,
    /// The files of the folder: path below the folder, and fields.
    files: Vec<(String, DocumentFields)>,
    /// Set while the overview on screen leaves areas out.
    overview: Option<Arc<FolderReading>>,
    /// Read once when the flow starts.
    kinds: Kinds,
}

impl RuleFlow {
    fn new(folder: String, root: PathBuf, kinds: Kinds) -> Self {
        Self {
            folder,
            root,
            levels: default_levels(),
            found: default_levels(),
            files: Vec::new(),
            overview: None,
            kinds,
        }
    }

    /// Without levels the files get their automatic name in the folder itself.
    fn pattern(&self) -> String {
        if self.levels.count() == 0 {
            return String::new();
        }
        self.levels.pattern()
    }

    /// The pattern the user confirmed; `None` when no order was recognised:
    /// no level at all, or levels as found that fit too few of the files.
    fn confirmed_pattern(&self) -> Option<String> {
        if self.levels.count() == 0 {
            return None;
        }
        let misfits = pattern_mismatches(&self.pattern(), &self.files, &self.kinds).len();
        let poor = self.levels == self.found && fits_poorly(misfits, self.files.len());
        (!poor).then(|| self.pattern())
    }
}

fn fits_poorly(misfits: usize, total: usize) -> bool {
    total.saturating_sub(misfits) * 100 < total * FIT_AT_LEAST_PERCENT
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
        .and_then(|access| access.try_kinds().ok())
        .unwrap_or_else(|| learn::word_kinds(ctx))
}

/// Reads a folder for its order.
pub fn open(state: &mut State, ctx: &Ctx, folder: PathBuf) {
    state.session.rule = Some(RuleFlow::new(
        folder_name(ctx, &folder),
        folder.clone(),
        kinds_of(ctx),
    ));
    state.step = Step::Idle;
    learn::read(state, ctx, folder);
}

pub fn folder_read(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
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
    let areas = reading.structure.areas.len();
    flow.overview = (areas > SHOWN_AREAS).then(|| Arc::clone(reading));
    let files = sample(&flow.files, MODEL_FILES);
    let all = flow.files.clone();
    let kinds = flow.kinds.clone();
    let mut parts = Vec::new();
    if areas > 0 {
        parts.push(Part::Text(overview_intro(ctx, reading)));
        parts.push(areas_part(ctx, reading, false));
    }
    parts.extend(learn::ai_progress(ctx, reading));
    if !parts.is_empty() {
        hermes(state, parts, Vec::new());
    }
    ask_model(state, ctx, RuleAsk::Recognize { files, all, kinds });
}

fn overview_intro(ctx: &Ctx, reading: &FolderReading) -> String {
    let count = number(reading.files.len());
    match reading.structure.areas.len() {
        1 => fill(
            ctx.lang,
            Msg::HermesChatReadSummaryOneArea,
            &[("count", &count)],
        ),
        areas => fill(
            ctx.lang,
            Msg::HermesChatReadSummary,
            &[("count", &count), ("areas", &number(areas))],
        ),
    }
}

fn areas_part(ctx: &Ctx, reading: &FolderReading, all: bool) -> Part {
    Part::Areas(areas_card(
        ctx.lang,
        &folder_name(ctx, &reading.root),
        reading,
        all,
    ))
}

/// The overview with every area; the levels follow again with their buttons.
pub fn show_all(state: &mut State, ctx: &Ctx) {
    let Some(reading) = state
        .session
        .rule
        .as_mut()
        .and_then(|flow| flow.overview.take())
    else {
        return;
    };
    hermes(state, vec![areas_part(ctx, &reading, true)], Vec::new());
    show_levels_checked(state, ctx, true);
}

/// Asks the model. Without one (AI off, model missing, none at all) the usual
/// offer is said instead and the flow is over, so nothing of it is left to
/// answer a later sentence.
fn ask_model(state: &mut State, ctx: &Ctx, ask: RuleAsk) {
    let usable = gate(state, ctx, None) == Gate::Usable;
    if usable && ctx.model.is_none() {
        fallback(state, ctx);
    }
    if !usable || ctx.model.is_none() {
        state.session.rule = None;
        state.step = Step::Idle;
        return;
    }
    state
        .thread
        .push(Entry::Working(text(ctx.lang, Msg::HermesChatThinking)));
    state.rule_ask = Some(ask);
    state.fresh_rule_ask = true;
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
            Err(_join) => Message::RuleAnswered(RuleAnswer::Failed(ModelProblem::Internal)),
        },
    )
}

pub fn answered(state: &mut State, ctx: &Ctx, answer: RuleAnswer) {
    state.rule_ask = None;
    drop_working(state);
    if state.session.rule.is_none() {
        return;
    }
    match answer {
        RuleAnswer::Levels(Ok(levels)) => {
            if let Some(flow) = state.session.rule.as_mut() {
                flow.levels = levels.clone();
                flow.found = levels;
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
        RuleAnswer::Levels(Err(problem)) | RuleAnswer::Failed(problem) => {
            state.session.rule = None;
            state.step = Step::Idle;
            if gate(state, ctx, None) == Gate::Usable {
                say(state, problem_text(ctx.lang, problem), fallback_chips(ctx));
            }
        }
    }
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
        moves: None,
        footer,
    }
}

/// The card of the levels, and under it the files that do not follow them.
fn levels_parts(state: &State, ctx: &Ctx, poor_fit: bool) -> (Vec<Part>, bool) {
    let Some(flow) = state.session.rule.as_ref() else {
        return (Vec::new(), false);
    };
    let kinds = flow.kinds.clone();
    let total = flow.files.len();
    let misfits = pattern_mismatches(&flow.pattern(), &flow.files, &kinds);
    let fitting = total - misfits.len();
    let footer = (!misfits.is_empty()).then(|| {
        fill(
            ctx.lang,
            Msg::HermesChatLevelsFit,
            &[("fitting", &number(fitting)), ("total", &number(total))],
        )
    });
    let mut parts = vec![Part::Levels(levels_card(ctx, flow, &kinds, footer, None))];
    let poor = poor_fit && fits_poorly(misfits.len(), total);
    if poor {
        parts.push(Part::Text(fill(
            ctx.lang,
            Msg::HermesChatLevelsPoor,
            &[("fitting", &number(fitting)), ("total", &number(total))],
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

fn suggest_chip(ctx: &Ctx, primary: bool) -> Chip {
    chip(
        ctx,
        Msg::HermesChatSuggestRules,
        ChipReply::SuggestRules,
        primary,
    )
}

/// `checked`: the levels are the model's, so a pattern that fits too few
/// files is not offered as it is.
fn show_levels_checked(state: &mut State, ctx: &Ctx, checked: bool) {
    let (parts, poor) = levels_parts(state, ctx, checked);
    let Some(flow) = state.session.rule.as_ref() else {
        return;
    };
    let mut chips = if poor {
        vec![
            chip(
                ctx,
                Msg::HermesChatLevelsChange,
                ChipReply::LevelsChange,
                true,
            ),
            suggest_chip(ctx, false),
        ]
    } else {
        levels_chips(ctx, !flow.files.is_empty())
    };
    if let Some(reading) = &flow.overview {
        chips.push(chip_labelled(
            fill(
                ctx.lang,
                Msg::HermesChatShowAll,
                &[("areas", &number(reading.structure.areas.len()))],
            ),
            ChipReply::ShowAll,
            false,
        ));
    }
    state.step = Step::RuleLevels;
    hermes(state, parts, chips);
}

/// The standard is a change of the levels found like any other: it is shown
/// marked against them, with what the files would be called and the way back.
pub fn levels_default(state: &mut State, ctx: &Ctx) {
    let Some(flow) = state.session.rule.as_mut() else {
        return;
    };
    flow.levels = default_levels();
    change::show_change(state, ctx, false);
}

/// The levels are taken as they stand: the rule suggestions are the next step.
pub fn levels_kept(state: &mut State, ctx: &Ctx) {
    let Some(flow) = state.session.rule.as_ref() else {
        return;
    };
    let sentence = fill(
        ctx.lang,
        Msg::HermesChatLevelsConfirmed,
        &[("folder", &flow.folder)],
    );
    state.step = Step::Idle;
    say(state, sentence, vec![suggest_chip(ctx, true)]);
}

/// Hands the folder and its order to the rule suggestions: the recognition is
/// over, the app opens the dialog.
pub fn suggest_rules(state: &mut State, ctx: &Ctx) -> Option<Action> {
    let flow = state.session.rule.take()?;
    state.step = Step::Idle;
    say(
        state,
        text(ctx.lang, Msg::HermesChatSuggestionsOpen),
        Vec::new(),
    );
    Some(Action::SuggestRules {
        pattern: flow.confirmed_pattern(),
        folder: flow.root,
    })
}

/// Typed words while the levels are shown change them.
pub fn typed(state: &mut State, ctx: &Ctx, typed: &str) {
    match change::level_reference(typed) {
        Some(level) => change::ask_level(state, ctx, level),
        None => change::revise(state, ctx, typed),
    }
}

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
