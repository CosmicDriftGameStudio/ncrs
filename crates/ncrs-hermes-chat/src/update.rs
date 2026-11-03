//! The message loop and the routing of what the user typed or pressed.

use std::path::{Path, PathBuf};

use iced::Task;
use ncrs_filing::chat_work::{ChatAccess, ChatResult, ChatWork};
use ncrs_i18n::Msg;
use ncrs_ui_kit::hermes_state::FolderRole;

use crate::action::Action;
use crate::ctx::{Availability, Ctx};
use crate::message::Message;
use crate::state::{Asked, Chip, ChipReply, Entry, GroupFlow, Part, State, Step};
use crate::texts::{fill, folder_name, text};
use crate::{learn, named_folders, stay_groups, tidy, understand, undo};

pub fn update(state: &mut State, message: Message, ctx: &Ctx) -> (Task<Message>, Option<Action>) {
    let action = route(state, message, ctx);
    (launch(state, ctx), action)
}

fn route(state: &mut State, message: Message, ctx: &Ctx) -> Option<Action> {
    match message {
        Message::Open => {
            state.open = true;
            if state.thread.is_empty() {
                open_thread(state, ctx);
            }
            None
        }
        Message::Close => {
            state.open = false;
            Some(Action::Close)
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
            user(state, typed.clone());
            if !is_ready(state, ctx) {
                return None;
            }
            handle_text(state, ctx, &typed, asked)
        }
        Message::ChipPressed { label, reply } => {
            if is_busy(state) {
                return None;
            }
            let bubble = bubble_text(state, &label, &reply);
            user(state, bubble);
            if !is_ready(state, ctx) && reply != ChipReply::Setup {
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
            say(
                state,
                text_with(ctx, Msg::HermesChatReadFailed, &[("reason", &reason)]),
                Vec::new(),
            );
            None
        }
        Message::FolderPicked(picked) => {
            let path = picked?;
            if is_busy(state) {
                return None;
            }
            user(state, path.to_string_lossy().into_owned());
            folder_answer(state, ctx, path, false)
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
        Message::Understood(result) => understand::understood(state, ctx, result),
        Message::ModelReady => understand::model_ready(state, ctx),
        Message::AiTurnedOn => understand::ai_turned_on(state, ctx),
    }
}

/// Work or the model is running; typing and buttons wait for it.
fn is_busy(state: &State) -> bool {
    state.pending.is_some()
        || state.thinking.is_some()
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
        launch_resolve(state, ctx),
        named_folders::launch(state, ctx),
    ])
}

