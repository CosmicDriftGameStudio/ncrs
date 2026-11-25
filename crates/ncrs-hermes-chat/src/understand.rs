//! Typed sentences: the local language model names what the user wants, and
//! Hermes asks before it does anything about it.

use iced::Task;
use ncrs_ai::{
    ChatIntent, ChatQuestion, ChatTurn, LocalModelStatus, ModelProblem, EVERY_AREA, LOCAL_MODEL,
};
use ncrs_i18n::Msg;
use ncrs_ui_kit::format;

use crate::action::Action;
use crate::ctx::{Availability, Ctx};
use crate::message::Message;
use crate::named_folders::{self, Purpose};
use crate::state::{Asked, Chip, ChipReply, Entry, Part, State, Thinking, Unsent};
use crate::texts::{fill, problem_text, text};
use crate::undo;
use crate::update::{
    ask_read_folder, cancel_chip, chip, drop_working, explain_not_yet, fallback, fallback_chips,
    handle_reply, open_thread, say, say_failed, say_model_failed, say_off, search_not_yet,
    sort_hint,
};

/// The question the newest bubble of Hermes asks and its buttons. Read before
/// the bubble of the user clears them.
pub fn asked(state: &State) -> Asked {
    let Some(Entry::Hermes { parts, chips }) = state.thread.last() else {
        return Asked::default();
    };
    let last_text = parts.iter().rev().find_map(|part| match part {
        Part::Text(sentence) => Some(sentence.clone()),
        _ => None,
    });
    // The buttons that offer what Hermes can do follow a statement, they are
    // not options of a question the model could answer.
    let only_ways_on = chips.iter().all(|chip| {
        matches!(
            chip.reply,
            ChipReply::IntentSearch
                | ChipReply::IntentTidy
                | ChipReply::IntentStructure
                | ChipReply::IntentUndo
                | ChipReply::PickFolder
                | ChipReply::OpenSort(_)
                | ChipReply::OpenRuleForm
        )
    });
    match last_text {
        Some(sentence) if !chips.is_empty() && !only_ways_on => Asked {
            question: Some(sentence),
            chips: chips.clone(),
        },
        _ => Asked::default(),
    }
}

/// Whether Hermes said why the model cannot be used, or the model is usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Usable,
    Said,
}

/// The one check before the model: the AI is on and the model is downloaded
/// and runs. Otherwise Hermes says why; the sentence that waited for it is
/// kept for when it is there.
pub fn gate(state: &mut State, ctx: &Ctx, unsent: Option<Unsent>) -> Gate {
    if !ctx.ai_on {
        keep_unsent(state, unsent);
        say(
            state,
            text(ctx.lang, Msg::HermesChatAiOff),
            vec![
                chip(ctx, Msg::HermesChatTurnOnAi, ChipReply::TurnOnAi, true),
                chip(ctx, Msg::HermesChatLater, ChipReply::Later, false),
            ],
        );
        return Gate::Said;
    }
    let Some(model) = ctx.model.as_ref() else {
        return Gate::Usable;
    };
    match model.status() {
        LocalModelStatus::NotDownloaded => {
            keep_unsent(state, unsent);
            say(
                state,
                fill(
                    ctx.lang,
                    Msg::HermesChatModelMissing,
                    &[("size", &format::megabytes(LOCAL_MODEL.size))],
                ),
                vec![
                    chip(
                        ctx,
                        Msg::HermesChatModelDownload,
                        ChipReply::DownloadModel,
                        true,
                    ),
                    chip(ctx, Msg::HermesChatLater, ChipReply::Later, false),
                ],
            );
            Gate::Said
        }
        LocalModelStatus::Failed(reason) => {
            say(
                state,
                crate::texts::problem_text(ctx.lang, ncrs_ai::ModelProblem::NotLoaded(reason)),
                fallback_chips(ctx),
            );
            Gate::Said
        }
        _ => Gate::Usable,
    }
}

fn keep_unsent(state: &mut State, unsent: Option<Unsent>) {
    if unsent.is_some() {
        state.session.unsent = unsent;
    }
}

