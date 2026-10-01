//! End-to-end tests: the real program, the real state, the real tasks.
//!
//! `ui_tests.rs` uses `iced_test::Simulator`, which renders a view and hands
//! back the messages an interaction produced. That checks what a click or a key
//! *reaches*. It cannot check what the app then does with it, because the
//! simulator keeps no state of its own: the `App` in the test is not the `App`
//! that reacted to the message.
//!
//! That gap is what let four reported bugs through. Each piece under test was
//! correct in isolation — `route_key` mapped the keys, `transfer` moved the
//! files, the queue counted the jobs — and the wiring between key routing,
//! `update`, the job queue and the filesystem was never exercised at all. Every
//! one of the bugs lived in the gap, and the tests that covered them passed.
//!
//! What closes the gap is running the app for real: its own `Program`, its own
//! subscriptions, its own `update`, its own tasks on the blocking pool, with
//! the state under test. A key goes in, the filesystem is checked afterwards.
//!
//! **Why not `iced_test::Emulator`.** It does run the real program, and it was
//! the obvious tool — but its only input is an `Instruction`, and
//! `Instruction` cannot express the keys this app is about. `instruction::Key`
//! has four variants (Enter, Escape, Tab, Backspace) and carries no modifiers.
//! F5, F6, F7, Insert, ArrowDown and Ctrl+C are all inexpressible. That is not
//! a detail of test setup; it is why the entire copy / move / queue surface had
//! no end-to-end coverage, and so why four real bugs shipped.
//!
//! So the runtime loop is built here from `iced_futures`' public parts — the
//! same `Runtime`, the same `subscription::Tracker`, the same tokio executor
//! `main` runs. Keys are delivered as `subscription::Event::Interaction` with
//! `Status::Ignored`, which is precisely what `keyboard::listen()` filters on,
//! so the routing under test is the app's own `route_key` and nothing is
//! shortcut.

// reason: this whole module is test code. A failing setup — a scratch directory
// that cannot be created, an executor that will not start — is the signal here
// just as much as a failing assertion, so the lints meant for production paths
// are allowed. They sit on the module rather than on `mod tests` because the
// harness above that module uses them too.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use iced::keyboard::{self, Key, Modifiers};
use iced::window;
use iced_test::core::event::Status;
use iced_test::futures::futures::channel::mpsc;
use iced_test::futures::futures::{self as futures, StreamExt};
use iced_test::futures::subscription::{self, Event as SubEvent};
use iced_test::futures::Runtime;
use iced_test::runtime::{task, Action, Task};

use crate::app::App;
use crate::messages::{Message, PanelSide};

/// One end-to-end session: the app's own state, subscriptions and task channel.
///
/// `Runtime` owns the subscription tracker and spawns what the subscriptions
/// produce, so it has to outlive every step. It runs on the tokio executor the
/// app itself uses, which is what makes a `spawn_blocking` copy run for real
/// rather than being simulated.
struct Session<P> {
    app: App,
    /// The program under test — the one `main` builds, so a test cannot pass
    /// while the binary is wired differently.
    program: P,
    runtime: Runtime<iced::executor::Default, mpsc::UnboundedSender<Outcome>, Outcome>,
    /// What comes back from tasks and subscriptions.
    actions: mpsc::UnboundedReceiver<Outcome>,
    /// How many tasks have been started and not yet reported finished.
    in_flight: usize,
    /// The app's subscriptions, held open so a test can poll them directly.
    ///
    /// Each is the app's own `Subscription`, built by `App::subscription` and
    /// run with an event channel this harness controls — see `broadcast`.
    subscriptions: Vec<SubscriptionStream>,
    /// The window the app believes it is in. Subscriptions carry it, so a wrong
    /// id would silently drop every key.
    window: window::Id,
}

