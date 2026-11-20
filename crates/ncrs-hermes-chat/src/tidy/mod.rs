//! Conversation 2: tidying a source folder into a target, with a trial first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ncrs_ai::Schedule;
use ncrs_filing::chat_work::{
    AnsweredGroups, ChatWork, FolderReading, ModelScope, RunItem, RunPlan, TrialEntry,
    TrialOutcome, TrialPlan,
};
use ncrs_filing::rules_work::ArchiveMove;
use ncrs_i18n::Msg;
use ncrs_index::Transfer;

use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::learn;
use crate::state::{Chip, ChipReply, Part, ReadFor, State, Step, Tidy, TrialSession};
use crate::texts::{fill, folder_name, number, text};
use crate::update::{access, chip, chip_labelled, hermes, say, work};

mod filed;

pub use filed::{archive_run, filed_done, trash_offer, trashed_done};

pub fn targets(state: &State, ctx: &Ctx) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for path in ctx.targets.iter().chain(&state.session.learned) {
        if !found.contains(path) {
            found.push(path.clone());
        }
    }
    found
}

pub fn begin(state: &mut State, ctx: &Ctx, source: PathBuf) {
    state.session.run_chain.clear();
    let is_target = targets(state, ctx)
        .iter()
        .any(|target| source.starts_with(target));
    if is_target {
        state.session.proposal = None;
        state.step = Step::Idle;
        say(
            state,
            fill(
                ctx.lang,
                Msg::HermesChatSourceIsTarget,
                &[("folder", &folder_name(ctx, &source))],
            ),
            crate::update::lost_folder_chips(ctx),
        );
        return;
    }
    state.session.rule_queue = None;
    state.session.tidy = Some(Tidy {
        rule_now: None,
        source: source.clone(),
        target: None,
        transfer: None,
        inside: Vec::new(),
    });
    state.session.layout = None;
    state.session.deciding = None;
    state.step = Step::Idle;
    learn::read_for(state, ctx, source, ReadFor::Source);
}

pub fn source_read(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
    let folder = folder_name(ctx, &reading.root);
    if reading.files.is_empty() {
        if crate::rule_flow::trial::empty_source(state, ctx) {
            return;
        }
        state.session.tidy = None;
        state.session.proposal = None;
        state.step = Step::Idle;
        say(
            state,
            fill(ctx.lang, Msg::HermesChatReadEmpty, &[("folder", &folder)]),
            crate::update::lost_folder_chips(ctx),
        );
        return;
    }
    if state
        .session
        .tidy
        .as_ref()
        .is_some_and(|tidy| tidy.rule_now.is_some())
    {
        choose_where(state, ctx, reading);
        return;
    }
    crate::photos::ask_or_go_on(state, ctx, reading);
}

/// Where the files of the source go: the target the sentence named, the only
/// target there is, or a question.
pub fn choose_where(state: &mut State, ctx: &Ctx, reading: &Arc<FolderReading>) {
    let folder = folder_name(ctx, &reading.root);
    if let Some(target) = state
        .session
        .tidy
        .as_ref()
        .filter(|tidy| tidy.rule_now.is_some())
        .and_then(|tidy| tidy.target.clone())
    {
        choose_target(state, ctx, target);
        return;
    }
    if let Some(target) = state
        .session
        .proposal
        .as_ref()
        .and_then(|proposal| proposal.target.clone())
    {
        state.step = Step::Target;
        crate::update::folder_answer(state, ctx, target, false);
        return;
    }
    let candidates = targets(state, ctx);
    if candidates.is_empty() {
        state.session.resume_tidy = true;
        state.session.read_for = Some(ReadFor::Learn);
        state.step = Step::ReadFolder;
        say(
            state,
            text(ctx.lang, Msg::HermesChatNoTarget),
            crate::update::target_chips(state, ctx),
        );
        return;
    }
    state.step = Step::Target;
    let chips = target_question_chips(ctx, candidates);
    say(
        state,
        fill(
            ctx.lang,
            Msg::HermesChatSourceSummary,
            &[("folder", &folder), ("count", &number(reading.files.len()))],
        ),
        chips,
    );
}

/// The buttons of the question where the files go: the known targets, a new
/// folder, or deciding file by file.
pub fn target_question_chips(ctx: &Ctx, candidates: Vec<PathBuf>) -> Vec<Chip> {
    let only_one = candidates.len() == 1;
    let mut chips = candidates
        .into_iter()
        .map(|path| chip_labelled(folder_name(ctx, &path), ChipReply::Folder(path), only_one))
        .collect::<Vec<_>>();
    chips.push(chip(
        ctx,
        Msg::HermesChatNewFolderChip,
        ChipReply::NewTarget,
        false,
    ));
    chips.push(chip(
        ctx,
        Msg::HermesChatDecideChip,
        ChipReply::DecidePerFile,
        false,
    ));
    chips
}

