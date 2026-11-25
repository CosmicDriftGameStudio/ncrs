//! The filing tab on a real index in a temporary folder, without a model.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use ncrs_config::config::TargetSetting;
use ncrs_filing::chat_work::ReadProgress;
use ncrs_filing::sorting::{
    run_watched, RowState, RunRequest, SortingAccess, WaitReason, WatchedFiles, WatchedReport,
};
use ncrs_fs::transfer::Counting;
use ncrs_fs::{FolderBackend, JournaledTrash};
use ncrs_i18n::{Language, Msg};
use ncrs_index::{Condition, ConditionKind, Index, RuleConditions, RuleOrigin, RuleSpec, TargetId};
use ncrs_intake::{
    canonical_path, LocationTransfer, OnNoText, OnSeveralTargets, SortSource, SortTarget,
};
use ncrs_ui_kit::palette::ThemePreset;
use tempfile::TempDir;

use crate::decide::{decide_once, decide_questions, Decided};
use crate::footer::figures;
use crate::left::what_happens;
use crate::run::{carry_answer, carry_out, Duplicates, RunProgress};
use crate::tree::{tree_items, Item};
use crate::{
    busy_source, filing_key, is_running, key_facts, open, question_is_open, tab_title, update,
    Action, Change, Ctx, Link, Message, Session, State,
};

const LANG: Language = Language::German;

struct Setup {
    root: PathBuf,
    access: SortingAccess,
    _dir: TempDir,
}

fn id(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}

fn spec(name: &str, word: &str, pattern: &str) -> RuleSpec {
    RuleSpec {
        name: Some(name.to_owned()),
        conditions: RuleConditions::all_of(vec![Condition::new(ConditionKind::Substring, word)]),
        extract_number: Some(r"Ref (\w+)".to_owned()),
        extract_date: None,
        extract_amount: None,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: pattern.to_owned(),
        confirmed: true,
        enabled: true,
    }
}

/// Invoices go to `t1`, letters to `t2`; a file can match both.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical_path(dir.path()).unwrap();
    let mut index = Index::create_plain(&root.join("index.db")).unwrap();
    index
        .create_rule_in(
            &spec("Invoices", "invoice", "Invoices/{number}"),
            RuleOrigin::default(),
            &id("t1"),
        )
        .unwrap();
    index
        .create_rule_in(
            &spec("Letters", "letter", "Letters/{number}"),
            RuleOrigin::default(),
            &id("t2"),
        )
        .unwrap();
    let targets = [("t1", "Documents"), ("t2", "Company")]
        .into_iter()
        .map(|(target, folder)| {
            let path = root.join(folder);
            fs::create_dir_all(&path).unwrap();
            TargetSetting {
                target: SortTarget {
                    id: id(target),
                    name: folder.to_owned(),
                    path,
                    own_volume: Some(false),
                },
                paused: None,
            }
        })
        .collect();
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(root.join("Inbox")).unwrap();
    let trash = JournaledTrash::new(root.join("trash.toml"), FolderBackend(bin));
    Setup {
        access: SortingAccess::for_tests(index, Vec::new(), targets, Arc::new(trash)),
        root,
        _dir: dir,
    }
}