pub fn ask(state: &mut State, ctx: &Ctx, typed: &str, asked: Asked) -> Option<Action> {
    let unsent = Unsent {
        message: typed.to_owned(),
        asked: asked.clone(),
    };
    if gate(state, ctx, Some(unsent)) == Gate::Said {
        return None;
    }
    let Some(model) = ctx.model.as_ref() else {
        fallback(state, ctx);
        return None;
    };
    let loaded = matches!(model.status(), LocalModelStatus::Loaded { .. });
    think(state, ctx, typed, asked, loaded);
    None
}

fn think(state: &mut State, ctx: &Ctx, typed: &str, asked: Asked, loaded: bool) {
    let question = asked.question.as_ref().map(|text| ChatQuestion {
        text: text.clone(),
        options: asked.chips.iter().map(|chip| chip.label.clone()).collect(),
    });
    let turn = ChatTurn {
        message: typed.to_owned(),
        question,
        previous_search: None,
    };
    let line = if loaded {
        Msg::HermesChatThinking
    } else {
        Msg::HermesChatWaking
    };
    state.thread.push(Entry::Working(text(ctx.lang, line)));
    state.thinking = Some(Thinking {
        turn,
        asked,
        discard_answer_on_drop: None,
    });
    state.fresh_turn = true;
}

/// The task that asks the model on a blocking thread.
pub fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh_turn) {
        return Task::none();
    }
    if state.thinking.is_none() {
        return Task::none();
    }
    let Some(model) = ctx.model.clone() else {
        // The model went away since the question: end the wait.
        state.thinking = None;
        drop_working(state);
        if gate(state, ctx, None) == Gate::Usable {
            fallback(state, ctx);
        }
        return Task::none();
    };
    let Some(thinking) = state.thinking.as_mut() else {
        return Task::none();
    };
    let turn = thinking.turn.clone();
    let (task, handle) = Task::perform(
        async move { tokio::task::spawn_blocking(move || model.understand(&turn)).await },
        |joined| match joined {
            Ok(Ok(intent)) => Message::Understood(Ok(intent)),
            Ok(Err(error)) => Message::Understood(Err(ModelProblem::from(&error))),
            Err(_join) => Message::Understood(Err(ModelProblem::Internal)),
        },
    )
    .abortable();
    thinking.discard_answer_on_drop = Some(handle.abort_on_drop());
    task
}

pub fn understood(
    state: &mut State,
    ctx: &Ctx,
    result: Result<ChatIntent, ModelProblem>,
) -> Option<Action> {
    let thinking = state.thinking.take()?;
    drop_working(state);
    if !matches!(result, Ok(ChatIntent::Answer { .. })) {
        // The talk moves on, and the button for the confirmed levels is gone.
        state.session.rule = None;
    }
    match result {
        Ok(intent) => act_on(state, ctx, intent, &thinking.asked),
        Err(problem) => {
            if gate(state, ctx, None) == Gate::Usable {
                say(state, problem_text(ctx.lang, problem), fallback_chips(ctx));
            }
            None
        }
    }
}

/// The model is there now.
pub fn model_ready(state: &mut State, ctx: &Ctx) -> Option<Action> {
    if let Some(model) = ctx.model.as_ref() {
        model.refresh();
    }
    if state.session.unsent.is_some() || state.greeting_owed {
        let usable = ctx.ai_on
            && ctx.model.as_ref().is_some_and(|model| {
                !matches!(
                    model.status(),
                    LocalModelStatus::NotDownloaded | LocalModelStatus::Failed(_)
                )
            });
        if usable {
            say(state, text(ctx.lang, Msg::HermesChatModelReady), Vec::new());
        }
    }
    resume(state, ctx)
}

/// Hermes will not come: what waited for it is dropped.
pub fn stop_waiting_for_hermes(state: &mut State) {
    state.waiting_for_hermes = false;
    drop_working(state);
    state.session.unsent = None;
    state.session.unsent_reply = None;
}

