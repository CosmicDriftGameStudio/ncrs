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
//! F5, F6, F7, Insert, ArrowDown and every chord are inexpressible, Ctrl+C
//! among them, which stops a job on a Mac next to Escape. That is not a detail
//! of test setup; it is why the entire copy / move / queue surface had no
//! end-to-end coverage, and so why four real bugs shipped.
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

use std::path::Path;

use iced::keyboard::{self, Key, Modifiers};
use iced::window;
use iced_test::core::event::Status;
use iced_test::core::renderer::Headless as _;
use iced_test::core::widget::operation::Outcome as OperationOutcome;
use iced_test::core::{mouse, Point, Size};
use iced_test::futures::futures::channel::mpsc;
use iced_test::futures::futures::{self as futures, StreamExt};
use iced_test::futures::subscription::{self, Event as SubEvent};
use iced_test::futures::Runtime;
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_test::runtime::{task, Action, Task};

use crate::app::App;
use crate::messages::{Message, PanelMessage, PanelSide};

/// One end-to-end session: the app's own state, subscriptions and task channel.
///
/// `Runtime` owns the subscription tracker and spawns what the subscriptions
/// produce, so it has to outlive every step. It runs on the tokio executor the
/// app itself uses, which is what makes a `spawn_blocking` copy run for real
/// rather than being simulated.
pub struct Session<P: iced::Program> {
    pub app: App,
    /// The program under test — the one `main` builds, so a test cannot pass
    /// while the binary is wired differently.
    pub program: P,
    pub runtime: Runtime<iced::executor::Default, mpsc::UnboundedSender<Outcome>, Outcome>,
    /// What comes back from tasks and subscriptions.
    pub actions: mpsc::UnboundedReceiver<Outcome>,
    /// How many tasks have been started and not yet reported finished.
    pub in_flight: usize,
    /// The app's subscriptions, held open so a test can poll them directly.
    ///
    /// Each is the app's own `Subscription`, built by `App::subscription` and
    /// run with an event channel this harness controls — see `broadcast`.
    pub subscriptions: Vec<SubscriptionStream>,
    /// The window the app believes it is in. Subscriptions carry it, so a wrong
    /// id would silently drop every key.
    pub window: window::Id,
    /// What the widgets see: the real view, laid out and drawn headless, so a
    /// click reaches the widget under it and a field can really be focused.
    pub widgets: Widgets<P::Renderer>,
}

/// The widget side of a session. Widget state, such as which text field has the
/// focus, lives in the cache between interactions, as it does in the window.
pub struct Widgets<R> {
    pub renderer: R,
    pub cache: Option<user_interface::Cache>,
    pub cursor: mouse::Cursor,
    pub clipboard: TestClipboard,
}

/// The size the view is laid out in.
const WINDOW: Size = Size::new(1200.0, 760.0);

#[derive(Default)]
pub struct TestClipboard {
    pub content: Option<String>,
}

impl iced_test::core::Clipboard for TestClipboard {
    fn read(&self, _kind: iced_test::core::clipboard::Kind) -> Option<String> {
        self.content.clone()
    }

    fn write(&mut self, _kind: iced_test::core::clipboard::Kind, contents: String) {
        self.content = Some(contents);
    }
}