impl Setup {
    fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    /// A file nobody has written to for a while.
    fn write(&self, relative: &str, content: &str) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(600))
            .unwrap();
        path
    }

    fn source(&self) -> SortSource {
        SortSource {
            path: self.path("Inbox"),
            include: String::new(),
            exclude: String::new(),
            targets: vec![id("t1"), id("t2")],
            watch: true,
            transfer: LocationTransfer::Move,
            no_text: OnNoText::Ask,
            several: OnSeveralTargets::Ask,
            own_volume: None,
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            lang: LANG,
            access: Some(self.access.clone()),
        }
    }

    fn decided(&self, read: bool) -> Decided {
        decide_once(
            &self.access,
            &self.source(),
            read,
            &ReadProgress::default(),
            &AtomicBool::new(false),
        )
        .unwrap()
    }

    /// The tab once the plan of the whole folder has come back.
    fn planned(&self) -> State {
        let mut state = State {
            session: Some(Session::new(self.source(), true)),
        };
        self.plan_again(&mut state);
        state
    }

    fn plan_again(&self, state: &mut State) {
        let message = Message::Planned {
            session: session(state).id,
            read: true,
            result: Ok(Box::new(self.decided(true))),
        };
        send(state, message, &self.ctx());
    }

    /// The first look only notes what lies in the source already.
    fn watch(&self) -> WatchedReport {
        run_watched(
            &self.access,
            &self.source(),
            &WatchedFiles::All,
            None,
            "watched",
            &mut Counting,
        )
        .unwrap()
    }

    /// The open questions of the watched source, after its first filing.
    fn questions(&self) -> State {
        self.decided(true);
        self.watch();
        let mut state = State {
            session: Some(Session::new(self.source(), false)),
        };
        let message = Message::Planned {
            session: session(&state).id,
            read: false,
            result: decide_questions(&self.access, &self.source()).map(Box::new),
        };
        send(&mut state, message, &self.ctx());
        state
    }

    /// What the buttons carry out, done at once.
    fn carry(&self, state: &mut State, transfer: LocationTransfer) -> Option<Action> {
        let shown = session(state);
        let request = RunRequest {
            name: "sorted".to_owned(),
            source: shown.source.clone(),
            entries: shown.entries(),
            transfer,
        };
        let duplicates = Duplicates {
            name: "duplicates".to_owned(),
            items: Vec::new(),
        };
        let progress = Arc::new(RunProgress::new(request.entries.len()));
        let id = shown.id;
        let carried = carry_out(&self.access, &request, &duplicates, progress, LANG).unwrap();
        send(
            state,
            Message::Carried(id, Ok(Box::new(carried))),
            &self.ctx(),
        )
    }
}

fn session(state: &State) -> &Session {
    state.session.as_ref().unwrap()
}

/// The tasks are dropped: what they would bring is sent by hand.
fn send(state: &mut State, message: Message, ctx: &Ctx) -> Option<Action> {
    let (_task, action) = update(state, message, ctx);
    action
}

fn texts(state: &State) -> Vec<String> {
    let palette = ThemePreset::Cosmic.palette();
    figures(session(state), LANG, &palette)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

fn what(state: &State, name: &str) -> String {
    let palette = ThemePreset::Cosmic.palette();
    let shown = session(state);
    let row = shown.row(Path::new(name)).unwrap();
    what_happens(shown, row, LANG, &palette).0
}

fn places(state: &State, name: &str) -> Vec<(TargetId, PathBuf)> {
    let shown = session(state);
    let row = shown.row(Path::new(name)).unwrap();
    shown.places(row).into_owned()
}

fn links(state: &State, name: &str) -> Vec<Link> {
    let shown = session(state);
    shown.links(shown.row(Path::new(name)).unwrap())
}

fn select(state: &mut State, name: &str, ctx: &Ctx) {
    send(state, Message::Select(PathBuf::from(name)), ctx);
}

fn in_target(target: &str, folder: &str, name: &str) -> (TargetId, PathBuf) {
    (id(target), Path::new(folder).join(name))
}

/// One file per state: filed, in both targets, without a rule, without text.
fn one_of_each(setup: &Setup) {
    setup.write("Inbox/a.txt", "invoice Ref A1");
    setup.write("Inbox/both.txt", "invoice and letter Ref B1");
    setup.write("Inbox/none.txt", "something else");
    setup.write("Inbox/blank.txt", " ");
}

#[test]
fn opening_plans_and_files_nothing() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = State::default();
    let _plan = open(&mut state, setup.source(), true, &setup.ctx());
    assert!(is_running(&state));
    assert!(!session(&state).can_file());
    assert_eq!(busy_source(&state), Some(setup.path("Inbox").as_path()));
    assert_eq!(tab_title(&state, LANG).unwrap(), "Einsortieren · Inbox");

    setup.plan_again(&mut state);
    assert!(session(&state).can_file());
    for name in ["a", "both", "none", "blank"] {
        assert!(setup.path(&format!("Inbox/{name}.txt")).exists());
    }
    assert!(!setup.path("Documents/Invoices").exists());
}