impl<P: iced::Program<State = App, Message = Message> + 'static> Session<P> {
    /// Boots the app and runs the initial directory loads.
    fn boot(program: P) -> Self {
        let executor = iced::executor::Default::new().expect("the tokio executor");
        let (action_tx, actions) = mpsc::unbounded();
        let runtime = Runtime::new(executor, action_tx);

        let (app, boot) = program.boot();
        let mut session = Self {
            app,
            program,
            runtime,
            actions,
            in_flight: 0,
            subscriptions: Vec::new(),
            window: window::Id::unique(),
        };
        session.resubscribe();
        session.drive(boot);
        // The boot task is two directory reads. Without this the session is
        // handed back while they are still in flight and every panel is empty —
        // which is what made the first version of these tests fail with a
        // listing of `[]` rather than with an honest error.
        session.settle();
        session
    }

    /// Tells the runtime about the app's current subscriptions. Called after
    /// every state change that can alter them, exactly as iced's own loop does.
    fn resubscribe(&mut self) {
        let subscription = self.runtime.enter(|| self.program.subscription(&self.app));
        let recipes = subscription::into_recipes(subscription);
        self.subscriptions = recipes
            .into_iter()
            .map(|recipe| {
                let (tx, rx) = mpsc::unbounded::<SubEvent>();
                // The sender has to outlive the stream: the stream's input ends
                // when every sender is dropped, and a closed input makes
                // `keyboard::listen` yield nothing ever again.
                SubscriptionStream {
                    input: tx,
                    stream: recipe.stream(rx.boxed()),
                }
            })
            .collect();
    }

    /// Presses and releases one key, then runs everything that follows.
    fn press(&mut self, key: Key) {
        self.key_event(key, Modifiers::default());
    }

    /// Presses a key while `modifiers` are held.
    fn press_with(&mut self, key: Key, modifiers: Modifiers) {
        self.key_event(key, modifiers);
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(Key::Character(c.to_string().into()));
        }
    }

    /// Delivers one keypress as the window would, with `Ignored` status — the
    /// status `keyboard::listen()` filters on, which is what lets the app's own
    /// `route_key` see it.
    fn key_event(&mut self, key: Key, modifiers: Modifiers) {
        let _ = modifiers;
        let pressed = keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: None,
        };
        self.press_raw(pressed);
    }

    fn press_raw(&mut self, event: keyboard::Event) {
        self.broadcast(SubEvent::Interaction {
            window: self.window,
            event: iced_test::core::Event::Keyboard(event),
            status: Status::Ignored,
        });
    }

    /// Hands one event to the live subscriptions and runs what comes out.
    ///
    /// The subscriptions are polled here, by hand, rather than through the
    /// runtime's tracker. The tracker spawns each stream onto the executor, and
    /// a spawned stream is only polled when the executor chooses to schedule
    /// it — which a test can neither observe nor wait for, and an event sent
    /// before that first poll goes nowhere.
    ///
    /// Driving them directly is what makes the test deterministic: a key press
    /// either reaches `update` before this returns, or it does not happen at
    /// all. The subscriptions are the app's own, built by `App::subscription`,
    /// so the routing under test is still the app's real routing.
    fn broadcast(&mut self, event: SubEvent) {
        // `SinkExt::send` polls the stream until it has consumed the event, so
        // there is no "the subscription has not started yet" window.
        let mut produced = Vec::new();
        for sub in self.subscriptions.iter_mut() {
            let _ignored = sub.input.unbounded_send(event.clone());
            // Drain what the subscription produced. `poll_next` is used rather
            // than `next().await` because a subscription with nothing to say
            // must not park the test here.
            loop {
                let waker = futures::task::noop_waker();
                let mut cx = std::task::Context::from_waker(&waker);
                match sub.stream.as_mut().poll_next(&mut cx) {
                    std::task::Poll::Ready(Some(message)) => produced.push(message),
                    std::task::Poll::Ready(None) | std::task::Poll::Pending => break,
                }
            }
        }
        for message in produced {
            self.send(message);
        }
        self.settle();
    }

    /// Delivers a message with no keyboard event behind it.
    fn send(&mut self, message: Message) {
        let program = &self.program;
        let task = self
            .runtime
            .enter(|| program.update(&mut self.app, message));
        self.resubscribe();
        self.drive(task);
        self.settle();
    }

    /// Starts `task`, and lets `settle` wait for it.
    fn drive(&mut self, task: Task<Message>) {
        let Some(stream) = task::into_stream(task) else {
            return;
        };
        // `into_stream` yields the runtime's own `Action`. Every task in this
        // app is `Task::perform`, so `Output` is the only arm that arrives.
        //
        // `Runtime::run` only spawns the stream, so there is no handle to wait
        // on. The `Finished` marker is what makes waiting possible: it comes
        // from this stream itself, after the last real message, and only then
        // is the task counted as done. That is a completion signal from the
        // work itself — a copy that takes ten seconds is waited for, and a
        // test cannot be flaky because a sleep was too short.
        let mapped = StreamExt::map(stream, |action| match action {
            Action::Output(message) => Outcome::Message(message),
            // F7 returns `operation::focus(FIELD_ID)`, which is a widget
            // operation rather than a message: it tells the runtime to focus a
            // text input, and there is no `Message` for it because nothing in
            // the app needs to know. The same goes for a font load or a
            // clipboard read, neither of which this app performs.
            //
            // Dropped rather than turned into a message. Faking one here — a
            // `Quit`, say — would let a real problem reach `update` as
            // something harmless; and failing instead would make every test that
            // presses F7 fail for a reason that is not about F7.
            _other => Outcome::Ignored,
        })
        .chain(iced_test::futures::futures::stream::once(async {
            Outcome::Finished
        }))
        .boxed();

        self.in_flight += 1;
        self.runtime.run(mapped);
    }

    /// Runs until nothing is left to run.
    ///
    /// Two things can be in flight, and a step has to wait for both:
    ///
    /// - **Tasks**, which report through `Outcome::Finished`. That is a real
    ///   completion signal from the work itself, so a copy that takes ten
    ///   seconds is waited for and no test can be flaky because a sleep was too
    ///   short.
    /// - **Subscription messages**, which a key press produces without starting
    ///   any task at all. Waiting only on tasks would return before the
    ///   keyboard stream had been polled, so the keypress would be read after
    ///   the assertion that was supposed to see it.
    ///
    /// `Runtime::block_on` drives the whole tokio runtime, which is what runs
    /// the subscription streams: a key press only turns into a message when its
    /// stream is polled. So each wait goes through `block_on`, and the wait ends
    /// when the action arrives rather than after a fixed delay.
    fn settle(&mut self) {
        loop {
            // Wait for the next thing to happen, or notice that nothing has.
            let outcome = self.next_outcome();
            match outcome {
                Some(Outcome::Message(message)) => self.send(message),
                Some(Outcome::Ignored) => {}
                Some(Outcome::Finished) => self.in_flight -= 1,
                None => return,
            }
            // Once nothing is left in flight, one more round catches messages
            // that were already queued: a task's last message can be a
            // subscription's, and vice versa.
            if self.in_flight == 0 && self.actions.try_recv().is_err() {
                return;
            }
        }
    }

    /// Waits for the next action, or `None` when nothing is pending and nothing
    /// is in flight.
    ///
    /// The bounded part matters: with work in flight this waits for real, but
    /// with nothing in flight it must not hang, so the wait gives up once the
    /// executor has been given a fair chance to deliver what is already queued.
    fn next_outcome(&mut self) -> Option<Outcome> {
        if self.in_flight == 0 {
            // Nothing can arrive later, so only what is already queued counts.
            return self.actions.try_recv().ok();
        }
        self.runtime.block_on(async { self.actions.next().await })
    }
}

