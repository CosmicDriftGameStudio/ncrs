//! The message loop and the routing of what the user typed or pressed.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use iced::Task;
use ncrs_filing::chat_work::{ChatAccess, ChatResult};
use ncrs_i18n::Msg;
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::chat_store::Speaker;
use crate::ctx::{Availability, Ctx};
pub use crate::fallback::{
    batch_not_yet, explain_not_yet, fallback, fallback_chips, lost_folder_chips,
    lost_folder_retry_chips, search_not_yet,
};
mod chips;
use crate::message::Message;
use crate::state::{Asked, ChipReply, Entry, Part, ReadFor, Retry, State, Step, Unsent};
use crate::texts::{fill, folder_name, text};
use crate::{learn, named_folders, photos, tidy, trial_end, understand, undo};
pub use chips::{
    ask_chips, cancel_chip, candidate_folders, chip, chip_labelled, clear_chips, hermes, is_known,
    is_target_folder, mention_of, say, source_chips, target_chips, user, user_folder, well_known,
    work,
};

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    if !ctx.ai_on {
        state.picks_stop.store(true, Ordering::Relaxed);
    }
    let action = route(state, message, ctx).or_else(|| state.queued_action.take());
    if state.restart_when_idle && !is_busy(state) {
        restart_conversation(state, ctx);
    }
    state.sync_chats(ctx.now);
    (launch(state, ctx), action)
}

