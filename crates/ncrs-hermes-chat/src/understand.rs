//! Typed sentences: the local language model names what the user wants, and
//! Hermes asks before it does anything about it.

use std::path::PathBuf;

use iced::Task;
use ncrs_ai::{
    ChatIntent, ChatQuestion, ChatTurn, LocalModelStatus, Order, Schedule,
    Transfer as SaidTransfer, LOCAL_MODEL,
};
use ncrs_filing::chat_work::ChangeOrder;
use ncrs_i18n::Msg;
use ncrs_index::Transfer;
use ncrs_ui_kit::format;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{Asked, Chip, ChipReply, Entry, Part, State, Step, Thinking, Unsent};
use crate::texts::{fill, text};
use crate::update::{
    ask_chips, batch_not_yet, cancel_chip, chip, drop_working, explain_not_yet, fallback,
    fallback_chips, handle_reply, open_thread, say, search_not_yet,
};
use crate::{learn, undo};

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
    match last_text {
        Some(sentence) if !chips.is_empty() => Asked {
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
                fill(ctx.lang, Msg::HermesChatModelFailed, &[("reason", reason)]),
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
    state.thinking = Some(Thinking { turn, asked });
    state.fresh_turn = true;
}

/// The task that asks the model on a blocking thread.
pub fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh_turn) {
        return Task::none();
    }
    let (Some(thinking), Some(model)) = (state.thinking.as_ref(), ctx.model.clone()) else {
        return Task::none();
    };
    let turn = thinking.turn.clone();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || model.understand(&turn)).await },
        |joined| match joined {
            Ok(Ok(intent)) => Message::Understood(Ok(intent)),
            Ok(Err(error)) => Message::Understood(Err(error.to_string())),
            Err(error) => Message::Understood(Err(error.to_string())),
        },
    )
}

pub fn understood(
    state: &mut State,
    ctx: &Ctx,
    result: Result<ChatIntent, String>,
) -> Option<Action> {
    let thinking = state.thinking.take()?;
    drop_working(state);
    match result {
        Ok(intent) => act_on(state, ctx, intent, &thinking.asked),
        Err(_) => {
            fallback(state, ctx);
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
        ChatIntent::TidyFolder {
            source,
            target,
            schedule,
            transfer,
        } => propose_tidy(state, ctx, &source, target.as_deref(), schedule, transfer),
        ChatIntent::ChangeStructure { order, area, scope } => {
            state.session.change_scope = scope;
            learn::change_order(state, ctx, area, change_order_of(order));
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
        ChatIntent::BulkMove { .. } => {
            batch_not_yet(state, ctx);
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

fn change_order_of(order: Order) -> ChangeOrder {
    match order {
        Order::ByMonth => ChangeOrder::ByMonth,
        Order::ByYear => ChangeOrder::ByYear,
        Order::BySender => ChangeOrder::BySender,
        Order::ByKind => ChangeOrder::ByKind,
        Order::Flat => ChangeOrder::Flat,
    }
}

fn propose_tidy(
    state: &mut State,
    ctx: &Ctx,
    source: &str,
    target: Option<&str>,
    schedule: Option<Schedule>,
    said_transfer: Option<SaidTransfer>,
) -> Option<Action> {
    let transfer = said_transfer.map(|said| match said {
        SaidTransfer::Copy => Transfer::Copy,
        SaidTransfer::Move => Transfer::Move,
    });
    if !crate::named_folders::start(state, ctx, source, target, schedule, transfer) {
        fallback(state, ctx);
    }
    None
}

/// "Source → target, move?" with the buttons to change what was understood.
pub fn confirm_tidy(state: &mut State, ctx: &Ctx) {
    let Some(proposal) = state.session.proposal.clone() else {
        return;
    };
    let mode = match proposal.transfer {
        Some(Transfer::Move) => text(ctx.lang, Msg::HermesChatConfirmMove),
        Some(Transfer::Copy) => text(ctx.lang, Msg::HermesChatConfirmCopy),
        _ => String::new(),
    };
    let source = proposal.source.display().to_string();
    let sentence = match &proposal.target {
        Some(target) => fill(
            ctx.lang,
            Msg::HermesChatConfirmTidyTo,
            &[
                ("source", &source),
                ("target", &target.display().to_string()),
                ("mode", &mode),
            ],
        ),
        None => fill(
            ctx.lang,
            Msg::HermesChatConfirmTidy,
            &[("source", &source), ("mode", &mode)],
        ),
    };
    let mut chips: Vec<Chip> = vec![
        chip(ctx, Msg::HermesChatConfirmYes, ChipReply::ConfirmTidy, true),
        chip(
            ctx,
            Msg::HermesChatOtherTarget,
            ChipReply::OtherTarget,
            false,
        ),
    ];
    match proposal.transfer {
        Some(Transfer::Move) => chips.push(chip(
            ctx,
            Msg::HermesChatCopy,
            ChipReply::SwitchToCopy,
            false,
        )),
        Some(Transfer::Copy) => chips.push(chip(
            ctx,
            Msg::HermesChatMove,
            ChipReply::SwitchToMove,
            false,
        )),
        _ => {}
    }
    chips.push(cancel_chip(ctx));
    state.step = Step::ConfirmTidy;
    say(state, sentence, chips);
}

pub fn switch_transfer(state: &mut State, ctx: &Ctx, transfer: Transfer) {
    if let Some(proposal) = state.session.proposal.as_mut() {
        proposal.transfer = Some(transfer);
    }
    confirm_tidy(state, ctx);
}

pub fn ask_other_target(state: &mut State, ctx: &Ctx) {
    state.step = Step::OtherTarget;
    say(
        state,
        text(ctx.lang, Msg::HermesChatAskOtherTarget),
        ask_chips(ctx),
    );
}

pub fn other_target(state: &mut State, ctx: &Ctx, target: PathBuf) {
    if let Some(proposal) = state.session.proposal.as_mut() {
        proposal.target = Some(target);
    }
    confirm_tidy(state, ctx);
}

/// "Yes" starts the usual way: read the folder, then the trial. Nothing is
/// filed before the trial is shown and confirmed.
pub fn confirm_tidy_yes(state: &mut State, ctx: &Ctx) -> Option<Action> {
    let source = state.session.proposal.as_ref()?.source.clone();
    state.step = Step::TidyFolder;
    crate::update::folder_answer(state, ctx, source, false)
}