impl<P: iced::Program<State = App, Message = Message> + 'static> Session<P> {
    /// Boots the app and runs the initial directory loads.
    pub fn boot(program: P) -> Self {
        let executor = iced::executor::Default::new().expect("the tokio executor");
        let (action_tx, actions) = mpsc::unbounded();
        let mut runtime = Runtime::new(executor, action_tx);

        let (app, boot) = program.boot();
        let settings = program.settings();
        for font in settings.fonts {
            iced_test::renderer::graphics::text::font_system()
                .write()
                .expect("the font system")
                .load_font(font);
        }
        let renderer = runtime
            .block_on(async {
                P::Renderer::new(settings.default_font, settings.default_text_size, None).await
            })
            .expect("a headless renderer");
        let mut session = Self {
            widgets: Widgets {
                renderer,
                cache: Some(user_interface::Cache::default()),
                cursor: mouse::Cursor::Unavailable,
                clipboard: TestClipboard::default(),
            },
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
        // A timer recipe (`time::every`) needs the tokio context to be built.
        let runtime = &self.runtime;
        self.subscriptions = recipes
            .into_iter()
            .map(|recipe| {
                let (tx, rx) = mpsc::unbounded::<SubEvent>();
                // The sender has to outlive the stream: the stream's input ends
                // when every sender is dropped, and a closed input makes
                // `keyboard::listen` yield nothing ever again.
                SubscriptionStream {
                    input: tx,
                    stream: runtime.enter(|| recipe.stream(rx.boxed())),
                }
            })
            .collect();
    }

    /// Runs a widget operation on the current view, as the runtime does after
    /// the update that asked for it.
    fn operate(&mut self, operation: Box<dyn iced_test::core::widget::Operation>) {
        let mut interface = UserInterface::build(
            self.program.view(&self.app, self.window),
            WINDOW,
            self.widgets.cache.take().unwrap_or_default(),
            &mut self.widgets.renderer,
        );
        let mut next = Some(operation);
        while let Some(mut current) = next.take() {
            interface.operate(&self.widgets.renderer, current.as_mut());
            if let OperationOutcome::Chain(chained) = current.finish() {
                next = Some(chained);
            }
        }
        self.widgets.cache = Some(interface.into_cache());
    }

    /// Delivers events to the view's widgets, then to the subscriptions with the
    /// status the widgets gave them, then applies what the widgets published:
    /// the order of the real event loop.
    fn interact(&mut self, events: Vec<iced_test::core::Event>) {
        for event in &events {
            if let iced_test::core::Event::Mouse(mouse::Event::CursorMoved { position }) = event {
                self.widgets.cursor = mouse::Cursor::Available(*position);
            }
        }
        let mut interface = UserInterface::build(
            self.program.view(&self.app, self.window),
            WINDOW,
            self.widgets.cache.take().unwrap_or_default(),
            &mut self.widgets.renderer,
        );
        let mut messages = Vec::new();
        let (_state, statuses) = interface.update(
            &events,
            self.widgets.cursor,
            &mut self.widgets.renderer,
            &mut self.widgets.clipboard,
            &mut messages,
        );
        self.widgets.cache = Some(interface.into_cache());
        for (event, status) in events.into_iter().zip(statuses) {
            self.broadcast(SubEvent::Interaction {
                window: self.window,
                event,
                status,
            });
        }
        for message in messages {
            self.send(message);
        }
    }

    /// A left click at `point`.
    fn click_at(&mut self, point: Point) {
        use iced_test::core::Event;
        self.interact(vec![
            Event::Mouse(mouse::Event::CursorMoved { position: point }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ]);
        // A press that arms a drag is applied before the release arrives, as
        // in the real loop; a column title sorts on the release.
        self.interact(vec![Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))]);
    }

    /// Types `text` the way the keyboard does: a key press with its text.
    fn type_into_widgets(&mut self, text: &str) {
        self.interact(iced_test::simulator::typewrite(text).collect());
    }

    /// One key press with `modifiers`, to the widgets first.
    fn press_in_widgets(&mut self, key: Key, modifiers: Modifiers) {
        self.interact(vec![iced_test::core::Event::Keyboard(
            keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Unidentified(
                    keyboard::key::NativeCode::Unidentified,
                ),
                location: keyboard::Location::Standard,
                modifiers,
                repeat: false,
                text: None,
            },
        )]);
    }

    /// Presses and releases one key, then runs everything that follows.
    pub fn press(&mut self, key: Key) {
        self.key_event(key, Modifiers::default());
    }

    /// Presses a key while `modifiers` are held.
    fn press_with(&mut self, key: Key, modifiers: Modifiers) {
        self.key_event(key, modifiers);
    }

    pub fn type_text(&mut self, text: &str) {
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

    /// A key held down: the same press again with `repeat` set, as the
    /// operating system sends it.
    fn press_repeat(&mut self, key: Key, modifiers: Modifiers) {
        self.press_raw(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers,
            repeat: true,
            text: None,
        });
    }

    pub fn press_raw(&mut self, event: keyboard::Event) {
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
        let runtime = &self.runtime;
        for sub in self.subscriptions.iter_mut() {
            let _ignored = sub.input.unbounded_send(event.clone());
            // Drain what the subscription produced. `poll_next` is used rather
            // than `next().await` because a subscription with nothing to say
            // must not park the test here. Inside the tokio context: the folder
            // watch sets a tokio timer once the system reports a change.
            runtime.enter(|| loop {
                let waker = futures::task::noop_waker();
                let mut cx = std::task::Context::from_waker(&waker);
                match sub.stream.as_mut().poll_next(&mut cx) {
                    std::task::Poll::Ready(Some(message)) => produced.push(message),
                    std::task::Poll::Ready(None) | std::task::Poll::Pending => break,
                }
            });
        }
        for message in produced {
            self.send(message);
        }
        self.settle();
    }

    /// Runs `update` for `message` but holds back the task it returns, so the
    /// work it would start has not begun. Lets a test act while a job is
    /// certainly still in flight.
    pub fn send_held(&mut self, message: Message) -> Task<Message> {
        let program = &self.program;
        let task = self
            .runtime
            .enter(|| program.update(&mut self.app, message));
        self.resubscribe();
        task
    }

    /// As [`Self::send_held`] for a confirmation whose check runs off the UI
    /// thread: waits for the check, delivers its answer and holds back the task
    /// that answer returns, the job.
    pub fn send_held_after_check(&mut self, message: Message) -> Task<Message> {
        let Some(mut stream) = task::into_stream(self.send_held(message)) else {
            return Task::none();
        };
        let answer = self.runtime.block_on(async move {
            while let Some(action) = stream.next().await {
                if let Action::Output(reported @ Message::Ops(_)) = action {
                    return Some(reported);
                }
            }
            None
        });
        answer.map_or_else(Task::none, |reported| self.send_held(reported))
    }

    /// Delivers a message with no keyboard event behind it.
    pub fn send(&mut self, message: Message) {
        let program = &self.program;
        let task = self
            .runtime
            .enter(|| program.update(&mut self.app, message));
        self.resubscribe();
        self.drive(task);
        self.settle();
    }

    /// Starts `task`, and lets `settle` wait for it.
    pub fn drive(&mut self, task: Task<Message>) {
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
            Action::Widget(operation) => Outcome::Widget(operation),
            Action::Clipboard(iced_test::runtime::clipboard::Action::Write {
                target: iced_test::core::clipboard::Kind::Standard,
                contents,
            }) => Outcome::ClipboardWrite(contents),
            // Widget operations, such as focusing a text field, run on the
            // widgets below. A font load or a clipboard read, which this app
            // does not perform, are dropped rather than turned into a
            // message: a faked one could reach `update` as something harmless.
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
    pub fn settle(&mut self) {
        loop {
            // Wait for the next thing to happen, or notice that nothing has.
            let outcome = self.next_outcome();
            match outcome {
                Some(Outcome::Message(message)) => self.send(message),
                Some(Outcome::Widget(operation)) => self.operate(operation),
                Some(Outcome::ClipboardWrite(contents)) => {
                    self.widgets.clipboard.content = Some(contents);
                }
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
pub struct SubscriptionStream {
    pub input: mpsc::UnboundedSender<SubEvent>,
    pub stream: futures::stream::BoxStream<'static, Message>,
}

/// What the runtime hands back: either a message for `update`, or the end of a
/// task.
///
/// A wrapper rather than the app's own `Message`, because "this task is done"
/// is not something the app should have to know about.
pub enum Outcome {
    Message(Message),
    /// A widget operation, such as focusing a field.
    Widget(Box<dyn iced_test::core::widget::Operation>),
    /// Text the app put on the system clipboard.
    ClipboardWrite(String),
    /// A runtime action with no message behind it.
    Ignored,
    Finished,
}

/// Boots the real program with the left panel in `dir` and the right one in
/// its `home` subdirectory, so a test touches nothing outside its scratch tree.
fn session_in(dir: &Path) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    Session::boot(super::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || App::starting_in(left.clone(), right.clone()),
    ))
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

/// Like `session_in`, with a trash of the test's own. The system trash is the
/// developer's real one, and no test may put anything in it.
fn session_with_trash(
    dir: &Path,
    trash: std::sync::Arc<dyn crate::fs::Trash>,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    Session::boot(super::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left.clone(), right.clone());
            (app.with_trash(std::sync::Arc::clone(&trash)), task)
        },
    ))
}