fn route(state: &mut State, message: Message, ctx: &Ctx) -> Option<Action> {
    if state.chats.is_read_only() {
        match message {
            // The buttons of an old chat are gone; only its text is kept.
            Message::ChipPressed { .. } => return None,
            Message::Submit => {
                let waits = state.input.trim().is_empty() || is_busy(state);
                if waits || !continue_old_chat(state, ctx) {
                    return None;
                }
            }
            _ => {}
        }
    }
    match message {
        Message::List(list_message) => crate::chat_list_update::handle(state, list_message, ctx),
        Message::Open => {
            state.open = true;
            if !is_busy(state) && state.session.rule.take().is_some() {
                clear_chips(state);
                if matches!(state.step, Step::RuleLevels | Step::RuleWhat) {
                    state.step = Step::Idle;
                }
            }
            if state.thread.is_empty() {
                open_thread(state, ctx);
                None
            } else {
                understand::hermes_ready(state, ctx)
            }
        }
        Message::OpenAbout(items) => open_about(state, items, ctx),
        Message::Close => {
            // Escape first closes what the list has opened.
            if state.chats.dismiss_open_thing() {
                return None;
            }
            state.open = false;
            Some(Action::Close)
        }
        Message::NewChat => {
            new_chat(state, ctx);
            None
        }
        Message::IndexReplaced => {
            if is_busy(state) {
                state.restart_when_idle = true;
            } else {
                restart_conversation(state, ctx);
            }
            None
        }
        Message::InputChanged(input) => {
            state.input = input;
            None
        }
        Message::Submit => {
            let typed = std::mem::take(&mut state.input).trim().to_owned();
            if typed.is_empty() || is_busy(state) {
                return None;
            }
            let asked = understand::asked(state);
            if looks_like_typed_path(&typed) && !words_are_taken_as_text(state.step) {
                user_folder(state, ctx, &expand_home(ctx, Path::new(&typed)));
            } else {
                user(state, typed.clone());
            }
            let unsent = Unsent {
                message: typed.clone(),
                asked: asked.clone(),
            };
            if hold_until_ready(state, ctx, Held::Sentence(unsent)) {
                return None;
            }
            handle_text(state, ctx, &typed, asked)
        }
        Message::ChipPressed { label, reply } => {
            if is_busy(state) {
                return None;
            }
            match &reply {
                ChipReply::Folder(path) | ChipReply::NamedFolder(path) => {
                    user_folder(state, ctx, path);
                }
                _ => {
                    user(state, label);
                }
            }
            if !reply.needs_no_hermes() && hold_until_ready(state, ctx, Held::Reply(reply.clone()))
            {
                return None;
            }
            handle_reply(state, ctx, reply)
        }
        Message::Done(result) => {
            finish_work(state);
            handle_done(state, ctx, *result)
        }
        Message::Failed(reason) => {
            finish_work(state);
            state.session.greeting_after_count = false;
            say(
                state,
                text_with(ctx, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                fallback_chips(ctx),
            );
            None
        }
        Message::FolderPicked(picked) => {
            if is_busy(state) {
                return None;
            }
            let Some(path) = picked else {
                let step = state.step;
                if step == Step::WaitingFolder && crate::waiting::ask_again(state, ctx) {
                    return None;
                }
                if step == Step::NewTarget {
                    tidy::ask_target_again(state, ctx);
                    return None;
                }
                reset_flow(state);
                say(
                    state,
                    text(ctx.lang, Msg::HermesChatLaterReply),
                    fallback_chips(ctx),
                );
                return None;
            };
            user_folder(state, ctx, &path);
            if hold_until_ready(state, ctx, Held::Reply(ChipReply::Folder(path.clone()))) {
                return None;
            }
            folder_answer(state, ctx, path, false)
        }
        Message::FolderGranted { folder, result } => match result {
            Ok(()) => route(state, Message::FolderPicked(Some(folder)), ctx),
            Err(reason) => route(state, Message::Failed(reason), ctx),
        },
        Message::TurnOnFailed(reason) => {
            understand::stop_waiting_for_hermes(state);
            say(
                state,
                text_with(ctx, Msg::HermesChatTurnOnFailed, &[("reason", &reason)]),
                vec![chip(ctx, Msg::HermesChatRetry, ChipReply::TurnOn, true)],
            );
            None
        }
        Message::TypedFolderResolved {
            typed,
            step,
            result,
        } => typed_folder_resolved(state, ctx, &typed, step, result),
        Message::NamedFoldersResolved { source, target } => {
            named_folders::resolved(state, ctx, source, target);
            None
        }
        Message::RuleAnswered(answer) => crate::rule_flow::answered(state, ctx, answer),
        Message::Understood(result) => understand::understood(state, ctx, result),
        Message::ModelReady => understand::model_ready(state, ctx),
        Message::AiTurnedOn => understand::ai_turned_on(state, ctx),
        Message::HermesReady => understand::hermes_ready(state, ctx),
        Message::ChangeArea { folder, area } => change_area(state, ctx, folder, area),
        Message::WaitingCheck => {
            crate::waiting::check(state, ctx);
            None
        }
        Message::WorkTick => {
            crate::working::tick(state, ctx);
            None
        }
        Message::ReadingSettled(sequence) => {
            state.picks_job = crate::picks::begin(state, ctx, sequence);
            None
        }
        Message::PicksRemembered => {
            state.picks_running = false;
            None
        }
    }
}

/// The settings page asks for the change of one area: the folder is read
/// and the orders for the area are on offer.
fn change_area(state: &mut State, ctx: &Ctx, folder: PathBuf, area: String) -> Option<Action> {
    state.open = true;
    let fresh = state.thread.is_empty();
    if fresh {
        open_thread(state, ctx);
    }
    if !matches!(ctx.availability, Availability::Ready(_)) {
        if !fresh {
            match &ctx.availability {
                Availability::Off => say_off(state, ctx),
                Availability::Failed(reason) => say_failed(state, ctx, reason),
                Availability::ModelFailed(problem) => say_model_failed(state, ctx, *problem),
                Availability::Starting | Availability::Ready(_) => {}
            }
        }
        return None;
    }
    if is_busy(state) {
        return None;
    }
    let area_folder = folder.join(area);
    if !crate::rule_flow::open(state, ctx, area_folder) {
        learn::read(state, ctx, folder);
    }
    None
}

/// The conversation starts over as when Hermes was opened the first time:
/// thread and flow are gone, the settings, the index and the rules are not
/// touched. While work runs, its result would land in the new chat, so the
/// button waits for it. The language model is the exception: a slow answer
/// must not hold the chat, and it is dropped with the old conversation.
fn new_chat(state: &mut State, ctx: &Ctx) {
    let thinking = state.thinking.take();
    if is_busy(state) {
        state.thinking = thinking;
        // The running conversation cannot be left; at least it is shown.
        state.chats.show_live();
        return;
    }
    drop(thinking);
    restart_conversation(state, ctx);
}

/// Starts a new conversation. The old one stays in the list as a closed chat.
pub fn restart_conversation(state: &mut State, ctx: &Ctx) {
    if state.chats.live().is_some() {
        state.sync_chats(ctx.now);
    }
    let chats = std::mem::take(&mut state.chats);
    // The background job is not part of the conversation: a second one must
    // not start beside it.
    *state = State {
        open: state.open,
        picks_running: state.picks_running,
        picks_stop: Arc::clone(&state.picks_stop),
        picks_settled_at: state.picks_settled_at,
        chats,
        ..State::default()
    };
    state.chats.start_live(ctx.now.timestamp());
    open_thread(state, ctx);
}

/// Writing in an old chat goes on in it: its saved turns become the thread
/// of a fresh conversation, as plain text. Nothing of the old run is
/// restored as something to press or run, and the chat keeps its id.
fn continue_old_chat(state: &mut State, ctx: &Ctx) -> bool {
    if state.chats.live().is_some() {
        state.sync_chats(ctx.now);
    }
    let input = std::mem::take(&mut state.input);
    let mut chats = std::mem::take(&mut state.chats);
    let Some(lines) = chats.take_over_shown() else {
        state.chats = chats;
        state.input = input;
        return false;
    };
    *state = State {
        open: state.open,
        input,
        picks_running: state.picks_running,
        picks_stop: Arc::clone(&state.picks_stop),
        picks_settled_at: state.picks_settled_at,
        chats,
        ..State::default()
    };
    state.thread = lines
        .into_iter()
        .map(|line| match line.speaker {
            Speaker::User => Entry::User(line.text),
            Speaker::Hermes => Entry::Hermes {
                parts: vec![Part::Text(line.text)],
                chips: Vec::new(),
            },
        })
        .collect();
    true
}

/// In these steps every typed text is for the flow, so a text that looks like
/// a path is no folder the user picked.
fn words_are_taken_as_text(step: Step) -> bool {
    matches!(step, Step::RuleLevels | Step::RuleWhat)
}

/// Work or the model is running, or something waits for Hermes to start;
/// typing and buttons wait for it.
pub fn is_busy(state: &State) -> bool {
    state.waits_for_hermes()
        || state.pending.is_some()
        || state.thinking.is_some()
        || state.rule_ask.is_some()
        || state.resolving.is_some()
        || state.naming.is_some()
}

pub fn text_with(ctx: &Ctx, msg: Msg, values: &[(&str, &str)]) -> String {
    fill(ctx.lang, msg, values)
}

fn launch(state: &mut State, ctx: &Ctx) -> Task<Message> {
    Task::batch([
        launch_work(state, ctx),
        understand::launch(state, ctx),
        crate::rule_flow::launch(state, ctx),
        launch_resolve(state, ctx),
        named_folders::launch(state, ctx),
        crate::picks::launch(state),
    ])
}

fn launch_work(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh) {
        return Task::none();
    }
    let Some(work) = state.pending.clone() else {
        return Task::none();
    };
    // Hermes went away between the question and the start: end the wait.
    let Availability::Ready(ready) = &ctx.availability else {
        finish_work(state);
        say(
            state,
            text(ctx.lang, Msg::HermesChatNotReadyYet),
            fallback_chips(ctx),
        );
        return Task::none();
    };
    let access = ready.clone().with_model(ctx.model.clone());
    Task::perform(
        async move { tokio::task::spawn_blocking(move || access.run(work)).await },
        |joined| match joined {
            Ok(result) => Message::Done(Box::new(result)),
            Err(error) => Message::Failed(error.to_string()),
        },
    )
}