fn launch_work(state: &mut State, ctx: &Ctx) -> Task<Message> {
    if !std::mem::take(&mut state.fresh) {
        return Task::none();
    }
    let (Some(work), Availability::Ready(ready)) = (state.pending.clone(), &ctx.availability)
    else {
        return Task::none();
    };
    let access = ready.clone();
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
    if asked.1 != step || state.step != step {
        return None;
    }
    match result {
        Err(_) => say(
            state,
            text_with(
                ctx,
                Msg::HermesChatNotAFolder,
                &[("path", &typed.to_string_lossy())],
            ),
            Vec::new(),
        ),
        Ok(real) => {
            let shown = real.display().to_string();
            state.session.confirm = Some((real, step));
            state.step = Step::ConfirmUnknown;
            say(
                state,
                text_with(ctx, Msg::HermesChatConfirmTypedFolder, &[("path", &shown)]),
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
        ChatResult::Preview(preview) => learn::preview_done(state, ctx, preview),
        ChatResult::Trial(trial) => tidy::trial_done(state, ctx, trial),
        ChatResult::RuleSaved(saved) => stay_groups::rule_saved(state, ctx, saved),
        ChatResult::Filed(filed) => tidy::filed_done(state, ctx, filed),
        ChatResult::Trashed(trashed) => tidy::trashed_done(state, ctx, trashed),
        ChatResult::LastRun(last) => undo::last_run_done(state, ctx, last),
        ChatResult::Undone(undone) => undo::undone_done(state, ctx, undone),
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

fn is_ready(state: &mut State, ctx: &Ctx) -> bool {
    match &ctx.availability {
        Availability::Ready(_) => true,
        Availability::Starting => {
            say(state, text(ctx.lang, Msg::HermesChatStarting), Vec::new());
            false
        }
        Availability::NeedsSetup => {
            say(
                state,
                text(ctx.lang, Msg::HermesChatNeedsSetup),
                vec![chip(ctx, Msg::HermesChatSetup, ChipReply::Setup, true)],
            );
            false
        }
    }
}

pub fn chip(ctx: &Ctx, msg: Msg, reply: ChipReply, primary: bool) -> Chip {
    chip_labelled(text(ctx.lang, msg), reply, primary)
}

pub fn chip_labelled(label: String, reply: ChipReply, primary: bool) -> Chip {
    Chip {
        label,
        reply,
        primary,
    }
}

pub fn cancel_chip(ctx: &Ctx) -> Chip {
    chip(ctx, Msg::HermesChatCancel, ChipReply::Cancel, false)
}

/// A bubble of the user clears the chips Hermes offered before it.
pub fn user(state: &mut State, typed: String) {
    for entry in &mut state.thread {
        if let Entry::Hermes { chips, .. } = entry {
            chips.clear();
        }
    }
    state.thread.push(Entry::User(typed));
}

pub fn hermes(state: &mut State, parts: Vec<Part>, chips: Vec<Chip>) {
    state.thread.push(Entry::Hermes { parts, chips });
}

pub fn say(state: &mut State, sentence: String, chips: Vec<Chip>) {
    hermes(state, vec![Part::Text(sentence)], chips);
}

/// Sets work for the app to run. `working` is the line shown meanwhile;
/// `None` for work too quick to show one.
pub fn work(state: &mut State, work: ChatWork, working: Option<String>) {
    if let Some(line) = working {
        state.thread.push(Entry::Working(line));
    }
    state.pending = Some(work);
    state.fresh = true;
}

fn bubble_text(state: &State, label: &str, reply: &ChipReply) -> String {
    if let ChipReply::UnsureChoice { entry, .. } = reply {
        if let Some(name) = state
            .session
            .trial
            .as_ref()
            .and_then(|trial| trial.plan.entries.get(*entry))
            .map(|planned| planned.name.as_str())
        {
            return format!("{name}: {label}");
        }
    }
    label.to_owned()
}

// Folders.

pub fn well_known(ctx: &Ctx) -> [PathBuf; 4] {
    [
        ctx.home.clone(),
        ctx.home.join("Documents"),
        ctx.home.join("Downloads"),
        ctx.home.join("Desktop"),
    ]
}

fn is_known(state: &State, ctx: &Ctx, path: &Path) -> bool {
    ctx.targets
        .iter()
        .chain(&ctx.sources)
        .chain(&state.session.learned)
        .chain(&well_known(ctx))
        .any(|known| known == path)
}

/// Targets and sources first, then Documents and Downloads when they exist.
fn candidate_folders(ctx: &Ctx, with_targets: bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let own = if with_targets {
        ctx.targets.iter().chain(&ctx.sources)
    } else {
        [].iter().chain(&ctx.sources)
    };
    let usual = [ctx.home.join("Documents"), ctx.home.join("Downloads")]
        .into_iter()
        .filter(|path| path.is_dir());
    for path in own.cloned().chain(usual) {
        if !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

pub fn folder_chips(ctx: &Ctx, folders: Vec<PathBuf>) -> Vec<Chip> {
    folders
        .into_iter()
        .map(|path| chip_labelled(folder_name(ctx, &path), ChipReply::Folder(path), false))
        .collect()
}

/// The folders to offer when a folder is asked for, and the picker.
pub fn ask_chips(ctx: &Ctx) -> Vec<Chip> {
    let mut chips = folder_chips(ctx, candidate_folders(ctx, true));
    chips.push(chip(
        ctx,
        Msg::HermesChatOtherFolder,
        ChipReply::PickFolder,
        false,
    ));
    chips
}

pub fn source_chips(state: &State, ctx: &Ctx) -> Vec<Chip> {
    let _ = state;
    let mut chips = folder_chips(ctx, candidate_folders(ctx, false));
    chips.push(chip(ctx, Msg::HermesChatLater, ChipReply::Later, false));
    chips
}

/// The first bubble: the greeting, or why Hermes cannot understand sentences
/// yet. The greeting follows once the AI and the model are there.
pub fn open_thread(state: &mut State, ctx: &Ctx) {
    if !is_ready(state, ctx) {
        return;
    }
    if understand::gate(state, ctx, None) == understand::Gate::Said {
        state.greeting_owed = true;
        return;
    }
    greet(state, ctx);
}

fn greet(state: &mut State, ctx: &Ctx) {
    state.greeting_owed = false;
    state.step = Step::ReadFolder;
    state.session.read_for = Some(crate::state::ReadFor::Learn);
    say(
        state,
        text(ctx.lang, Msg::HermesChatGreeting),
        ask_chips(ctx),
    );
}

fn expects_folder(step: Step) -> bool {
    matches!(
        step,
        Step::ReadFolder | Step::Source | Step::TidyFolder | Step::Target | Step::OtherTarget
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
        let shown = path.to_string_lossy().into_owned();
        state.session.confirm = Some((path, state.step));
        state.step = Step::ConfirmUnknown;
        say(
            state,
            text_with(ctx, Msg::HermesChatUnknownPathConfirm, &[("path", &shown)]),
            vec![
                chip(ctx, Msg::HermesChatReadAnyway, ChipReply::ReadAnyway, true),
                cancel_chip(ctx),
            ],
        );
        return None;
    }
    match state.step {
        Step::ReadFolder => {
            learn::read(state, ctx, path);
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
        _ => None,
    }
}

// What the user said or pressed.

/// A path written out: from the root, from home, or absolute in the
/// platform's own form (`C:\…` and `\\server\share` on Windows).
pub fn looks_like_typed_path(text: &str) -> bool {
    text.starts_with('/') || text.starts_with('~') || Path::new(text).is_absolute()
}

/// Only a typed path while a folder is asked for is taken literally; every
/// other sentence goes to the model.
fn handle_text(state: &mut State, ctx: &Ctx, typed: &str, asked: Asked) -> Option<Action> {
    if state.step == Step::GroupFolderName {
        return stay_groups::folder_named(state, ctx, typed);
    }
    if expects_folder(state.step) && looks_like_typed_path(typed) {
        state.resolving = Some((expand_home(ctx, Path::new(typed)), state.step));
        state.fresh_resolve = true;
        return None;
    }
    understand::ask(state, ctx, typed, asked)
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

fn ask_read_folder(state: &mut State, ctx: &Ctx, folder: Option<PathBuf>) -> Option<Action> {
    state.step = Step::ReadFolder;
    state.session.read_for = Some(crate::state::ReadFor::Learn);
    let Some(path) = folder else {
        say(
            state,
            text(ctx.lang, Msg::HermesChatAskReadFolder),
            ask_chips(ctx),
        );
        return None;
    };
    folder_answer(state, ctx, path, true)
}

pub fn search_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatSearchNotYet),
        vec![chip(
            ctx,
            Msg::HermesChatOpenSearch,
            ChipReply::OpenSearch,
            true,
        )],
    );
}

pub fn explain_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatExplainNotYet),
        Vec::new(),
    );
}

pub fn batch_not_yet(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatBatchNotYet),
        Vec::new(),
    );
}