/// The filing module is ready: the greeting or the sentence that waited for
/// its start comes.
pub fn hermes_ready(state: &mut State, ctx: &Ctx) -> Option<Action> {
    if !state.waiting_for_hermes {
        return None;
    }
    match &ctx.availability {
        Availability::Starting => return None,
        Availability::Off => {
            stop_waiting_for_hermes(state);
            say_off(state, ctx);
            return None;
        }
        Availability::Failed(reason) => {
            stop_waiting_for_hermes(state);
            say_failed(state, ctx, reason);
            return None;
        }
        Availability::ModelFailed(problem) => {
            stop_waiting_for_hermes(state);
            say_model_failed(state, ctx, *problem);
            return None;
        }
        Availability::Ready(_) => {}
    }
    state.waiting_for_hermes = false;
    drop_working(state);
    if let Some(reply) = state.session.unsent_reply.take() {
        return handle_reply(state, ctx, reply);
    }
    if state.session.unsent.is_none() && state.thread.is_empty() {
        state.greeting_owed = true;
    }
    resume(state, ctx)
}

/// The AI is on now: whatever waited for it goes on, which offers the model
/// download when it is missing.
pub fn ai_turned_on(state: &mut State, ctx: &Ctx) -> Option<Action> {
    resume(state, ctx)
}

/// The sentence that waited goes in again, else the opening question that
/// waited comes.
fn resume(state: &mut State, ctx: &Ctx) -> Option<Action> {
    if let Some(unsent) = state.session.unsent.take() {
        state.greeting_owed = false;
        return ask(state, ctx, &unsent.message, unsent.asked);
    }
    if state.greeting_owed {
        open_thread(state, ctx);
    }
    None
}

fn act_on(state: &mut State, ctx: &Ctx, intent: ChatIntent, asked: &Asked) -> Option<Action> {
    match intent {
        ChatIntent::Answer { choice } => match asked.chips.get(choice) {
            Some(answer) if answer.reply.changes_something() => {
                confirm_typed_choice(state, ctx, answer.clone());
                None
            }
            Some(answer) => handle_reply(state, ctx, answer.reply.clone()),
            None => {
                fallback(state, ctx);
                None
            }
        },
        ChatIntent::TidyFolder { source, .. } => {
            look_for(state, ctx, &source, Purpose::Sort);
            None
        }
        ChatIntent::ChangeStructure { area, .. } => {
            if area == EVERY_AREA {
                ask_read_folder(state, ctx);
            } else {
                look_for(state, ctx, &area, Purpose::Open);
            }
            None
        }
        ChatIntent::Undo => {
            undo::ask(state, ctx);
            None
        }
        ChatIntent::Search(_) | ChatIntent::AskContent { .. } => {
            search_not_yet(state, ctx);
            None
        }
        ChatIntent::Explain { .. } => {
            explain_not_yet(state, ctx);
            None
        }
        ChatIntent::OpenFolder { folder } => {
            look_for(state, ctx, &folder, Purpose::Open);
            None
        }
        ChatIntent::BulkMove { .. } => {
            sort_hint(state, ctx, None);
            None
        }
        ChatIntent::SmallTalk => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatSmallTalk),
                fallback_chips(ctx),
            );
            None
        }
        ChatIntent::LeaveAsIs => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatLeaveAsIs),
                fallback_chips(ctx),
            );
            None
        }
        ChatIntent::Other => {
            fallback(state, ctx);
            None
        }
    }
}

/// Document text reaches the model, so a typed answer never does a change on
/// its own: the user taps the option to confirm it.
fn confirm_typed_choice(state: &mut State, ctx: &Ctx, answer: Chip) {
    let sentence = fill(
        ctx.lang,
        Msg::HermesChatTypedChoiceConfirm,
        &[("choice", &answer.label)],
    );
    say(state, sentence, vec![answer, cancel_chip(ctx)]);
}

/// Looks for the folder a sentence named.
fn look_for(state: &mut State, ctx: &Ctx, said: &str, purpose: Purpose) {
    if !named_folders::start(state, ctx, said, purpose) {
        fallback(state, ctx);
    }
}
