//! The trial and the purchase of the App Store build: what the app asks the
//! store, when it holds a change back, and what it says then.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::*;
use crate::messages::StoreMessage;
use ncrs_settings::StoreButton;
use ncrs_store::{Entitlement, PurchaseOutcome, Store, FULL_PRODUCT_ID, TRIAL_PRODUCT_ID};

/// How often the app looks whether the store reported something.
const POLL_EVERY: Duration = Duration::from_secs(60);
/// The longest the app goes without asking the store.
const REFETCH_AFTER: Duration = Duration::from_secs(60 * 60);

pub(in crate::app) struct StoreState {
    /// `None` where there is no data folder to keep the answer in.
    store: Option<Arc<Store>>,
    entitlement: Entitlement,
    seen_changes: u64,
    fetched_at: Instant,
}

impl StoreState {
    /// Never locked, and nothing asked.
    pub(in crate::app) fn unrestricted() -> Self {
        Self {
            store: None,
            entitlement: Entitlement::Unrestricted,
            seen_changes: 0,
            fetched_at: Instant::now(),
        }
    }

    /// The store of this build, with its last answer kept in `folder`.
    pub(in crate::app) fn in_folder(folder: Option<&Path>) -> Self {
        let Some(kept_in) = folder else {
            return Self::unrestricted();
        };
        let store = Store::new(kept_in.to_path_buf());
        Self {
            entitlement: store.cached_entitlement(),
            store: Some(Arc::new(store)),
            ..Self::unrestricted()
        }
    }

    /// Only the tests of the app put the state in by hand.
    #[cfg(test)]
    pub(in crate::app) fn set_entitlement(&mut self, entitlement: Entitlement) {
        self.entitlement = entitlement;
    }

    fn is_watched(&self) -> bool {
        self.entitlement != Entitlement::Unrestricted
            || self.store.as_deref().is_some_and(Store::is_active)
    }
}

/// What changes files, and so waits for the full version once the trial is
/// not running. Viewing, browsing, searching and previewing are not in it.
fn changes_files(message: &Message) -> bool {
    match message {
        Message::Transfer(_)
        | Message::Delete { .. }
        | Message::Paste
        | Message::CreateDirPrompt
        | Message::CreateFilePrompt
        | Message::PackPrompt
        | Message::ExtractHere
        | Message::PermissionsPrompt
        | Message::NoteOnFile
        | Message::Panel(PanelMessage::RenameStart)
        | Message::Ops(OpsMessage::FileDropped(_))
        | Message::FilingView(ncrs_filing_view::Message::File(_))
        | Message::Reviews(
            ncrs_hermes_ui::ReviewsMessage::Answer(..)
            | ncrs_hermes_ui::ReviewsMessage::AnswerFile(..)
            | ncrs_hermes_ui::ReviewsMessage::AnswerRepeat(..),
        )
        | Message::Notes(
            ncrs_notes::Message::NewNote
            | ncrs_notes::Message::NewFolder
            | ncrs_notes::Message::EditNoteOn(_)
            | ncrs_notes::Message::Seal(..),
        ) => true,
        Message::Notes(ncrs_notes::Message::Edited(action)) => action.is_edit(),
        _ => false,
    }
}

impl App {
    /// The named check every entry that changes files makes first.
    pub(in crate::app) fn changes_allowed(&self) -> bool {
        self.store.entitlement.allows_changes()
    }

    /// Holds `message` back when it would change files and the trial does not
    /// allow it.
    pub(in crate::app) fn refuse_if_locked(&mut self, message: &Message) -> Option<Task<Message>> {
        (!self.changes_allowed() && changes_files(message)).then(|| self.refuse_locked_change())
    }

    /// The notice of an action that did not run, pressing it opens the page.
    pub(in crate::app) fn refuse_locked_change(&mut self) -> Task<Message> {
        let reason = self.lang.text(match self.store.entitlement {
            Entitlement::Expired => Msg::StoreStatusExpired,
            _ => Msg::StoreLockedNotStarted,
        });
        let text = fill_template(
            self.lang.text(Msg::StoreLockedPress),
            &[("{reason}", reason)],
        );
        self.set_status_error_with_press(&text, Message::Store(StoreMessage::OpenPage));
        Task::none()
    }

    /// Starts the listener of the store and asks for the first answer.
    pub(in crate::app) fn start_store(&mut self) -> Task<Message> {
        if let Some(store) = &self.store.store {
            store.start();
        }
        self.fetch_entitlement()
    }