fn launch_resolve(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh_resolve) {
        return Task::none();
    }
    let Some((typed, step)) = state.resolving.clone() else {
        return Task::none();
    };
    let resolve = ctx.resolve_folder;
    let asked = typed.clone();
    Task::perform(
        async move { tokio::task::spawn_blocking(move || resolve(&asked)).await },
        move |joined| Message::TypedFolderResolved {
            typed: typed.clone(),
            step,
            result: joined.unwrap_or_else(|error| Err(error.to_string())),
        },
    )
}

/// Whatever the user may set in the filing setup, they may type here: the
/// path is checked the same way, and used only after a yes.
fn typed_folder_resolved(
    state: &mut State,
    ctx: &Ctx,
    typed: &Path,
    step: Step,
    result: Result<PathBuf, String>,
) -> Option<Action> {
    let asked = state.resolving.take()?;
    let sentence = state.unresolved_sentence.take();
    if asked.1 != step || state.step != step {
        return None;
    }
    if !expects_folder(step) {
        return match (result, sentence) {
            (Ok(real), _) => {
                state.step = Step::Greeting;
                folder_answer(state, ctx, real, true)
            }
            (Err(_), Some((unresolved, unresolved_asked))) => {
                understand::ask(state, ctx, &unresolved, unresolved_asked)
            }
            (Err(_), None) => None,
        };
    }
    match result {
        Err(_) => {
            state.session.retry = match step {
                Step::Source | Step::TidyFolder => Some(ReadFor::Source),
                Step::ReadFolder => Some(ReadFor::Learn),
                _ => None,
            }
            .map(|purpose| Retry {
                folder: typed.to_path_buf(),
                purpose,
                unresolved_in: Some(step),
            });
            say(
                state,
                text_with(
                    ctx,
                    Msg::HermesChatNotAFolder,
                    &[("name", &folder_name(ctx, typed))],
                ),
                lost_folder_retry_chips(state, ctx),
            );
        }
        Ok(real) => {
            let mention = mention_of(ctx, &real);
            state.session.confirm = Some((real, step));
            state.step = Step::ConfirmUnknown;
            hermes(
                state,
                vec![
                    Part::Text(text_with(
                        ctx,
                        Msg::HermesChatConfirmTypedFolder,
                        &[("name", &mention.name)],
                    )),
                    Part::Folder(mention),
                ],
                vec![
                    chip(ctx, Msg::HermesChatConfirmYes, ChipReply::ReadAnyway, true),
                    cancel_chip(ctx),
                ],
            );
        }
    }
    None
}

