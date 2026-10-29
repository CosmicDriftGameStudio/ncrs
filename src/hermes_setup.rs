//! The first-time setup of Hermes: the user chooses the inbox, the filing
//! location and how the index is protected, and one press starts it all.
//!
//! The state only. Looking up a folder, writing the config and creating the
//! index touch the disk and run on blocking threads; the app hands their
//! results back.

use std::path::{Path, PathBuf};

use ncrs_index::Index;
use ncrs_intake::LocationMode;

use crate::config::FilingLocation;
use crate::filing::Phase;
use crate::filing_setup::{FilingSetup, SetupNote, SetupProblem};

/// The two folders the user chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderSlot {
    Inbox,
    FilingLocation,
}

/// What is known about an index file that exists already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingIndex {
    Encrypted,
    Plain,
    /// The file cannot be read to tell.
    Unknown,
}

impl ExistingIndex {
    /// `None` when there is no index file yet.
    pub fn of_file(index_path: &Path) -> Option<Self> {
        if !index_path.exists() {
            return None;
        }
        Some(match Index::file_is_plain(index_path) {
            Ok(true) => Self::Plain,
            Ok(false) => Self::Encrypted,
            Err(_) => Self::Unknown,
        })
    }
}

/// The elements the keys walk through, in the order of the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupFocus {
    ChooseInbox,
    ChooseFilingLocation,
    Encrypted,
    Plain,
    LoadModel,
    Later,
    Start,
}

/// What the app needs to know to offer the options of the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupOffer {
    pub existing_index: Option<ExistingIndex>,
    /// The AI model can be fetched and is not installed.
    pub model_download: bool,
    /// Hermes is switched off, so starting also switches it on.
    pub hermes_was_off: bool,
}

/// What pressing "Start Hermes" sets in motion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRequest {
    pub locations: Vec<FilingLocation>,
    pub plan: StartPlan,
}

/// What happens once the locations are saved, kept until it is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartPlan {
    encrypted: bool,
    load_model: bool,
    index_requested: bool,
}

/// What the plan wants next from the module it waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStep {
    Wait,
    CreateIndex { encrypted: bool },
    /// The index is open: fetch the model now if the user asked for it.
    Done { load_model: bool },
    /// Nothing more will happen on its own; the card says why.
    Abandon,
}

impl StartPlan {
    /// Looks at the module's phase and says what to do about it. Asks for the
    /// index once; a module that asks again has failed to create it.
    pub fn next_step(&mut self, phase: &Phase) -> PlanStep {
        match phase {
            Phase::AskEncryption { busy: true, .. } => PlanStep::Wait,
            Phase::AskEncryption { .. } if !self.index_requested => {
                self.index_requested = true;
                PlanStep::CreateIndex {
                    encrypted: self.encrypted,
                }
            }
            Phase::AskEncryption { .. } | Phase::Failed(_) | Phase::Dismissed => PlanStep::Abandon,
            Phase::Starting
            | Phase::ShowRecoveryKey { .. }
            | Phase::AskRecoveryKey { .. }
            | Phase::PlainFileWarning => PlanStep::Wait,
            Phase::Ready => PlanStep::Done {
                load_model: self.load_model,
            },
        }
    }
}

/// Whether the card has done its part once the module is in `phase`: what is
/// left to do is on the cards of the module.
pub fn card_is_done_at(phase: &Phase) -> bool {
    !matches!(phase, Phase::Starting | Phase::AskEncryption { .. })
}

#[derive(Debug)]
pub struct HermesSetup {
    /// The list in the config now. What the user does not change stays in it.
    configured: Vec<FilingLocation>,
    prefilled_inbox: Option<PathBuf>,
    inbox: Option<PathBuf>,
    filing_location: Option<PathBuf>,
    encrypted: bool,
    load_model: bool,
    offer: SetupOffer,
    picking: Option<FolderSlot>,
    writing: bool,
    note: Option<SetupNote>,
}