/// A trash that is a folder: an entry "in the trash" is one that moved there.
struct FolderTrash(std::path::PathBuf);

impl crate::fs::Trash for FolderTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        let name = path.file_name().ok_or("no file name")?;
        std::fs::rename(path, self.0.join(name)).map_err(|err| err.to_string())
    }

    fn reach(&self) -> crate::fs::TrashReach {
        crate::fs::TrashReach::Everything
    }

    fn list(&self) -> Result<Vec<crate::fs::TrashedItem>, String> {
        Ok(Vec::new())
    }

    fn restore(&self, _item: &crate::fs::TrashedItem) -> Result<(), crate::fs::RestoreError> {
        Err(crate::fs::RestoreError::Failed(
            "not in this fake".to_string(),
        ))
    }
}

/// A trash that refuses everything, like a volume without one.
struct RefusingTrash;

impl crate::fs::Trash for RefusingTrash {
    fn trash(&self, _path: &Path) -> Result<(), String> {
        Err("no trash on this volume".to_string())
    }

    fn reach(&self) -> crate::fs::TrashReach {
        crate::fs::TrashReach::FinderOnly
    }

    fn list(&self) -> Result<Vec<crate::fs::TrashedItem>, String> {
        Ok(Vec::new())
    }

    fn restore(&self, _item: &crate::fs::TrashedItem) -> Result<(), crate::fs::RestoreError> {
        Err(crate::fs::RestoreError::Failed(
            "no trash on this volume".to_string(),
        ))
    }
}