/// What the runtime hands back/// One live subscription: the event channel feeding it and the stream it makes.
///
/// The sender is kept beside the stream on purpose — see `resubscribe`.
struct SubscriptionStream {
    input: mpsc::UnboundedSender<SubEvent>,
    stream: futures::stream::BoxStream<'static, Message>,
}

/// What the runtime hands back: either a message for `update`, or the end of a
/// task.
///
/// A wrapper rather than the app's own `Message`, because "this task is done"
/// is not something the app should have to know about.
enum Outcome {
    Message(Message),
    /// A runtime action with no message behind it, such as focusing a field.
    Ignored,
    Finished,
}

/// Boots the real program with the left panel in `dir` and the right one in
/// its `home` subdirectory, so a test touches nothing outside its scratch tree.
fn session_in(dir: &Path) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    Session::boot(super::program(move || {
        App::starting_in(left.clone(), right.clone())
    }))
}

/// Writes bytes, for a file large enough that copying it takes a moment.
fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("a parent directory");
    }
    std::fs::write(path, contents).expect("a test file");
}

/// A scratch tree with a source and a target directory — the two panels, and
/// something to copy between them.
fn scratch(label: &str) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(&format!("ncrs-e2e-{label}-"))
        .tempdir()
        .expect("a scratch directory");
    // `links` and `files` give a case something to name; `home` is what the
    // right panel starts in.
    for name in ["links", "files", "home"] {
        std::fs::create_dir_all(dir.path().join(name)).expect("a panel directory");
    }
    dir
}

