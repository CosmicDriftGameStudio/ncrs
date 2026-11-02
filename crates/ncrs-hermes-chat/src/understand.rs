//! Typed sentences: the local language model names what the user wants, and
//! Hermes asks before it does anything about it.

use std::path::PathBuf;

use iced::Task;
use ncrs_ai::{
    ChatIntent, ChatQuestion, ChatTurn, LocalModelStatus, Order, Schedule,
    Transfer as SaidTransfer, LOCAL_MODEL,
};
use ncrs_filing::chat_work::TimeUnit;
use ncrs_i18n::Msg;
use ncrs_index::Transfer;
use ncrs_ui_kit::format;

use crate::action::Action;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{Asked, Chip, ChipReply, Entry, Part, Proposal, State, Step, Thinking, Unsent};
use crate::texts::{fill, folder_name, text};
use crate::update::{
    ask_chips, batch_not_yet, cancel_chip, chip, drop_working, explain_not_yet, fallback,
    fallback_chips, handle_reply, resolve_folder_name, say, search_not_yet,
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

pub fn ask(state: &mut State, ctx: &Ctx, typed: &str, asked: Asked) -> Option<Action> {
    let Some(model) = ctx.model.as_ref() else {
        fallback(state, ctx);
        return None;
    };
    match model.status() {
        LocalModelStatus::NotDownloaded => {
            state.session.unsent = Some(Unsent {
                message: typed.to_owned(),
                asked,
            });
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
        }
        LocalModelStatus::Failed(reason) => {
            say(
                state,
                fill(ctx.lang, Msg::HermesChatModelFailed, &[("reason", reason)]),
                fallback_chips(ctx),
            );
        }
        status => {
            let loaded = matches!(status, LocalModelStatus::Loaded { .. });
            think(state, ctx, typed, asked, loaded);
        }
    }
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

/// The model is there now: the sentence that waited for it goes in again.
pub fn model_ready(state: &mut State, ctx: &Ctx) -> Option<Action> {
    if let Some(model) = ctx.model.as_ref() {
        model.refresh();
    }
    let unsent = state.session.unsent.take()?;
    ask(state, ctx, &unsent.message, unsent.asked)
}

fn act_on(state: &mut State, ctx: &Ctx, intent: ChatIntent, asked: &Asked) -> Option<Action> {
    match intent {
        ChatIntent::Answer { choice } => match asked.chips.get(choice) {
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
            if let Some(name) = unsupported_order(order) {
                say(
                    state,
                    fill(
                        ctx.lang,
                        Msg::HermesChatOrderNotYet,
                        &[("order", &text(ctx.lang, name))],
                    ),
                    Vec::new(),
                );
                return None;
            }
            state.session.change_scope = scope;
            learn::change_order(state, ctx, area, TimeUnit::Month);
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

/// The name of an order Hermes cannot do yet, `None` for the one it can.
fn unsupported_order(order: Order) -> Option<Msg> {
    match order {
        Order::ByMonth => None,
        Order::ByYear => Some(Msg::HermesChatOrderYear),
        Order::BySender => Some(Msg::HermesChatOrderSender),
        Order::ByKind => Some(Msg::HermesChatOrderKind),
        Order::Flat => Some(Msg::HermesChatOrderFlat),
    }
}

fn propose_tidy(
    state: &mut State,
    ctx: &Ctx,
    source: &str,
    target: Option<&str>,
    schedule: Option<Schedule>,
    transfer: Option<SaidTransfer>,
) -> Option<Action> {
    let Some(source_folder) = resolve_folder_name(state, ctx, source) else {
        fallback(state, ctx);
        return None;
    };
    let target_folder = match target.map(|name| resolve_folder_name(state, ctx, name)) {
        Some(None) => {
            fallback(state, ctx);
            return None;
        }
        Some(Some(folder)) => Some(folder),
        None => None,
    };
    state.session.proposal = Some(Proposal {
        source: source_folder,
        target: target_folder,
        schedule,
        transfer: transfer.map(|said| match said {
            SaidTransfer::Copy => Transfer::Copy,
            SaidTransfer::Move => Transfer::Move,
        }),
    });
    confirm_tidy(state, ctx);
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
    let source = folder_name(ctx, &proposal.source);
    let sentence = match &proposal.target {
        Some(target) => fill(
            ctx.lang,
            Msg::HermesChatConfirmTidyTo,
            &[
                ("source", &source),
                ("target", &folder_name(ctx, target)),
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
    crate::update::folder_answer(state, ctx, source, true)
}