#[test]
fn without_the_index_the_tab_says_so() {
    let setup = setup();
    let mut state = State::default();
    let ctx = Ctx {
        lang: LANG,
        access: None,
    };
    let _none = open(&mut state, setup.source(), true, &ctx);
    assert_eq!(
        session(&state).error.as_deref(),
        Some(LANG.text(Msg::IndexNotOpen))
    );
    assert!(!is_running(&state));
}

#[test]
fn the_first_look_shows_rows_and_then_reads() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = State::default();
    let _plan = open(&mut state, setup.source(), true, &setup.ctx());
    let message = Message::Planned {
        session: session(&state).id,
        read: false,
        result: Ok(Box::new(setup.decided(false))),
    };
    send(&mut state, message, &setup.ctx());
    assert_eq!(session(&state).rows.len(), 4);
    assert!(session(&state).planning && session(&state).reading);
    assert_eq!(
        what(&state, "a.txt"),
        LANG.text(Msg::FilingViewStateLooking)
    );
}

#[test]
fn the_plan_of_another_session_is_dropped() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = State {
        session: Some(Session::new(setup.source(), true)),
    };
    let message = Message::Planned {
        session: session(&state).id + 1,
        read: true,
        result: Ok(Box::new(setup.decided(true))),
    };
    send(&mut state, message, &setup.ctx());
    assert!(session(&state).rows.is_empty() && !session(&state).planned);
}

#[test]
fn every_row_says_what_happens_and_what_can_be_done() {
    let setup = setup();
    setup.write("Inbox/copied.txt", "invoice Ref OLD");
    let mut state = setup.planned();
    setup.carry(&mut state, LocationTransfer::Copy);
    one_of_each(&setup);
    setup.plan_again(&mut state);
    state.session.as_mut().unwrap().outcome = None;
    let place = Path::new("Invoices").join("A1.txt").display().to_string();
    assert_eq!(what(&state, "a.txt"), format!("Invoices → T1 {place}"));
    assert_eq!(what(&state, "both.txt"), "passt in T1 und T2");
    assert_eq!(what(&state, "none.txt"), LANG.text(Msg::SortingStateNoRule));
    assert_eq!(
        what(&state, "blank.txt"),
        LANG.text(Msg::SortingStateNoTextAsk)
    );
    let there = Path::new("Invoices").join("OLD.txt").display().to_string();
    assert_eq!(
        what(&state, "copied.txt"),
        format!("= schon da: T1 {there}")
    );

    assert!(links(&state, "a.txt").is_empty());
    assert_eq!(links(&state, "both.txt"), [Link::Ask]);
    assert_eq!(
        links(&state, "none.txt"),
        [Link::MakeRule, Link::SuggestRule]
    );
    assert_eq!(links(&state, "blank.txt"), [Link::Assign, Link::Leave]);
    assert_eq!(
        texts(&state),
        [
            "5 gelesen",
            "1 nach Regel",
            "1 schon da",
            "1 ohne passende Regel",
            "1 ohne Text",
            "1 in T1 und T2"
        ]
    );
}