impl HermesSetup {
    /// Starts from what the config lists now.
    pub fn new(configured: &[FilingLocation], offer: SetupOffer) -> Self {
        let prefilled_inbox = configured
            .iter()
            .find(|location| location.mode == LocationMode::Inbox)
            .map(|location| location.path.clone());
        let known = FilingSetup::new(configured);
        let filing_location = known
            .target_index()
            .and_then(|index| configured.get(index))
            .map(|location| location.path.clone());
        Self {
            configured: configured.to_vec(),
            inbox: prefilled_inbox.clone(),
            prefilled_inbox,
            filing_location,
            encrypted: true,
            load_model: false,
            offer,
            picking: None,
            writing: false,
            note: None,
        }
    }

    pub fn encrypted(&self) -> bool {
        self.encrypted
    }

    pub fn load_model(&self) -> bool {
        self.load_model
    }

    pub fn offer(&self) -> SetupOffer {
        self.offer
    }

    pub fn folder(&self, slot: FolderSlot) -> Option<&Path> {
        match slot {
            FolderSlot::Inbox => self.inbox.as_deref(),
            FolderSlot::FilingLocation => self.filing_location.as_deref(),
        }
    }

    pub fn note(&self) -> Option<&SetupNote> {
        self.note.as_ref()
    }

    pub fn is_busy(&self) -> bool {
        self.picking.is_some() || self.writing
    }

    /// The elements that can be reached by key now, left to right and top to
    /// bottom. `can_start` is whether Start is available.
    pub fn focus_order(&self, can_start: bool) -> Vec<SetupFocus> {
        let mut order = vec![SetupFocus::ChooseInbox, SetupFocus::ChooseFilingLocation];
        if self.offer.existing_index.is_none() {
            order.extend([SetupFocus::Encrypted, SetupFocus::Plain]);
        }
        if self.offer.model_download {
            order.push(SetupFocus::LoadModel);
        }
        order.push(SetupFocus::Later);
        if can_start {
            order.push(SetupFocus::Start);
        }
        order
    }

    /// Where the focus is when the card opens: on Start if it can be pressed,
    /// otherwise on the first folder.
    pub fn first_focus(&self, can_start: bool) -> usize {
        let order = self.focus_order(can_start);
        if can_start {
            order.len().saturating_sub(1)
        } else {
            0
        }
    }

    /// The list that would be written: the chosen inbox first, the filing
    /// location last, everything else as the config has it.
    pub fn locations(&self) -> Vec<FilingLocation> {
        let replaced = |location: &FilingLocation| {
            self.prefilled_inbox.as_deref() == Some(location.path.as_path())
                || self.inbox.as_deref() == Some(location.path.as_path())
                || self.filing_location.as_deref() == Some(location.path.as_path())
        };
        let kept_images = |path: &Path| {
            self.configured
                .iter()
                .find(|location| location.path == path)
                .is_some_and(|location| location.images)
        };
        let mut list: Vec<FilingLocation> = Vec::new();
        if let Some(path) = &self.inbox {
            list.push(FilingLocation {
                path: path.clone(),
                mode: LocationMode::Inbox,
                images: kept_images(path),
                is_target: false,
            });
        }
        list.extend(
            self.configured
                .iter()
                .filter(|location| !replaced(location))
                .map(|location| FilingLocation {
                    is_target: false,
                    ..location.clone()
                }),
        );
        if let Some(path) = &self.filing_location {
            list.push(FilingLocation {
                path: path.clone(),
                mode: LocationMode::Archive,
                images: kept_images(path),
                is_target: true,
            });
        }
        list
    }

    /// Why the folders chosen cannot be used together. `None` while one is
    /// still missing: the card shows that by itself.
    pub fn problem(&self) -> Option<(SetupProblem, Vec<FilingLocation>)> {
        if self.inbox.is_none() || self.filing_location.is_none() {
            return None;
        }
        let list = self.locations();
        FilingSetup::new(&list)
            .problem()
            .map(|problem| (problem, list))
    }

    pub fn can_start(&self) -> bool {
        self.inbox.is_some() && self.filing_location.is_some() && self.problem().is_none()
    }