    fn fetch_entitlement(&mut self) -> Task<Message> {
        let Some(store) = self.store.store.clone() else {
            return Task::none();
        };
        self.store.fetched_at = Instant::now();
        self.store.seen_changes = store.changes();
        Task::perform(
            async move { tokio::task::spawn_blocking(move || store.entitlement()).await },
            |joined| joined.ok(),
        )
        .and_then(|entitlement| Task::done(Message::Store(StoreMessage::Fetched(entitlement))))
    }

    pub(in crate::app) fn update_store(&mut self, message: StoreMessage) -> Task<Message> {
        match message {
            StoreMessage::Tick => {
                let due = self.store.fetched_at.elapsed() >= REFETCH_AFTER
                    || self
                        .store
                        .store
                        .as_deref()
                        .is_some_and(|store| store.changes() != self.store.seen_changes);
                if due {
                    self.fetch_entitlement()
                } else {
                    Task::none()
                }
            }
            StoreMessage::Fetched(entitlement) => {
                self.store.entitlement = entitlement;
                Task::none()
            }
            StoreMessage::Restored(entitlement) => self.purchase_restored(entitlement),
            StoreMessage::OpenPage => {
                self.show_setting(crate::messages::SettingsFeature::FullVersion, None)
            }
            StoreMessage::PurchaseDone(outcome) => self.purchase_done(outcome),
        }
    }

    /// A button of the Full version page.
    pub(in crate::app) fn press_store_button(&mut self, button: StoreButton) -> Task<Message> {
        let Some(store) = self.store.store.clone() else {
            return Task::none();
        };
        match button {
            StoreButton::StartTrial | StoreButton::Buy => {
                let product = match button {
                    StoreButton::StartTrial => TRIAL_PRODUCT_ID,
                    _ => FULL_PRODUCT_ID,
                };
                Task::perform(
                    async move { tokio::task::spawn_blocking(move || store.purchase(product)).await },
                    |joined| {
                        let outcome = joined
                            .unwrap_or_else(|error| PurchaseOutcome::Failed(error.to_string()));
                        Message::Store(StoreMessage::PurchaseDone(outcome))
                    },
                )
            }
            StoreButton::Restore => Task::perform(
                async move { tokio::task::spawn_blocking(move || store.restore()).await },
                |joined| joined.ok(),
            )
            .and_then(|entitlement| {
                Task::done(Message::Store(StoreMessage::Restored(entitlement)))
            }),
        }
    }

    fn purchase_done(&mut self, outcome: PurchaseOutcome) -> Task<Message> {
        let notice = match outcome {
            PurchaseOutcome::Purchased => None,
            PurchaseOutcome::Cancelled => return Task::none(),
            PurchaseOutcome::Pending => Some(self.lang.text(Msg::StorePurchasePending).to_owned()),
            PurchaseOutcome::Failed(error) => Some(fill_template(
                self.lang.text(Msg::StorePurchaseFailed),
                &[("{error}", &error)],
            )),
        };
        let refetch = self.fetch_entitlement();
        match notice {
            Some(text) => Task::batch([self.show_notice(text), refetch]),
            None => refetch,
        }
    }

    /// The answer of "Restore purchase": says so when there was nothing.
    fn purchase_restored(&mut self, entitlement: Entitlement) -> Task<Message> {
        self.store.entitlement = entitlement;
        if matches!(entitlement, Entitlement::Full(_)) {
            return Task::none();
        }
        let text = self.lang.text(Msg::StoreRestoreNothing).to_owned();
        self.show_notice(text)
    }

    /// Asks the store again every minute, where there is anything to ask.
    pub(in crate::app) fn store_subscription(&self) -> Subscription<Message> {
        if self.store.is_watched() {
            iced::time::every(POLL_EVERY).map(|_| Message::Store(StoreMessage::Tick))
        } else {
            Subscription::none()
        }
    }

    /// The hint of the status bar in the last days of the trial and after it.
    pub(in crate::app) fn store_badge(&self) -> Option<statusbar::StoreBadge<Message>> {
        let entitlement = self.store.entitlement;
        if !entitlement.shows_reminder() {
            return None;
        }
        let text = match entitlement {
            Entitlement::Trial { days_left: 1 } => {
                self.lang.text(Msg::StoreBadgeTrialOne).to_owned()
            }
            Entitlement::Trial { days_left } => fill_template(
                self.lang.text(Msg::StoreBadgeTrial),
                &[("{days}", &days_left.to_string())],
            ),
            _ => self.lang.text(Msg::StoreBadgeExpired).to_owned(),
        };
        Some(statusbar::StoreBadge {
            text,
            on_press: Message::Store(StoreMessage::OpenPage),
            expired: entitlement == Entitlement::Expired,
        })
    }

    /// What the Full version page shows.
    pub(in crate::app) fn store_entitlement(&self) -> Entitlement {
        self.store.entitlement
    }
}