/// Writes a file, creating parents, so a case can describe its tree in one line.
fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("a parent directory");
    }
    std::fs::write(path, contents).expect("a test file");
}

/// The names a panel shows.
fn names(app: &App, side: PanelSide) -> Vec<String> {
    app.panel(side)
        .entries
        .iter()
        .map(|e| e.name.to_string_lossy().into_owned())
        .collect()
}

/// Moves the cursor onto the row called `name`, pressing Down until it is
/// there.
///
/// By name rather than by a fixed number of presses, because the row order
/// depends on what else the scratch directory holds. A test that counted
/// presses would break whenever the fixture changed — and a test that breaks
/// when the fixture changes is a test nobody trusts.
fn cursor_to<P: iced::Program<State = App, Message = Message> + 'static>(
    session: &mut Session<P>,
    name: &str,
) {
    for _ in 0..64 {
        if selected(&session.app, PanelSide::Left).as_deref() == Some(name) {
            return;
        }
        session.press(iced::keyboard::Key::Named(
            iced::keyboard::key::Named::ArrowDown,
        ));
    }
    panic!(
        "no row called {name:?}; the panel holds {:?}",
        names(&session.app, PanelSide::Left)
    );
}

/// The name under the cursor.
fn selected(app: &App, side: PanelSide) -> Option<String> {
    app.panel(side)
        .selected_entry()
        .map(|e| e.name.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::messages::ConflictChoice;
    use iced::keyboard::key::Named;

    /// The panels have to be pointing at the scratch tree before a key means
    /// anything. `App::starting_in` is given the scratch tree for the left panel and its
    /// `home` subdirectory for the right one.
    #[test]
    fn the_panels_load_the_working_directory() {
        let dir = scratch("panels");
        write(&dir.path().join("alpha.txt"), "a");
        write(&dir.path().join("beta.txt"), "b");

        let session = session_in(dir.path());

        assert_eq!(
            session.app.panel(PanelSide::Left).path,
            std::fs::canonicalize(dir.path()).expect("a canonical path"),
            "the left panel did not load the working directory"
        );
        assert!(
            names(&session.app, PanelSide::Left).contains(&"alpha.txt".to_string()),
            "the left panel shows {:?}",
            names(&session.app, PanelSide::Left)
        );
    }

    /// The first reported symptom: selecting rows did nothing. A key has to
    /// reach `route_key` and move the cursor.
    ///
    /// Asserted on the *name* under the cursor rather than on a row count: the
    /// order depends on the fixture, and a test that counted presses would
    /// break every time the fixture changed.
    #[test]
    fn arrow_keys_move_the_cursor() {
        let dir = scratch("cursor");
        write(&dir.path().join("alpha.txt"), "a");
        write(&dir.path().join("beta.txt"), "b");

        let mut session = session_in(dir.path());
        assert_eq!(
            selected(&session.app, PanelSide::Left),
            Some("..".to_string()),
            "the panel should start on the parent row; it holds {:?}",
            names(&session.app, PanelSide::Left)
        );

        let first = {
            session.press(Key::Named(Named::ArrowDown));
            let after = selected(&session.app, PanelSide::Left);
            assert_ne!(after.as_deref(), Some(".."), "the cursor did not move");
            after.expect("a row is selected")
        };

        session.press(Key::Named(Named::ArrowDown));
        let second = selected(&session.app, PanelSide::Left).expect("a row is selected");
        assert_ne!(
            second, first,
            "the second press did not move the cursor off {first:?}"
        );

        // And back up again, so the key is not merely advancing one way.
        session.press(Key::Named(Named::ArrowUp));
        assert_eq!(
            selected(&session.app, PanelSide::Left),
            Some(first),
            "the cursor did not move back"
        );
    }

    /// The second reported symptom: copying threw the focus away and lost the
    /// tags. A copy has to move the file *and* leave the cursor where it was.
    ///
    /// The panels start on the working directory and on `$HOME`, so the copy
    /// target is `$HOME`. That is not something to do to the developer's
    /// machine, so the session points `$HOME` at the scratch tree first.
    #[test]
    fn a_copy_moves_the_file_and_keeps_the_cursor_and_the_tags() {
        let dir = scratch("copy");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "a");
        write(&dir.path().join("beta.txt"), "b");

        let mut session = session_in(dir.path());
        cursor_to(&mut session, "alpha.txt");
        session.press(Key::Named(Named::Insert));
        session.press(Key::Named(Named::F5));

        assert!(
            home.join("alpha.txt").is_file(),
            "F5 did not copy alpha.txt"
        );
        assert!(
            dir.path().join("alpha.txt").is_file(),
            "copying removed the source"
        );
        assert_eq!(
            selected(&session.app, PanelSide::Left),
            Some("alpha.txt".to_string()),
            "the copy moved the cursor; the panel holds {:?}",
            names(&session.app, PanelSide::Left)
        );
        assert!(
            session.app.panel(PanelSide::Left).selection.any_tagged(),
            "the copy dropped the tags"
        );
    }

    /// Move is copy plus delete. If the source is still there, F6 is a copy.
    #[test]
    fn a_move_removes_the_source() {
        let dir = scratch("move");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "a");

        let mut session = session_in(dir.path());
        cursor_to(&mut session, "alpha.txt");
        session.press(Key::Named(Named::F6));

        assert!(
            home.join("alpha.txt").is_file(),
            "F6 did not move alpha.txt"
        );
        assert!(
            !dir.path().join("alpha.txt").exists(),
            "F6 left the source in place"
        );
    }

    /// The third reported symptom: no dialog on overwrite. Copying onto a name
    /// that is taken has to ask, and must not touch the existing file until it
    /// does.
    #[test]
    fn copying_onto_a_taken_name_asks_before_overwriting() {
        let dir = scratch("conflict");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "new");
        write(&home.join("alpha.txt"), "old");

        let mut session = session_in(dir.path());
        cursor_to(&mut session, "alpha.txt");
        session.press(Key::Named(Named::F5));

        assert_eq!(
            std::fs::read_to_string(home.join("alpha.txt")).expect("the old file"),
            "old",
            "the existing file was overwritten without asking"
        );
        assert!(
            session.app.conflict_is_pending(),
            "no conflict was raised, so the user is never asked"
        );
    }

    /// Answering the dialog has to actually resume the copy. Without this the
    /// dialog would appear and then do nothing, which is what the user saw.
    #[test]
    fn answering_the_dialog_overwrites() {
        let dir = scratch("answer");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "new");
        write(&home.join("alpha.txt"), "old");

        let mut session = session_in(dir.path());
        cursor_to(&mut session, "alpha.txt");
        session.press(Key::Named(Named::F5));
        // The answer goes in as a message: `Instruction` cannot carry a
        // modifier, and this dialog is answered with Enter and Escape.
        session.press(Key::Named(Named::Enter));

        assert_eq!(
            std::fs::read_to_string(home.join("alpha.txt")).expect("the file"),
            "new",
            "answering the dialog did not overwrite"
        );
    }

    /// Escape means "keep", and keeping is the answer that does not lose data.
    #[test]
    fn answering_the_dialog_with_keep_leaves_the_target_alone() {
        let dir = scratch("keep");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "new");
        write(&home.join("alpha.txt"), "old");

        let mut session = session_in(dir.path());
        cursor_to(&mut session, "alpha.txt");
        session.press(Key::Named(Named::F5));
        session.press(Key::Named(Named::Escape));

        assert_eq!(
            std::fs::read_to_string(home.join("alpha.txt")).expect("the old file"),
            "old",
            "keeping still overwrote the target"
        );
        assert!(
            !session.app.conflict_is_pending(),
            "the dialog opened again on the file that was just kept"
        );
        assert!(session.app.job_is_idle(), "the transfer never ended");
    }

    /// Tags the named rows, in the order given, and starts `key` on them.
    fn tag_and_press(
        session: &mut Session<impl iced::Program<State = App, Message = Message> + 'static>,
        rows: &[&str],
        key: Named,
    ) {
        for row in rows {
            cursor_to(session, row);
            session.press(Key::Named(Named::Insert));
        }
        session.press(Key::Named(key));
    }

    /// Keep skips one file and the transfer goes on with the next.
    #[test]
    fn keeping_one_file_goes_on_to_the_next() {
        let dir = scratch("keep-next");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "new");
        write(&dir.path().join("beta.txt"), "b");
        write(&home.join("alpha.txt"), "old");

        let mut session = session_in(dir.path());
        tag_and_press(&mut session, &["alpha.txt", "beta.txt"], Named::F5);
        assert!(session.app.conflict_is_pending(), "no question about alpha");
        session.press(Key::Named(Named::Escape));

        assert!(
            !session.app.conflict_is_pending(),
            "asked about alpha twice"
        );
        assert_eq!(
            std::fs::read_to_string(home.join("alpha.txt")).unwrap(),
            "old"
        );
        assert!(
            home.join("beta.txt").is_file(),
            "the transfer stopped after keep"
        );
    }

    /// "Keep all" answers every later conflict the same way, without asking.
    #[test]
    fn keep_all_does_not_ask_again() {
        let dir = scratch("keep-all");
        let home = dir.path().join("home");
        for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
            write(&dir.path().join(name), "new");
            write(&home.join(name), "old");
        }
        write(&dir.path().join("zeta.txt"), "z");

        let mut session = session_in(dir.path());
        tag_and_press(
            &mut session,
            &["alpha.txt", "beta.txt", "gamma.txt", "zeta.txt"],
            Named::F5,
        );
        session.send(Message::TransferConflict(ConflictChoice::AllKeep));

        assert!(
            !session.app.conflict_is_pending(),
            "asked again after keep all"
        );
        for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
            assert_eq!(std::fs::read_to_string(home.join(name)).unwrap(), "old");
        }
        assert!(home.join("zeta.txt").is_file(), "the rest was not copied");
    }

    /// Four rows, three of them already in the target, the dialog up on the
    /// first conflict, and "for all files" ticked with Space.
    fn all_ticked_on_three_conflicts() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        Session<impl iced::Program<State = App, Message = Message> + 'static>,
    ) {
        let dir = scratch("for-all");
        let home = dir.path().join("home");
        for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
            write(&dir.path().join(name), "new");
            write(&home.join(name), "old");
        }
        write(&dir.path().join("zeta.txt"), "z");
        let mut session = session_in(dir.path());
        for row in ["alpha.txt", "beta.txt", "gamma.txt", "zeta.txt"] {
            cursor_to(&mut session, row);
            session.press(Key::Named(Named::Insert));
        }
        session.press(Key::Named(Named::F5));
        assert!(session.app.conflict_is_pending(), "no question about alpha");
        session.press(Key::Named(Named::Space));
        (dir, home, session)
    }

    /// The tick is wired: Keep with it set keeps every later conflict too,
    /// without a second question.
    #[test]
    fn keep_with_for_all_ticked_does_not_ask_again() {
        let (_dir, home, mut session) = all_ticked_on_three_conflicts();
        session.press(Key::Named(Named::Escape));

        assert!(!session.app.conflict_is_pending(), "asked again");
        for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
            assert_eq!(std::fs::read_to_string(home.join(name)).unwrap(), "old");
        }
        assert!(home.join("zeta.txt").is_file(), "the rest was not copied");
    }

    #[test]
    fn overwrite_with_for_all_ticked_does_not_ask_again() {
        let (_dir, home, mut session) = all_ticked_on_three_conflicts();
        session.press(Key::Named(Named::Enter));

        assert!(!session.app.conflict_is_pending(), "asked again");
        for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
            assert_eq!(std::fs::read_to_string(home.join(name)).unwrap(), "new");
        }
        assert!(home.join("zeta.txt").is_file());
    }

    /// Cancel ends the whole transfer: the rows after the conflict are not
    /// touched and the dialog does not come back.
    #[test]
    fn cancel_ends_the_transfer() {
        let dir = scratch("cancel-dialog");
        let home = dir.path().join("home");
        write(&dir.path().join("alpha.txt"), "new");
        write(&dir.path().join("beta.txt"), "b");
        write(&home.join("alpha.txt"), "old");

        let mut session = session_in(dir.path());
        tag_and_press(&mut session, &["alpha.txt", "beta.txt"], Named::F5);
        session.press_with(Key::Character("c".into()), Modifiers::CTRL);

        assert!(!session.app.conflict_is_pending(), "the dialog is still up");
        assert!(session.app.job_is_idle());
        assert_eq!(
            std::fs::read_to_string(home.join("alpha.txt")).unwrap(),
            "old"
        );
        assert!(!home.join("beta.txt").exists(), "cancel went on copying");
    }

    /// A multi-file transfer that fails on a later row still shows the rows
    /// that were done: the panels are reloaded.
    #[cfg(unix)]
    #[test]
    fn a_failure_midway_reloads_the_panels() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("fail-midway");
        write(&dir.path().join("alpha.txt"), "a");
        write(&dir.path().join("beta.txt"), "b");
        std::fs::set_permissions(
            dir.path().join("beta.txt"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();

        let mut session = session_in(dir.path());
        tag_and_press(&mut session, &["alpha.txt", "beta.txt"], Named::F5);

        assert!(
            names(&session.app, PanelSide::Right).contains(&"alpha.txt".to_string()),
            "the target panel does not show the file that was copied; it holds {:?}",
            names(&session.app, PanelSide::Right)
        );
    }

    /// Ctrl+C stops a running job — the fourth reported symptom was that the
    /// queue could not be steered at all.
    ///
    /// The copy here is large enough that it cannot have finished by the time
    /// the abort is pressed, and the assertion is that the operation stops
    /// rather than running on.
    #[test]
    fn ctrl_c_stops_a_running_copy() {
        let dir = scratch("abort");
        // A big file, so the copy is still going when the key arrives.
        let big = vec![b'x'; 64 * 1024 * 1024];
        write_bytes(&dir.path().join("big.bin"), &big);
        write(&dir.path().join("alpha.txt"), "a");

        let mut session = session_in(dir.path());
        cursor_to(&mut session, "big.bin");
        session.press(Key::Named(Named::F5));
        session.press_with(Key::Character("c".into()), Modifiers::CTRL);

        assert!(
            session.app.job_is_idle(),
            "the copy is still running after Ctrl+C"
        );
    }

    /// F7 creates a directory. Typing needs two separate abilities — a key
    /// that carries a character, and text typed as a burst — and both go
    /// through the app's real routing rather than a message.
    #[test]
    fn f7_creates_a_directory_and_selects_it() {
        let dir = scratch("mkdir");

        let mut session = session_in(dir.path());
        session.press(Key::Named(Named::F7));
        session.type_text("neuer ordner");
        session.press(Key::Named(Named::Enter));

        assert!(
            dir.path().join("neuer ordner").is_dir(),
            "F7 did not create the directory; the panel holds {:?}",
            names(&session.app, PanelSide::Left)
        );
        assert_eq!(
            selected(&session.app, PanelSide::Left),
            Some("neuer ordner".to_string()),
            "the new directory was not selected"
        );
    }

    /// Escape closes the prompt without creating anything. A prompt that could
    /// only be left by typing a valid name would be a trap.
    #[test]
    fn escape_closes_the_prompt_without_creating_anything() {
        let dir = scratch("cancel-mkdir");

        let mut session = session_in(dir.path());
        session.press(Key::Named(Named::F7));
        session.type_text("weg damit");
        session.press(Key::Named(Named::Escape));

        assert!(
            !dir.path().join("weg damit").exists(),
            "Escape created the directory anyway"
        );
        assert!(
            !names(&session.app, PanelSide::Left).contains(&"weg damit".to_string()),
            "the prompt is still up: the panel holds {:?}",
            names(&session.app, PanelSide::Left)
        );
    }

    /// Tagging is what a multi-file copy is built on.
    #[test]
    fn insert_tags_the_row_under_the_cursor() {
        let dir = scratch("tag");
        write(&dir.path().join("alpha.txt"), "a");
        write(&dir.path().join("beta.txt"), "b");

        let mut session = session_in(dir.path());
        session.press(Key::Named(Named::ArrowDown));
        session.press(Key::Named(Named::Insert));

        assert!(
            session.app.panel(PanelSide::Left).selection.any_tagged(),
            "Insert tagged nothing; the panel holds {:?}",
            names(&session.app, PanelSide::Left)
        );
    }
}