/// The folder dialog of "New folder" was closed: the question where the files
/// go is asked again.
pub fn ask_target_again(state: &mut State, ctx: &Ctx) {
    state.step = Step::Target;
    let chips = target_question_chips(ctx, targets(state, ctx));
    say(state, text(ctx.lang, Msg::HermesChatAskOtherTarget), chips);
}

/// The target was just learned: go on with the tidy-up it interrupted.
pub fn resume(state: &mut State, ctx: &Ctx) {
    state.session.resume_tidy = false;
    let Some(target) = state.session.learned.last().cloned() else {
        return;
    };
    choose_target(state, ctx, target);
}

pub fn choose_target(state: &mut State, ctx: &Ctx, target: PathBuf) {
    let Some(tidy) = state.session.tidy.as_mut() else {
        return;
    };
    tidy.target = Some(target.clone());
    state.step = Step::Idle;
    learn::read_for(state, ctx, target, ReadFor::Target);
}

pub fn target_read(state: &mut State, ctx: &Ctx) {
    let Some(tidy) = state.session.tidy.as_ref() else {
        return;
    };
    if tidy.rule_now.is_some() {
        mode(state, ctx, Transfer::Move);
        return;
    }
    if let Some(schedule) = state
        .session
        .proposal
        .as_ref()
        .and_then(|proposal| proposal.schedule)
    {
        once(state, ctx, schedule == Schedule::Always);
        return;
    }
    let folder = folder_name(ctx, &tidy.source);
    state.step = Step::Once;
    say(
        state,
        fill(ctx.lang, Msg::HermesChatAskOnce, &[("folder", &folder)]),
        vec![
            chip(ctx, Msg::HermesChatOnce, ChipReply::Once, true),
            chip(ctx, Msg::HermesChatAlways, ChipReply::Always, false),
        ],
    );
}

pub fn once(state: &mut State, ctx: &Ctx, always: bool) {
    let Some(tidy) = state.session.tidy.as_ref() else {
        return;
    };
    let said = state
        .session
        .proposal
        .as_ref()
        .and_then(|proposal| proposal.transfer);
    let (source, chosen_target) = (tidy.source.clone(), tidy.target.clone());
    let watch_source = always && !ctx.sources.contains(&source);
    let deciding = state.session.deciding.is_some();
    if always {
        if deciding && watch_source {
            state.queued_action = Some(Action::SetFolderRole {
                folder: tidy.source.clone(),
                role: FolderRole::Inbox,
            });
        }
        let shown = folder_name(ctx, &tidy.source);
        say(
            state,
            fill(ctx.lang, Msg::HermesChatAlwaysSet, &[("folder", &shown)]),
            Vec::new(),
        );
    }
    if deciding {
        match said {
            Some(transfer) => mode(state, ctx, transfer),
            None => ask_mode(state, ctx),
        }
        return;
    }
    let Some(target) = chosen_target else {
        return;
    };
    state.session.tidy = None;
    state.session.proposal = None;
    state.session.readings.clear();
    crate::in_tab::hand_over(state, ctx);
    state.queued_action = Some(Action::OpenFiling {
        source,
        target,
        watch_source,
    });
}

/// What a read folder is set to wins over what was asked: files of a folder
/// that is copied are never moved out of it.
pub fn transfer_from(ctx: &Ctx, source: &Path, wanted: Transfer) -> Transfer {
    if wanted == Transfer::Move && access(ctx).is_some_and(|access| access.copies_from(source)) {
        Transfer::Copy
    } else {
        wanted
    }
}

pub fn ask_mode(state: &mut State, ctx: &Ctx) {
    let Some(tidy) = state.session.tidy.as_ref() else {
        return;
    };
    if transfer_from(ctx, &tidy.source, Transfer::Move) == Transfer::Copy {
        mode(state, ctx, Transfer::Copy);
        return;
    }
    let folder = folder_name(ctx, &tidy.source);
    state.step = Step::Mode;
    hermes(
        state,
        vec![
            Part::Text(fill(
                ctx.lang,
                Msg::HermesChatAskMode,
                &[("folder", &folder)],
            )),
            Part::Text(fill(
                ctx.lang,
                Msg::HermesChatModeConsequence,
                &[("folder", &folder)],
            )),
        ],
        vec![
            chip(ctx, Msg::HermesChatCopy, ChipReply::ModeCopy, false),
            chip(ctx, Msg::HermesChatMove, ChipReply::ModeMove, false),
        ],
    );
}