#[test]
fn a_run_files_what_has_a_place_and_tells_what_stays() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = setup.planned();
    let action = setup.carry(&mut state, LocationTransfer::Move);
    assert!(setup.path("Documents/Invoices/A1.txt").exists());
    assert!(!setup.path("Inbox/a.txt").exists());
    assert!(setup.path("Inbox/both.txt").exists());
    assert_eq!(
        texts(&state),
        ["1 einsortiert", "3 Dateien liegen weiter hier"]
    );
    let Some(Action::Filed {
        source,
        transfer,
        filed,
        left,
        batch,
    }) = action
    else {
        panic!("expected what was filed, got {action:?}");
    };
    assert_eq!(source, setup.path("Inbox"));
    assert_eq!((transfer, filed, left), (LocationTransfer::Move, 1, 3));
    assert!(batch.is_some());
    assert!(session(&state).planning, "the rest is decided again");
}

#[test]
fn the_button_hands_the_run_over_and_no_second_one_starts() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = setup.planned();
    let key = Key::Named(Named::Enter);
    let message = filing_key(&key, Modifiers::empty(), key_facts(&state)).unwrap();
    assert!(matches!(message, Message::File(LocationTransfer::Move)));
    let (_dropped_run, action) = update(&mut state, message, &setup.ctx());
    assert_eq!(action, None);
    assert_eq!(texts(&state), ["0 von 1 werden einsortiert …"]);
    assert!(!session(&state).can_file());

    select(&mut state, "a.txt", &setup.ctx());
    send(&mut state, Message::Stay, &setup.ctx());
    assert!(
        session(&state).changes.is_empty(),
        "the run goes on as shown"
    );

    let closed = send(&mut state, Message::Cancel, &setup.ctx());
    assert_eq!(closed, None, "the tab stays to show what the run did");
    assert!(session(&state).running.as_ref().unwrap().is_stopped());
}

#[test]
fn a_copying_source_copies_on_enter_and_moves_when_asked() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = setup.planned();
    state.session.as_mut().unwrap().source.transfer = LocationTransfer::Copy;
    send(&mut state, Message::ToggleTrashDupes, &setup.ctx());
    assert!(!session(&state).trash_dupes);
    let key = Key::Named(Named::Enter);
    let message = filing_key(&key, Modifiers::empty(), key_facts(&state)).unwrap();
    assert!(matches!(message, Message::File(LocationTransfer::Copy)));
    let shifted_message = filing_key(&key, Modifiers::SHIFT, key_facts(&state)).unwrap();
    assert!(matches!(
        shifted_message,
        Message::File(LocationTransfer::Move)
    ));
    let (_dropped_run, _) = update(&mut state, shifted_message, &setup.ctx());
    assert!(session(&state).running.is_some());
}

#[test]
fn folders_that_only_lead_to_one_other_share_its_row() {
    let setup = setup();
    setup.write("Inbox/a.txt", "invoice Ref A1");
    setup.write("Inbox/b.txt", "invoice Ref B1");
    let ctx = setup.ctx();
    let mut state = setup.planned();
    let place = |folder: &str| {
        let folder_path = folder.split('/').collect::<PathBuf>();
        Change::Places(vec![(id("t1"), folder_path.join("x.txt"))])
    };
    let rows = |planned_state: &State| -> Vec<(String, usize)> {
        tree_items(session(planned_state), &id("t1"))
            .into_iter()
            .filter_map(|item| match item {
                Item::Folder { label, depth, .. } => Some((label.replace('\\', "/"), depth)),
                _ => None,
            })
            .collect()
    };
    let row = |label: &str, depth: usize| (label.to_owned(), depth);
    let shown = state.session.as_mut().unwrap();
    shown
        .changes
        .insert(PathBuf::from("a.txt"), place("Bank/North/2026"));
    shown
        .changes
        .insert(PathBuf::from("b.txt"), place("Bank/North/2026"));
    assert_eq!(rows(&state), [row("", 0), row("Bank/North/2026", 1)]);

    let reassigned = state.session.as_mut().unwrap();
    reassigned
        .changes
        .insert(PathBuf::from("b.txt"), place("Bank/North/2025"));
    assert_eq!(
        rows(&state),
        [
            row("", 0),
            row("Bank/North", 1),
            row("2025", 2),
            row("2026", 2)
        ]
    );
    let folder = Path::new("Bank").join("North");
    send(&mut state, Message::ToggleFolder(id("t1"), folder), &ctx);
    assert_eq!(rows(&state), [row("", 0), row("Bank/North", 1)]);

    let reopened = state.session.as_mut().unwrap();
    reopened.closed_folders.clear();
    reopened
        .changes
        .insert(PathBuf::from("b.txt"), place("Bank"));
    assert_eq!(
        rows(&state),
        [row("", 0), row("Bank", 1), row("North/2026", 2)]
    );
}