fn finish_work(state: &mut State) {
    state.pending = None;
    drop_working(state);
}

pub fn drop_working(state: &mut State) {
    state
        .thread
        .retain(|entry| !matches!(entry, Entry::Working(_)));
}

fn handle_done(state: &mut State, ctx: &Ctx, result: ChatResult) -> Option<Action> {
    match result {
        ChatResult::Read(read) => learn::read_done(state, ctx, read),
        ChatResult::Trial(trial) => tidy::trial_done(state, ctx, trial.map(|plan| *plan)),
        // The chat no longer asks for group rules; the work stays in ncrs-filing.
        ChatResult::RuleSaved(_) => {}
        ChatResult::RuleTried(tried) => crate::rule_flow::trial_done(state, ctx, tried),
        ChatResult::RuleStored(stored) => crate::rule_flow::stored(state, ctx, stored),
        ChatResult::Filed(filed) => tidy::filed_done(state, ctx, filed),
        ChatResult::Trashed(trashed) => tidy::trashed_done(state, ctx, trashed),
        ChatResult::LastRun(last) => undo::last_run_done(state, ctx, last),
        ChatResult::Undone(undone) => undo::undone_done(state, ctx, undone),
        ChatResult::WaitingCount(count) => crate::waiting::counted(state, ctx, count),
        ChatResult::WaitingLoaded(loaded) => crate::waiting::loaded(state, ctx, loaded),
        ChatResult::WaitingFiled(filed) => crate::waiting::filed(state, ctx, filed),
    }
    None
}

// Building the thread.

pub fn access(ctx: &Ctx) -> Option<&ChatAccess> {
    match &ctx.availability {
        Availability::Ready(access) => Some(access),
        _ => None,
    }
}

/// What the user said or pressed that cannot run before Hermes is there.
enum Held {
    Sentence(Unsent),
    Reply(ChipReply),
}

/// True when Hermes is not ready and has answered for it: while it starts, the
/// input waits in the chat; when it is off, the way to switch it on is offered.
fn hold_until_ready(state: &mut State, ctx: &Ctx, held: Held) -> bool {
    match &ctx.availability {
        Availability::Ready(_) => false,
        Availability::Starting => {
            state.waiting_for_hermes = true;
            match held {
                Held::Sentence(unsent) => state.session.unsent = Some(unsent),
                Held::Reply(reply) => state.session.unsent_reply = Some(reply),
            }
            state
                .thread
                .push(Entry::Working(text(ctx.lang, Msg::HermesChatStartingWait)));
            true
        }
        Availability::Failed(reason) => {
            say_failed(state, ctx, reason);
            true
        }
        Availability::ModelFailed(problem) => {
            say_model_failed(state, ctx, *problem);
            true
        }
        Availability::Off => {
            say_off(state, ctx);
            true
        }
    }
}

/// Hermes tried to start and could not: the reason, and a way to try again.
pub fn say_failed(state: &mut State, ctx: &Ctx, reason: &str) {
    say(
        state,
        text_with(ctx, Msg::HermesChatStartFailed, &[("reason", reason)]),
        vec![chip(ctx, Msg::HermesChatRetry, ChipReply::TurnOn, true)],
    );
}