pub fn fallback(state: &mut State, ctx: &Ctx) {
    say(
        state,
        text(ctx.lang, Msg::HermesChatFallback),
        fallback_chips(ctx),
    );
}

pub fn fallback_chips(ctx: &Ctx) -> Vec<Chip> {
    vec![
        chip(
            ctx,
            Msg::HermesChatIntentSearch,
            ChipReply::IntentSearch,
            false,
        ),
        chip(ctx, Msg::HermesChatIntentTidy, ChipReply::IntentTidy, false),
        chip(
            ctx,
            Msg::HermesChatIntentStructure,
            ChipReply::IntentStructure,
            false,
        ),
        chip(ctx, Msg::HermesChatIntentUndo, ChipReply::IntentUndo, false),
    ]
}

pub fn handle_reply(state: &mut State, ctx: &Ctx, reply: ChipReply) -> Option<Action> {
    match reply {
        ChipReply::Cancel => cancel(state, ctx),
        ChipReply::Folder(path) => return folder_answer(state, ctx, path, false),
        ChipReply::NamedFolder(folder) => named_folders::chosen(state, ctx, folder),
        ChipReply::PickFolder => return Some(Action::PickFolder),
        ChipReply::Setup => return Some(Action::OpenFilingSetup),
        ChipReply::ReadAnyway => {
            if let Some((path, step)) = state.session.confirm.take() {
                state.step = step;
                return folder_answer(state, ctx, path, false);
            }
        }
        ChipReply::KeepYes | ChipReply::FineAsIs => return learn::keep(state, ctx),
        ChipReply::WantChange => learn::want_change(state, ctx),
        ChipReply::ShowAll => learn::show_all(state, ctx),
        ChipReply::ScopeAll => learn::scope(state, ctx, None),
        ChipReply::ScopeOnly(value) => learn::scope(state, ctx, Some(value)),
        ChipReply::ToTrash => tidy::trash_offer(state, ctx, true),
        ChipReply::LeaveThem => tidy::trash_offer(state, ctx, false),
        ChipReply::Later => {
            say(state, text(ctx.lang, Msg::HermesChatLaterReply), Vec::new());
            state.step = Step::Idle;
        }
        ChipReply::Once => tidy::once(state, ctx, false),
        ChipReply::Always => tidy::once(state, ctx, true),
        ChipReply::DownloadModel => return Some(Action::DownloadModel),
        ChipReply::TurnOnAi => return Some(Action::TurnOnAi),
        ChipReply::ConfirmTidy => return understand::confirm_tidy_yes(state, ctx),
        ChipReply::ConfirmChange => learn::confirm_change(state, ctx),
        ChipReply::OtherTarget => understand::ask_other_target(state, ctx),
        ChipReply::SwitchToCopy => {
            understand::switch_transfer(state, ctx, ncrs_index::Transfer::Copy)
        }
        ChipReply::SwitchToMove => {
            understand::switch_transfer(state, ctx, ncrs_index::Transfer::Move)
        }
        ChipReply::ModeCopy => tidy::mode(state, ctx, ncrs_index::Transfer::Copy),
        ChipReply::ModeMove => tidy::mode(state, ctx, ncrs_index::Transfer::Move),
        ChipReply::FileAll => tidy::file_all(state, ctx),
        ChipReply::ShowMore => tidy::show_more(state, ctx),
        ChipReply::GroupArea(area) => stay_groups::area_chosen(state, ctx, area),
        ChipReply::GroupNewFolder => stay_groups::new_folder(state, ctx),
        ChipReply::GroupLeave => stay_groups::leave_group(state, ctx),
        ChipReply::SinglesShow => stay_groups::singles_show(state, ctx),
        ChipReply::SinglesLeave => stay_groups::singles_leave(state, ctx),
        ChipReply::NamesNumbered => stay_groups::names_numbered(state, ctx),
        ChipReply::NamesLeave => stay_groups::names_leave(state, ctx),
        ChipReply::UnsureChoice { entry, choice } => tidy::unsure(state, ctx, entry, choice),
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
    state.session.tidy = None;
    state.session.trial = None;
    state.session.groups = GroupFlow::default();
    state.session.pending_change = None;
    state.session.offer = None;
    state.session.resume_tidy = false;
    state.session.confirm = None;
    state.session.choosing = None;
    state.session.proposal = None;
    state.session.change_scope = None;
    state.step = Step::Idle;
    say(state, text(ctx.lang, Msg::HermesChatCancelled), Vec::new());
}