    /// `false` while another lookup or the start is running.
    pub fn start_picking(&mut self, slot: FolderSlot) -> bool {
        if self.is_busy() {
            return false;
        }
        self.picking = Some(slot);
        self.note = None;
        true
    }

    /// The dialog was cancelled or could not open.
    pub fn cancel_picking(&mut self) {
        self.picking = None;
    }

    pub fn finish_picking(
        &mut self,
        slot: FolderSlot,
        folder: PathBuf,
        resolved: Result<PathBuf, String>,
    ) {
        if self.picking != Some(slot) {
            return;
        }
        self.picking = None;
        match resolved {
            Ok(canonical) => match slot {
                FolderSlot::Inbox => self.inbox = Some(canonical),
                FolderSlot::FilingLocation => self.filing_location = Some(canonical),
            },
            Err(reason) => self.note = Some(SetupNote::FolderUnusable { folder, reason }),
        }
    }

    pub fn choose_encrypted(&mut self, encrypted: bool) {
        if !self.is_busy() {
            self.encrypted = encrypted;
        }
    }

    pub fn toggle_load_model(&mut self) {
        if !self.is_busy() && self.offer.model_download {
            self.load_model = !self.load_model;
        }
    }

    /// What to do when Start is pressed, or `None` when it cannot be now.
    pub fn start(&mut self) -> Option<StartRequest> {
        if self.is_busy() || !self.can_start() {
            return None;
        }
        self.writing = true;
        self.note = None;
        Some(StartRequest {
            locations: self.locations(),
            plan: StartPlan {
                encrypted: self.encrypted,
                load_model: self.load_model && self.offer.model_download,
                index_requested: false,
            },
        })
    }

    /// The write did not happen or failed: the card is free again.
    pub fn stop_writing(&mut self, note: SetupNote) {
        self.writing = false;
        self.note = Some(note);
    }

    pub fn written(&mut self) {
        self.writing = false;
    }

    pub fn show_note(&mut self, note: SetupNote) {
        self.note = Some(note);
    }
}