pub fn say_model_failed(state: &mut State, ctx: &Ctx, problem: ncrs_ai::ModelProblem) {
    say(
        state,
        crate::texts::problem_text(ctx.lang, problem),
        vec![chip(ctx, Msg::HermesChatRetry, ChipReply::TurnOn, true)],
    );
}

pub fn say_off(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatOff),
        vec![chip(ctx, Msg::HermesChatTurnOn, ChipReply::TurnOn, true)],
    );
}

/// yet. The greeting follows once the AI and the model are there.
/// Without a ready Hermes or while busy it is a plain open: the documents are
/// still in the reviews.
fn open_about(
    state: &mut State,
    items: Vec<ncrs_filing::review_work::ReviewItem>,
    ctx: &Ctx,
) -> Option<Action> {
    if items.is_empty() || is_busy(state) || !matches!(ctx.availability, Availability::Ready(_)) {
        return route(state, Message::Open, ctx);
    }
    state.open = true;
    crate::waiting::ask_about(state, ctx, items);
    None
}

pub fn open_thread(state: &mut State, ctx: &Ctx) {
    match &ctx.availability {
        Availability::Off => {
            say_off(state, ctx);
            return;
        }
        Availability::Failed(reason) => {
            say_failed(state, ctx, reason);
            return;
        }
        Availability::ModelFailed(problem) => {
            say_model_failed(state, ctx, *problem);
            return;
        }
        Availability::Starting => {
            state.waiting_for_hermes = true;
            greet(state, ctx);
            return;
        }
        Availability::Ready(_) => {}
    }
    if understand::gate(state, ctx, None) == understand::Gate::Said {
        state.greeting_owed = true;
        return;
    }
    greet(state, ctx);
}

fn greet(state: &mut State, ctx: &Ctx) {
    state.greeting_owed = false;
    state.step = Step::Greeting;
    if crate::waiting::has_review(ctx) {
        crate::waiting::count_before_greeting(state);
        return;
    }
    say(
        state,
        text(ctx.lang, Msg::HermesChatGreeting),
        crate::greeting::chips(state, ctx),
    );
}

fn expects_folder(step: Step) -> bool {
    matches!(
        step,
        Step::ReadFolder
            | Step::Greeting
            | Step::Source
            | Step::TidyFolder
            | Step::Target
            | Step::OtherTarget
            | Step::NewTarget
            | Step::WaitingFolder
    )
}