/// Like `session_in`, offering `Scratch`, `Files` and `Links` of the scratch
/// tree as the drives. The right panel starts in `home`, which is on `Scratch`.
fn session_with_drives(
    dir: &Path,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    Session::boot(super::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left.clone(), right.clone());
            let root = left.clone();
            let with_drives = app.with_volumes(move || {
                ["", "files", "links"]
                    .iter()
                    .map(|name| crate::fs::Volume {
                        name: if name.is_empty() { "Scratch" } else { name }.to_string(),
                        path: root.join(name),
                    })
                    .collect()
            });
            (with_drives, task)
        },
    ))
}

/// Like `session_in`, with a launcher of the test's own: no test may start a
/// real viewer.
fn session_with_launcher(
    dir: &Path,
    launcher: std::sync::Arc<dyn crate::fs::Launcher>,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    session_with_config(dir, || Ok(crate::config::Config::default()), launcher)
}

/// Like `session_with_launcher`, booting with the config `load` returns, the
/// way `App::new` applies a loaded file.
fn session_with_config(
    dir: &Path,
    load: impl Fn() -> Result<crate::config::Config, crate::config::ConfigError> + 'static,
    launcher: std::sync::Arc<dyn crate::fs::Launcher>,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    Session::boot(super::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left.clone(), right.clone());
            let configured = app
                .with_config(load())
                .with_launcher(std::sync::Arc::clone(&launcher));
            (configured, task)
        },
    ))
}

/// Like `session_with_launcher`, unpacking the files F3 views into `cache`.
fn session_with_view_copies(
    dir: &Path,
    cache: &Path,
    launcher: std::sync::Arc<dyn crate::fs::Launcher>,
) -> Session<impl iced::Program<State = App, Message = Message> + 'static> {
    let left = std::fs::canonicalize(dir).expect("a canonical scratch path");
    let right = left.join("home");
    let view_root = cache.to_path_buf();
    Session::boot(super::program(
        crate::ui::layout::INITIAL_WINDOW_SIZE,
        move || {
            let (app, task) = App::starting_in(left.clone(), right.clone());
            let configured = app
                .with_config(Ok(crate::config::Config::default()))
                .with_launcher(std::sync::Arc::clone(&launcher))
                .with_view_copies_in(&view_root);
            (configured, task)
        },
    ))
}

/// A launcher that only writes down what it was asked to start.
#[derive(Default)]
struct RecordingLauncher(std::sync::Mutex<Vec<crate::fs::OpenCommand>>);

impl RecordingLauncher {
    fn calls(&self) -> Vec<crate::fs::OpenCommand> {
        self.0.lock().expect("the call log").clone()
    }
}

impl crate::fs::Launcher for RecordingLauncher {
    fn launch(&self, command: &crate::fs::OpenCommand) -> Result<(), String> {
        let mut log = self.0.lock().map_err(|err| err.to_string())?;
        log.push(command.clone());
        Ok(())
    }
}

/// A launcher whose program is missing.
#[derive(Debug)]
struct MissingProgramLauncher;

impl crate::fs::Launcher for MissingProgramLauncher {
    fn launch(&self, _command: &crate::fs::OpenCommand) -> Result<(), String> {
        Err("xdg-open: No such file or directory".to_string())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod path_and_sort;
#[cfg(test)]
use path_and_sort::focus_the_window;

#[cfg(test)]
mod acceptance;

#[cfg(test)]
mod process_again;

mod folder_merge;
#[cfg(test)]
mod notes;
mod notes_move;
mod viewer;

#[cfg(test)]
mod acceptance_v150;

#[cfg(test)]
mod favourites;

#[cfg(test)]
mod fonts;

#[cfg(test)]
mod monitor;

#[cfg(test)]
mod search_view;

#[cfg(test)]
mod quick_open;

mod archive_browsing;

mod archive_writing;

mod archive_viewing;