pub fn mode(state: &mut State, ctx: &Ctx, chosen: Transfer) {
    let transfer = state
        .session
        .tidy
        .as_ref()
        .map_or(chosen, |tidy| transfer_from(ctx, &tidy.source, chosen));
    if state.session.deciding.is_some() {
        crate::decide::start(state, ctx, transfer);
        return;
    }
    let Some(tidy) = state.session.tidy.as_mut() else {
        return;
    };
    tidy.transfer = Some(transfer);
    state.session.proposal = None;
    let (Some(source), Some(target)) = (
        state.session.readings.get(&tidy.source).cloned(),
        tidy.target
            .as_ref()
            .and_then(|target| state.session.readings.get(target).cloned()),
    ) else {
        crate::update::cancel(state, ctx);
        return;
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::Trial {
            source,
            target,
            answered: AnsweredGroups::default(),
            layout: state.session.layout.clone(),
            scope: ModelScope::FirstPage,
        },
        Some(text(ctx.lang, Msg::HermesChatWorkingTrial)),
    );
}

pub fn trial_done(state: &mut State, ctx: &Ctx, result: Result<TrialPlan, String>) {
    let plan = match result {
        Ok(plan) => plan,
        Err(reason) => {
            state.session.rule_queue = None;
            state.step = Step::Idle;
            say(
                state,
                fill(ctx.lang, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                crate::update::fallback_chips(ctx),
            );
            return;
        }
    };
    // A model that cannot choose folders counts as no model: the trial goes
    // on, after saying so once.
    if matches!(plan.model_failure, Some(ncrs_ai::AiError::Unsupported)) {
        say(
            state,
            crate::texts::problem_text(ctx.lang, ncrs_ai::ModelProblem::Unsupported),
            Vec::new(),
        );
    }
    if let Some(failure) = plan
        .model_failure
        .as_ref()
        .filter(|failure| !matches!(failure, ncrs_ai::AiError::Unsupported))
    {
        // The files the model did not answer for would show up as "stays"
        // in the group questions, as if the model had said so.
        state.session.rule_queue = None;
        state.step = Step::Idle;
        say(
            state,
            crate::texts::problem_text(ctx.lang, ncrs_ai::ModelProblem::from(failure)),
            crate::update::fallback_chips(ctx),
        );
        return;
    }
    state.session.trial = Some(TrialSession { plan });
    crate::rule_flow::trial::file_rule_trial(state, ctx);
}

/// The archive documents of this target as items of the run.
pub fn inside_items(moves: &[ArchiveMove], target: &Path) -> Vec<RunItem> {
    moves
        .iter()
        .filter(|found| found.root == target)
        .map(|found| RunItem {
            from: found.root.join(&found.from),
            to: found.to.clone(),
            hash: found.hash,
        })
        .collect()
}

/// The name the file gets in the folder chosen for it: the one rendered for
/// that folder, else its own.
fn chosen_name<'a>(entry: &'a TrialEntry, folder: &Path) -> &'a str {
    let TrialOutcome::Unsure {
        choices,
        file_names,
        ..
    } = &entry.outcome
    else {
        return &entry.name;
    };
    choices
        .iter()
        .position(|choice| choice == folder)
        .and_then(|position| file_names.get(position))
        .map_or(entry.name.as_str(), String::as_str)
}

pub fn file_all(state: &mut State, ctx: &Ctx) {
    let (Some(trial), Some(tidy)) = (state.session.trial.as_mut(), state.session.tidy.as_ref())
    else {
        return;
    };
    let (Some(transfer), Some(target)) = (tidy.transfer, tidy.target.clone()) else {
        return;
    };
    let mut items = Vec::new();
    let entries = trial.plan.entries.clone();
    let rule_prefix = tidy.rule_now.as_deref();
    for entry in &entries {
        if !crate::rule_flow::trial::rule_keeps(rule_prefix, &entry.outcome) {
            continue;
        }
        let to = match &entry.outcome {
            TrialOutcome::Filed { to } => to.clone(),
            TrialOutcome::Unsure { choices, .. } => {
                // A row the user did not touch goes to its first choice.
                let Some(folder) = choices.first().cloned() else {
                    continue;
                };
                let to = folder.join(chosen_name(entry, &folder));
                if !trial.plan.claim(&to) {
                    continue;
                }
                to
            }
            _ => continue,
        };
        items.push(RunItem {
            from: entry.from.clone(),
            to,
            hash: entry.hash,
        });
    }
    if items.is_empty() && inside_items(&tidy.inside, &target).is_empty() {
        let (source, moves) = (tidy.source.clone(), tidy.inside.clone());
        archive_run(state, ctx, source, moves);
        return;
    }
    let folder = folder_name(ctx, &tidy.source);
    let run = RunPlan {
        name: fill(ctx.lang, Msg::HermesChatRunName, &[("folder", &folder)]),
        transfer,
        source_root: tidy.source.clone(),
        inside: inside_items(&tidy.inside, &target),
        target_root: target,
        items,
    };
    state.step = Step::Idle;
    work(
        state,
        ChatWork::File { run },
        Some(text(ctx.lang, Msg::HermesChatWorkingFiling)),
    );
}