#[test]
fn a_problem_of_one_file_is_told_and_the_others_are_filed() {
    let setup = setup();
    setup.write("Inbox/a.txt", "invoice Ref A1");
    setup.write("Inbox/b.txt", "invoice Ref B1");
    let mut state = setup.planned();
    setup.write("Documents/Invoices/B1.txt", "somebody else's");
    setup.carry(&mut state, LocationTransfer::Move);
    assert!(setup.path("Documents/Invoices/A1.txt").exists());
    assert!(setup.path("Inbox/b.txt").exists());
    let outcome = session(&state).outcome.as_ref().unwrap();
    assert_eq!((outcome.filed, outcome.left), (1, 1));
    assert_eq!(outcome.problems.len(), 1);
    assert!(outcome.problems[0].starts_with("b.txt"));
}

#[test]
fn a_waiting_target_stops_everything_until_it_is_there() {
    let setup = setup();
    one_of_each(&setup);
    fs::remove_dir(setup.path("Company")).unwrap();
    let mut state = setup.planned();
    assert!(session(&state)
        .waiting
        .contains(&(id("t2"), WaitReason::Missing)));
    assert!(!session(&state).can_file());
    send(
        &mut state,
        Message::File(LocationTransfer::Move),
        &setup.ctx(),
    );
    assert!(session(&state).running.is_none());

    fs::create_dir(setup.path("Company")).unwrap();
    send(&mut state, Message::Recheck, &setup.ctx());
    assert!(session(&state).reading);
    setup.plan_again(&mut state);
    assert!(session(&state).can_file());
}

#[test]
fn a_changed_rule_decides_again_without_reading() {
    let setup = setup();
    one_of_each(&setup);
    let mut state = setup.planned();
    send(&mut state, Message::RulesChanged, &setup.ctx());
    assert!(session(&state).planning && !session(&state).reading);
    send(&mut state, Message::RulesChanged, &setup.ctx());
    assert!(session(&state).stale);
    setup.plan_again(&mut state);
    assert!(session(&state).planning && !session(&state).stale);
    setup.plan_again(&mut state);
    assert!(!session(&state).planning);
}

#[test]
fn a_new_name_keeps_the_folder_and_is_checked() {
    let setup = setup();
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.planned();
    select(&mut state, "a.txt", &ctx);
    send(&mut state, Message::StartRename, &ctx);
    assert_eq!(session(&state).renaming.as_deref(), Some("A1.txt"));
    assert!(filing_key(
        &Key::Named(Named::Enter),
        Modifiers::empty(),
        key_facts(&state)
    )
    .is_none());
    for bad in ["", "..", "x/y.txt"] {
        send(&mut state, Message::RenameInput(bad.to_owned()), &ctx);
        send(&mut state, Message::RenameSubmit, &ctx);
        assert!(session(&state).renaming.is_some());
        assert!(session(&state).error.is_some());
    }
    send(
        &mut state,
        Message::RenameInput("Heating.txt".to_owned()),
        &ctx,
    );
    send(&mut state, Message::RenameSubmit, &ctx);
    assert_eq!(
        places(&state, "a.txt"),
        [in_target("t1", "Invoices", "Heating.txt")]
    );
    assert!(session(&state).error.is_none());

    setup.plan_again(&mut state);
    assert_eq!(
        places(&state, "a.txt"),
        [in_target("t1", "Invoices", "Heating.txt")],
        "a new plan keeps what the user decided"
    );
}