// reason: a failing assertion is the signal in a test
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        PathBuf::from("/home/user").join(name)
    }

    fn offer() -> SetupOffer {
        SetupOffer {
            existing_index: None,
            model_download: true,
            hermes_was_off: false,
        }
    }

    fn location(mode: LocationMode, name: &str, is_target: bool) -> FilingLocation {
        FilingLocation {
            path: folder(name),
            mode,
            images: false,
            is_target,
        }
    }

    fn picked(setup: &mut HermesSetup, slot: FolderSlot, name: &str) {
        assert!(setup.start_picking(slot));
        setup.finish_picking(slot, folder(name), Ok(folder(name)));
    }

    fn ready_to_start() -> HermesSetup {
        let mut setup = HermesSetup::new(&[], offer());
        picked(&mut setup, FolderSlot::Inbox, "tubes");
        picked(&mut setup, FolderSlot::FilingLocation, "filed");
        setup
    }

    #[test]
    fn starting_needs_both_folders() {
        let mut setup = HermesSetup::new(&[], offer());
        assert!(!setup.can_start());
        assert_eq!(setup.start(), None);

        picked(&mut setup, FolderSlot::Inbox, "tubes");
        assert!(!setup.can_start());
        assert_eq!(setup.problem(), None, "a missing folder is not a problem");

        picked(&mut setup, FolderSlot::FilingLocation, "filed");
        assert!(setup.can_start());
    }

    #[test]
    fn folders_that_lie_in_each_other_cannot_be_started_with() {
        let mut setup = HermesSetup::new(&[], offer());
        picked(&mut setup, FolderSlot::Inbox, "docs");
        let inside = PathBuf::from("/home/user/docs").join("filed");
        assert!(setup.start_picking(FolderSlot::FilingLocation));
        setup.finish_picking(FolderSlot::FilingLocation, inside.clone(), Ok(inside));

        let (problem, _) = setup.problem().unwrap();
        assert!(matches!(problem, SetupProblem::Rule(_)), "{problem:?}");
        assert!(!setup.can_start());
        assert_eq!(setup.start(), None);
    }

    #[test]
    fn the_same_folder_twice_is_a_problem() {
        let mut setup = HermesSetup::new(&[], offer());
        picked(&mut setup, FolderSlot::Inbox, "docs");
        picked(&mut setup, FolderSlot::FilingLocation, "docs");
        assert!(setup.problem().is_some());
        assert!(!setup.can_start());
    }

    #[test]
    fn starting_hands_out_the_locations_and_the_choice() {
        let mut setup = ready_to_start();
        setup.choose_encrypted(false);
        setup.toggle_load_model();

        let request = setup.start().unwrap();

        assert_eq!(
            request.locations,
            [
                location(LocationMode::Inbox, "tubes", false),
                location(LocationMode::Archive, "filed", true),
            ]
        );
        assert!(!request.plan.encrypted);
        assert!(request.plan.load_model);
        assert!(setup.is_busy(), "busy until the write is answered");
        assert_eq!(setup.start(), None);
    }

    #[test]
    fn the_model_is_not_asked_for_when_it_is_not_offered() {
        let mut setup = HermesSetup::new(
            &[],
            SetupOffer {
                model_download: false,
                ..offer()
            },
        );
        picked(&mut setup, FolderSlot::Inbox, "tubes");
        picked(&mut setup, FolderSlot::FilingLocation, "filed");
        setup.toggle_load_model();

        assert!(!setup.start().unwrap().plan.load_model);
    }

    #[test]
    fn the_card_starts_from_the_configured_locations_and_keeps_the_rest() {
        let configured = [
            location(LocationMode::Inbox, "tubes", false),
            location(LocationMode::Archive, "records", false),
            location(LocationMode::Archive, "filed", true),
        ];
        let mut setup = HermesSetup::new(&configured, offer());
        assert_eq!(setup.folder(FolderSlot::Inbox), Some(folder("tubes").as_path()));
        assert_eq!(
            setup.folder(FolderSlot::FilingLocation),
            Some(folder("filed").as_path())
        );
        assert!(setup.can_start());

        picked(&mut setup, FolderSlot::Inbox, "scans");
        let kept = setup.locations();

        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0], location(LocationMode::Inbox, "scans", false));
        assert!(
            kept.contains(&location(LocationMode::Archive, "records", false)),
            "existing archives stay"
        );
        assert!(kept.contains(&location(LocationMode::Archive, "filed", true)));
        assert!(
            !kept.iter().any(|entry| entry.path == folder("tubes")),
            "the inbox that was replaced goes"
        );
    }

    #[test]
    fn a_new_filing_location_takes_the_target_from_the_old_one() {
        let configured = [
            location(LocationMode::Inbox, "tubes", false),
            location(LocationMode::Archive, "filed", true),
        ];
        let mut setup = HermesSetup::new(&configured, offer());
        picked(&mut setup, FolderSlot::FilingLocation, "records");

        let list = setup.locations();

        assert_eq!(list.iter().filter(|entry| entry.is_target).count(), 1);
        assert!(list.contains(&location(LocationMode::Archive, "records", true)));
        assert!(list.contains(&location(LocationMode::Archive, "filed", false)));
        assert!(setup.can_start());
    }

    #[test]
    fn an_unusable_folder_is_a_note_and_changes_nothing() {
        let mut setup = HermesSetup::new(&[], offer());
        assert!(setup.start_picking(FolderSlot::Inbox));
        setup.finish_picking(FolderSlot::Inbox, folder("gone"), Err("not found".to_owned()));

        assert_eq!(setup.folder(FolderSlot::Inbox), None);
        assert!(matches!(setup.note(), Some(SetupNote::FolderUnusable { .. })));
        assert!(!setup.is_busy());
    }

    #[test]
    fn nothing_changes_while_a_pick_or_the_start_is_running() {
        let mut setup = ready_to_start();
        assert!(setup.start_picking(FolderSlot::Inbox));
        assert!(!setup.start_picking(FolderSlot::FilingLocation));
        setup.choose_encrypted(false);
        assert!(setup.encrypted());
        setup.cancel_picking();

        assert!(setup.start().is_some());
        setup.toggle_load_model();
        assert!(!setup.load_model());
        setup.written();
        assert!(!setup.is_busy());
    }

    #[test]
    fn the_keys_skip_what_the_card_does_not_show() {
        let mut setup = ready_to_start();
        assert_eq!(
            setup.focus_order(true),
            [
                SetupFocus::ChooseInbox,
                SetupFocus::ChooseFilingLocation,
                SetupFocus::Encrypted,
                SetupFocus::Plain,
                SetupFocus::LoadModel,
                SetupFocus::Later,
                SetupFocus::Start,
            ]
        );
        assert_eq!(setup.first_focus(true), 6, "Enter starts");
        assert_eq!(setup.first_focus(false), 0);

        setup.offer = SetupOffer {
            existing_index: Some(ExistingIndex::Encrypted),
            model_download: false,
            hermes_was_off: false,
        };
        assert_eq!(
            setup.focus_order(false),
            [
                SetupFocus::ChooseInbox,
                SetupFocus::ChooseFilingLocation,
                SetupFocus::Later,
            ]
        );
    }

    fn plan(encrypted: bool, load_model: bool) -> StartPlan {
        StartPlan {
            encrypted,
            load_model,
            index_requested: false,
        }
    }

    fn asks(busy: bool, error: Option<&str>) -> Phase {
        Phase::AskEncryption {
            encrypted: true,
            busy,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn the_plan_asks_for_the_index_once_with_the_choice() {
        let mut plan = plan(false, false);

        assert_eq!(plan.next_step(&Phase::Starting), PlanStep::Wait);
        assert_eq!(
            plan.next_step(&asks(false, None)),
            PlanStep::CreateIndex { encrypted: false }
        );
        assert_eq!(plan.next_step(&asks(true, None)), PlanStep::Wait);
        assert_eq!(
            plan.next_step(&asks(false, Some("disk full"))),
            PlanStep::Abandon,
            "a second question means the first attempt failed"
        );
    }

    #[test]
    fn the_plan_is_done_when_the_index_is_open_and_keeps_the_model_choice() {
        assert_eq!(
            plan(true, true).next_step(&Phase::Ready),
            PlanStep::Done { load_model: true }
        );
        assert_eq!(
            plan(true, false).next_step(&Phase::Ready),
            PlanStep::Done { load_model: false }
        );
    }

    #[test]
    fn the_plan_waits_for_the_cards_of_the_module_and_ends_with_its_failure() {
        let mut plan = plan(true, false);
        assert_eq!(plan.next_step(&Phase::PlainFileWarning), PlanStep::Wait);
        assert_eq!(
            plan.next_step(&Phase::Failed("no".to_owned())),
            PlanStep::Abandon
        );
        assert_eq!(plan.next_step(&Phase::Dismissed), PlanStep::Abandon);
    }

    #[test]
    fn the_card_is_done_once_the_module_is_past_the_question() {
        assert!(!card_is_done_at(&Phase::Starting));
        assert!(!card_is_done_at(&asks(false, None)));
        assert!(card_is_done_at(&Phase::Ready));
        assert!(card_is_done_at(&Phase::PlainFileWarning));
    }

    #[test]
    fn an_index_file_is_judged_by_its_header() {
        let dir = tempfile::TempDir::new().unwrap();
        let absent = dir.path().join("none.db");
        assert_eq!(ExistingIndex::of_file(&absent), None);

        let plain = dir.path().join("plain.db");
        std::fs::write(&plain, b"SQLite format 3\0 and more").unwrap();
        assert_eq!(ExistingIndex::of_file(&plain), Some(ExistingIndex::Plain));

        let encrypted = dir.path().join("encrypted.db");
        std::fs::write(&encrypted, [u8::MAX; 64]).unwrap();
        assert_eq!(
            ExistingIndex::of_file(&encrypted),
            Some(ExistingIndex::Encrypted)
        );
    }
}