pub fn expand_home(ctx: &Ctx, path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => ctx.home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

pub fn folder_answer(state: &mut State, ctx: &Ctx, path: PathBuf, typed: bool) -> Option<Action> {
    if typed && !is_known(state, ctx, &path) {
        let mention = mention_of(ctx, &path);
        state.session.confirm = Some((path, state.step));
        state.step = Step::ConfirmUnknown;
        hermes(
            state,
            vec![
                Part::Text(text_with(
                    ctx,
                    Msg::HermesChatUnknownPathConfirm,
                    &[("name", &mention.name)],
                )),
                Part::Folder(mention),
            ],
            vec![
                chip(ctx, Msg::HermesChatReadAnyway, ChipReply::ReadAnyway, true),
                cancel_chip(ctx),
            ],
        );
        return None;
    }
    match state.step {
        Step::ReadFolder => {
            if !crate::rule_flow::open(state, ctx, path.clone()) {
                learn::read(state, ctx, path);
            }
            None
        }
        Step::Greeting => {
            crate::greeting::route(state, ctx, path);
            None
        }
        Step::Source => {
            let role = (!ctx.sources.contains(&path)).then(|| Action::SetFolderRole {
                folder: path.clone(),
                role: FolderRole::Inbox,
            });
            tidy::begin(state, ctx, path);
            role
        }
        Step::TidyFolder => {
            tidy::begin(state, ctx, path);
            None
        }
        Step::Target => {
            tidy::choose_target(state, ctx, path);
            None
        }
        Step::OtherTarget => {
            understand::other_target(state, ctx, path);
            None
        }
        Step::NewTarget => {
            crate::decide::new_target_picked(state, ctx, path);
            None
        }
        Step::WaitingFolder => {
            crate::waiting::file_into(state, ctx, path);
            None
        }
        _ => None,
    }
}

// What the user said or pressed.

/// A path written out: from the root, from home, or absolute in the
/// platform's own form (`C:\…` and `\\server\share` on Windows).
pub fn looks_like_typed_path(text: &str) -> bool {
    text.starts_with('/') || text.starts_with('~') || Path::new(text).is_absolute()
}

/// A typed path is taken literally while a folder is asked for, and anywhere
/// when it is an existing folder; every other sentence goes to the model.
fn handle_text(state: &mut State, ctx: &Ctx, typed: &str, asked: Asked) -> Option<Action> {
    if matches!(state.step, Step::RuleLevels | Step::RuleWhat) {
        crate::rule_flow::typed(state, ctx, typed);
        return None;
    }
    if expects_folder(state.step) && looks_like_typed_path(typed) {
        state.resolving = Some((expand_home(ctx, Path::new(typed)), state.step));
        state.fresh_resolve = true;
        return None;
    }
    if looks_like_typed_path(typed) {
        state.resolving = Some((expand_home(ctx, Path::new(typed)), state.step));
        state.unresolved_sentence = Some((typed.to_owned(), asked));
        state.fresh_resolve = true;
        return None;
    }
    if let Some(reply) = chip_reply_for(&asked, typed) {
        return handle_reply(state, ctx, reply);
    }
    understand::ask(state, ctx, typed, asked)
}

/// A typed answer that is exactly the label of one of the buttons is that
/// button, without asking the model.
fn chip_reply_for(asked: &Asked, typed: &str) -> Option<ChipReply> {
    let wanted = typed.trim().to_lowercase();
    let mut matching = asked
        .chips
        .iter()
        .filter(|chip| chip.label.trim().to_lowercase() == wanted);
    let first = matching.next()?;
    matching.next().is_none().then(|| first.reply.clone())
}

fn ask_tidy_folder(state: &mut State, ctx: &Ctx, folder: Option<PathBuf>) -> Option<Action> {
    state.step = Step::TidyFolder;
    let Some(path) = folder else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatAskTidyFolder),
            ask_chips(ctx),
        );
        return None;
    };
    folder_answer(state, ctx, path, true)
}

pub fn ask_read_folder(state: &mut State, ctx: &Ctx, folder: Option<PathBuf>) -> Option<Action> {
    state.step = Step::ReadFolder;
    state.session.read_for = Some(crate::state::ReadFor::Learn);
    let Some(path) = folder else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatAskReadFolder),
            target_chips(state, ctx),
        );
        return None;
    };
    folder_answer(state, ctx, path, true)
}