#[test]
fn a_folder_is_only_taken_below_a_target_of_the_source() {
    let setup = setup();
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.planned();
    select(&mut state, "a.txt", &ctx);
    let action = send(&mut state, Message::PickFolder, &ctx);
    assert_eq!(
        action,
        Some(Action::PickFolder {
            start: setup.path("Documents")
        }),
        "its own folder is not there yet"
    );
    send(
        &mut state,
        Message::FolderPicked(Some(setup.path("Inbox"))),
        &ctx,
    );
    assert_eq!(
        session(&state).error.as_deref(),
        Some(LANG.text(Msg::FilingViewErrorOutside))
    );
    let folder = setup.path("Company/Archive");
    fs::create_dir_all(&folder).unwrap();
    send(&mut state, Message::FolderPicked(Some(folder)), &ctx);
    assert_eq!(
        places(&state, "a.txt"),
        [in_target("t2", "Archive", "A1.txt")]
    );
}

#[test]
fn staying_is_taken_back_by_pressing_it_again() {
    let setup = setup();
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.planned();
    select(&mut state, "a.txt", &ctx);
    send(&mut state, Message::Stay, &ctx);
    assert!(places(&state, "a.txt").is_empty());
    assert!(!session(&state).can_file());
    send(&mut state, Message::Stay, &ctx);
    assert_eq!(places(&state, "a.txt").len(), 1);
}

#[test]
fn the_question_about_two_targets_is_answered_by_keys() {
    let setup = setup();
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.planned();
    select(&mut state, "both.txt", &ctx);
    for key in [Named::ArrowRight, Named::Enter] {
        let message = filing_key(&Key::Named(key), Modifiers::empty(), key_facts(&state)).unwrap();
        send(&mut state, message, &ctx);
    }
    assert!(question_is_open(&state));
    assert!(setup.path("Inbox/both.txt").exists());
    let message = filing_key(
        &Key::Named(Named::Enter),
        Modifiers::empty(),
        key_facts(&state),
    )
    .unwrap();
    send(&mut state, message, &ctx);
    assert!(!question_is_open(&state));
    assert_eq!(
        places(&state, "both.txt"),
        [in_target("t1", "Invoices", "B1.txt")]
    );
}

#[test]
fn both_targets_get_a_copy_and_the_file_stays() {
    let setup = setup();
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.planned();
    send(&mut state, Message::Ask(PathBuf::from("both.txt")), &ctx);
    send(&mut state, Message::QuestionAnswer(vec![0, 1]), &ctx);
    assert_eq!(places(&state, "both.txt").len(), 2);
    setup.carry(&mut state, LocationTransfer::Move);
    assert!(setup.path("Documents/Invoices/B1.txt").exists());
    assert!(setup.path("Company/Letters/B1.txt").exists());
    assert!(setup.path("Inbox/both.txt").exists());
}

#[test]
fn the_links_reach_the_app() {
    let setup = setup();
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.planned();
    let path = setup.path("Inbox/none.txt");
    select(&mut state, "none.txt", &ctx);
    send(&mut state, Message::LinkNext, &ctx);
    let action = send(&mut state, Message::PressMarkedLink, &ctx);
    assert!(
        matches!(
            &action,
            Some(Action::OpenRuleForm { target, path: asked, .. })
                if *asked == path && target.as_ref() == session(&state).source.targets.first()
        ),
        "{action:?}"
    );
    send(&mut state, Message::LinkNext, &ctx);
    let suggest_action = send(&mut state, Message::PressMarkedLink, &ctx);
    assert_eq!(
        suggest_action,
        Some(Action::SuggestRule {
            source: setup.path("Inbox"),
            path
        })
    );
    send(&mut state, Message::LinkPrevious, &ctx);
    send(&mut state, Message::LinkPrevious, &ctx);
    assert_eq!(session(&state).marked_link, None);
}

