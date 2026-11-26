//! The login questions of a push: git asks through the broker, the app hands
//! the question to this tab by its job number, and the dialog answers it.
//! While a question is open the keys of the list do nothing.

use iced::Task;
use ncrs_git::askpass::{self, AskRequest, PromptKind, Secret};

use crate::message::Message;
use crate::state::State;
use crate::update::Outcome;

/// A question git asked and the answer typed so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub id: u64,
    pub prompt: String,
    pub kind: PromptKind,
    pub typed: Secret,
}

impl Credential {
    fn of(request: &AskRequest) -> Self {
        Self {
            id: request.id,
            prompt: request.prompt.clone(),
            kind: request.kind,
            typed: Secret::new(""),
        }
    }

    fn answer(&self) -> Secret {
        match self.kind {
            PromptKind::Confirm => Secret::new("yes"),
            PromptKind::Text | PromptKind::Secret => self.typed.clone(),
        }
    }
}

fn quiet() -> Outcome {
    (Task::none(), Vec::new())
}

/// Keys that act on the list, the menu or the bars behind the dialog.
const fn is_list_key(message: &Message) -> bool {
    matches!(
        message,
        Message::Up
            | Message::Down
            | Message::Left
            | Message::Right
            | Message::SwitchFocus
            | Message::ExtendUp
            | Message::ExtendDown
            | Message::PageUp
            | Message::PageDown
            | Message::First
            | Message::Last
            | Message::Mark
            | Message::MarkAll
            | Message::Enter
            | Message::ToggleFavouriteAtCursor
            | Message::RemoveAtCursor
            | Message::FocusFilter
            | Message::OpenMenuKey
            | Message::ShowInFilesAtCursor
            | Message::OpenTerminalAtCursor
            | Message::CopyPathAtCursor
            | Message::Bulk(_)
    )
}

fn show_next(state: &mut State) {
    if !state.waiting_credentials.is_empty() {
        let next = state.waiting_credentials.remove(0);
        state.credential = Some(Credential::of(&next));
    }
}

/// Esc, "Cancel" or a "no": git is told nobody answered.
fn refuse(state: &mut State) {
    if let Some(open) = state.credential.take() {
        askpass::answer(open.id, None);
        show_next(state);
    }
}

fn send(state: &mut State, id: u64) {
    let Some(open) = state.credential.as_ref().filter(|open| open.id == id) else {
        return;
    };
    if open.kind != PromptKind::Confirm && open.typed.expose().is_empty() {
        return;
    }
    askpass::answer(open.id, Some(open.answer()));
    state.credential = None;
    show_next(state);
}

/// Handles the messages of the dialog and swallows the keys of the list while
/// it is open; any other message comes back.
pub fn update(state: &mut State, message: Message) -> Result<Outcome, Message> {
    Ok(match message {
        Message::CredentialAsked(request) => {
            if state.credential.is_some() {
                state.waiting_credentials.push(request);
            } else {
                state.credential = Some(Credential::of(&request));
            }
            state.filter_focused = false;
            quiet()
        }
        Message::CredentialTyped(id, typed) => {
            if let Some(open) = state.credential.as_mut().filter(|open| open.id == id) {
                open.typed = typed;
            }
            quiet()
        }
        Message::CredentialSend(id) => {
            send(state, id);
            quiet()
        }
        Message::CredentialCancel(id) => {
            if state.credential.as_ref().is_some_and(|open| open.id == id) {
                refuse(state);
            }
            quiet()
        }
        Message::Escape if state.credential.is_some() => {
            refuse(state);
            quiet()
        }
        // A yes or no question (an unknown host key) is refused by Enter:
        // it is the safe answer, "Connect" is the button's.
        Message::Enter
            if state
                .credential
                .as_ref()
                .is_some_and(|open| open.kind == PromptKind::Confirm) =>
        {
            refuse(state);
            quiet()
        }
        other if state.credential.is_some() && is_list_key(&other) => quiet(),
        other => return Err(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn request(id: u64, kind: PromptKind) -> AskRequest {
        AskRequest {
            id,
            job: 1,
            prompt: "Password: ".to_owned(),
            kind,
        }
    }

    fn state() -> State {
        State::new(Store::default(), None)
    }

    fn ask(state: &mut State, request: AskRequest) {
        drop(update(state, Message::CredentialAsked(request)).unwrap());
    }

    #[test]
    fn a_second_question_waits_until_the_first_is_answered() {
        let mut state = state();
        ask(&mut state, request(10, PromptKind::Text));
        ask(&mut state, request(11, PromptKind::Secret));
        assert_eq!(state.credential.as_ref().map(|open| open.id), Some(10));
        drop(update(&mut state, Message::CredentialTyped(10, Secret::new("me"))).unwrap());
        drop(update(&mut state, Message::CredentialSend(10)).unwrap());
        assert_eq!(state.credential.as_ref().map(|open| open.id), Some(11));
    }

    #[test]
    fn an_empty_answer_to_a_text_question_stays_unsent() {
        let mut state = state();
        ask(&mut state, request(10, PromptKind::Secret));
        drop(update(&mut state, Message::CredentialSend(10)).unwrap());
        assert!(state.credential.is_some());
    }

    #[test]
    fn the_keys_of_the_list_do_nothing_behind_the_dialog() {
        let mut state = state();
        update(&mut state, Message::Enter).unwrap_err();
        ask(&mut state, request(10, PromptKind::Secret));
        drop(update(&mut state, Message::Enter).unwrap());
        drop(update(&mut state, Message::Down).unwrap());
        assert!(state.credential.is_some());
        drop(update(&mut state, Message::Escape).unwrap());
        assert!(state.credential.is_none());
    }

    #[test]
    fn enter_refuses_a_yes_or_no_question() {
        let mut state = state();
        ask(&mut state, request(10, PromptKind::Confirm));
        drop(update(&mut state, Message::Enter).unwrap());
        assert!(state.credential.is_none());
    }
}