pub fn handle_reply(state: &mut State, ctx: &Ctx, reply: ChipReply) -> Option<Action> {
    match reply {
        ChipReply::Cancel => cancel(state, ctx),
        ChipReply::Folder(path) => return folder_answer(state, ctx, path, false),
        ChipReply::NamedFolder(folder) => named_folders::chosen(state, ctx, folder),
        ChipReply::PickFolder => return Some(Action::PickFolder),
        ChipReply::TurnOn => {
            state.waiting_for_hermes = true;
            state.greeting_owed = true;
            state
                .thread
                .push(Entry::Working(text(ctx.lang, Msg::HermesChatStartingWait)));
            return Some(Action::TurnOnHermes);
        }
        ChipReply::ReadAnyway => {
            if let Some((path, step)) = state.session.confirm.take() {
                state.step = step;
                return folder_answer(state, ctx, path, false);
            }
        }
        ChipReply::KeepYes | ChipReply::FineAsIs => return learn::keep(state, ctx),
        ChipReply::ShowAll => learn::show_all(state, ctx),
        ChipReply::ToTrash => tidy::trash_offer(state, ctx, true),
        ChipReply::LeaveThem => tidy::trash_offer(state, ctx, false),
        ChipReply::Later => {
            reset_flow(state);
            say(
                state,
                text(ctx.lang, Msg::HermesChatLaterReply),
                fallback_chips(ctx),
            );
        }
        ChipReply::Retry => learn::retry(state, ctx),
        ChipReply::TrashDuplicatesNow => trial_end::trash_duplicates(state, ctx),
        ChipReply::TrialOtherTarget => return trial_end::other_target(state),
        ChipReply::Once => tidy::once(state, ctx, false),
        ChipReply::Always => tidy::once(state, ctx, true),
        ChipReply::DownloadModel => return Some(Action::DownloadModel),
        ChipReply::TurnOnAi => return Some(Action::TurnOnAi),
        ChipReply::ConfirmTidy => return understand::confirm_tidy_yes(state, ctx),
        ChipReply::OtherTarget => understand::ask_other_target(state, ctx),
        ChipReply::SwitchToCopy => {
            understand::switch_transfer(state, ctx, ncrs_index::Transfer::Copy)
        }
        ChipReply::SwitchToMove => {
            understand::switch_transfer(state, ctx, ncrs_index::Transfer::Move)
        }
        ChipReply::ModeCopy => tidy::mode(state, ctx, ncrs_index::Transfer::Copy),
        ChipReply::ModeMove => tidy::mode(state, ctx, ncrs_index::Transfer::Move),
        ChipReply::PhotosYes => return photos::yes(state, ctx),
        ChipReply::PhotosNo => photos::no(state, ctx),
        ChipReply::RoleSource => return crate::greeting::role_chosen(state, ctx, true),
        ChipReply::RoleTarget => return crate::greeting::role_chosen(state, ctx, false),
        ChipReply::NewTarget => return crate::decide::pick_new_target(state),
        ChipReply::DecidePerFile => crate::decide::per_file(state, ctx),
        ChipReply::DecideTarget(path) => crate::decide::target_chosen(state, ctx, path),
        ChipReply::DecideSkip => crate::decide::skipped(state, ctx),
        ChipReply::NewLayout(layout) => crate::decide::layout_chosen(state, ctx, layout),
        ChipReply::WaitingShow => crate::waiting::show(state),
        ChipReply::WaitingFile => crate::waiting::file_current(state, ctx),
        ChipReply::WaitingFileInto(folder) => crate::waiting::file_into(state, ctx, folder),
        ChipReply::WaitingOtherFolder => return crate::waiting::other_folder(state),
        ChipReply::WaitingSkip => crate::waiting::skip(state, ctx),
        ChipReply::LevelsKeep => crate::rule_flow::levels_kept(state, ctx),
        ChipReply::LevelsChange => crate::rule_flow::levels_change(state, ctx),
        ChipReply::LevelsDefault => crate::rule_flow::levels_default(state, ctx),
        ChipReply::LevelsApply | ChipReply::LevelsDone => crate::rule_flow::levels_kept(state, ctx),
        ChipReply::LevelsMore => crate::rule_flow::levels_more(state, ctx),
        ChipReply::LevelsDiscard => crate::rule_flow::levels_discard(state, ctx),
        ChipReply::RuleWhat(description) => crate::rule_flow::described(state, ctx, &description),
        ChipReply::RuleCreate => crate::rule_flow::create(state),
        ChipReply::RuleCondition => crate::rule_flow::edit(state, ctx),
        ChipReply::OnlyFrom(source) => crate::rule_flow::limit_to_source(state, ctx, Some(source)),
        ChipReply::FromAllSources => crate::rule_flow::limit_to_source(state, ctx, None),
        ChipReply::RuleNow => return crate::rule_flow::file_now(state, ctx),
        ChipReply::RuleOnlyNew => crate::rule_flow::only_new(state, ctx),
        ChipReply::PutBack => undo::put_back(state, ctx),
        ChipReply::Undo | ChipReply::IntentUndo => undo::ask(state, ctx),
        ChipReply::OpenSearch | ChipReply::IntentSearch => return Some(Action::OpenSearch),
        ChipReply::IntentTidy => {
            return ask_tidy_folder(state, ctx, None);
        }
        ChipReply::IntentStructure => {
            return ask_read_folder(state, ctx, None);
        }
    }
    None
}

pub fn cancel(state: &mut State, ctx: &Ctx) {
    reset_flow(state);
    say(
        state,
        text(ctx.lang, Msg::HermesChatCancelled),
        fallback_chips(ctx),
    );
}

/// Forgets what the running conversation was about; nothing is remembered.
fn reset_flow(state: &mut State) {
    state.session.waiting = None;
    state.session.tidy = None;
    state.session.rule_queue = None;
    state.session.retry = None;
    state.session.trial = None;
    state.session.offer = None;
    state.session.resume_tidy = false;
    state.session.role_asked = None;
    state.session.photos_of = None;
    state.session.layout = None;
    state.session.deciding = None;
    state.session.confirm = None;
    state.session.choosing = None;
    state.session.proposal = None;
    state.session.rule = None;
    crate::follow_up::forget(state);
    state.step = Step::Idle;
}

#[cfg(test)]
mod typed_chip_tests;