#[test]
fn a_tree_per_target_shows_only_the_folders_that_get_something() {
    let setup = setup();
    one_of_each(&setup);
    fs::create_dir_all(setup.path("Documents/Empty")).unwrap();
    let ctx = setup.ctx();
    let mut state = setup.planned();
    let folders = |planned_state: &State, target: &str| -> Vec<(PathBuf, usize)> {
        tree_items(session(planned_state), &id(target))
            .into_iter()
            .filter_map(|item| match item {
                Item::Folder {
                    path, new_files, ..
                } => Some((path, new_files)),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        folders(&state, "t1"),
        [(PathBuf::new(), 1), (PathBuf::from("Invoices"), 1)]
    );
    assert_eq!(folders(&state, "t2"), [(PathBuf::new(), 0)]);

    send(&mut state, Message::ToggleOnlyNew, &ctx);
    let listed = BTreeSet::from([(id("t1"), PathBuf::from("Empty"))]);
    let session_id = session(&state).id;
    send(&mut state, Message::FoldersListed(session_id, listed), &ctx);
    assert!(folders(&state, "t1").contains(&(PathBuf::from("Empty"), 0)));
}

#[test]
fn a_watched_source_shows_only_its_questions_and_answers_them_at_once() {
    let setup = setup();
    assert_eq!(setup.watch(), WatchedReport::BacklogRecorded { files: 0 });
    one_of_each(&setup);
    let ctx = setup.ctx();
    let mut state = setup.questions();
    assert!(setup.path("Documents/Invoices/A1.txt").exists());
    assert!(!texts(&state).iter().any(|figure| figure.starts_with("0 ")));
    let shown = session(&state);
    let mut unmatched = shown.row(Path::new("both.txt")).unwrap().clone();
    unmatched.state = RowState::NoRule;
    assert_eq!(
        shown.links(&unmatched),
        [Link::MakeRule, Link::SuggestRule, Link::Leave]
    );
    assert!(shown.row(Path::new("a.txt")).is_none());
    assert!(matches!(
        shown.row(Path::new("both.txt")).unwrap().state,
        RowState::Several(_)
    ));
    assert!(!shown.can_file());

    send(&mut state, Message::Ask(PathBuf::from("both.txt")), &ctx);
    let (_dropped_answer, _) = update(&mut state, Message::QuestionAnswer(vec![1]), &ctx);
    let answering = session(&state);
    assert!(answering.answering.contains(Path::new("both.txt")));
    assert!(answering.changes.is_empty());

    let document = answering
        .row(Path::new("both.txt"))
        .unwrap()
        .document
        .unwrap();
    let chosen = Change::Places(vec![in_target("t2", "Letters", "B1.txt")]);
    let answer = carry_answer(
        &setup.access,
        &setup.source(),
        document,
        chosen,
        "answered",
        LANG,
    )
    .unwrap();
    let message = Message::Answered(
        answering.id,
        PathBuf::from("both.txt"),
        Ok(Box::new(answer)),
    );
    let action = send(&mut state, message, &ctx);
    assert!(setup.path("Company/Letters/B1.txt").exists());
    assert!(matches!(action, Some(Action::Filed { filed: 1, .. })));
    assert!(session(&state).planning);
}

#[test]
fn closing_stops_the_planning() {
    let setup = setup();
    let mut state = State::default();
    let _plan = open(&mut state, setup.source(), true, &setup.ctx());
    let stop = Arc::clone(&session(&state).stop);
    let action = send(&mut state, Message::Cancel, &setup.ctx());
    assert_eq!(action, Some(Action::Close));
    assert!(state.session.is_none());
    assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
}
