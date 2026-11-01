use crate::app::*;

#[cfg(target_os = "macos")]
fn native_menu_events() -> Subscription<Message> {
    native_menu::events()
}

#[cfg(not(target_os = "macos"))]
fn native_menu_events() -> Subscription<Message> {
    Subscription::none()
}

/// Turns the transfer's channel into a stream `Subscription::run_with` can use.
///
/// tokio's Mutex rather than std's, because the guard is held across an await.
/// There is a single consumer, so the lock is a way of moving the receiver
/// rather than of sharing it. `recv().await` registers the waker with the
/// channel, so an empty channel costs nothing until a tick is sent; waking
/// the task by hand on empty would spin it for as long as the sender lives,
/// and the app keeps one.
pub(in crate::app) fn receiver_stream(
    shared: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>,
) -> impl futures::Stream<Item = fs::transfer::Tick> + 'static {
    futures::stream::unfold(shared, |channel| async move {
        let tick = channel.lock().await.recv().await?;
        Some((tick, channel))
    })
}

impl App {
    pub fn subscription(&self) -> Subscription<Message> {
        // `listen` yields the event stream, so the mapping can decide
        // without routing every key through a message first.
        //
        // The routing state is read through a shared cell, deliberately *not*
        // carried in the stream by `with`. `with` folds its value into the
        // subscription's hash, and iced's tracker drops any subscription whose
        // hash changes: every keystroke that opened a prompt, started a job or
        // raised a conflict killed the keyboard subscription and spawned a
        // fresh one, so the keys pressed around that moment went to a stream
        // nobody was listening to. That is why the app looked alive and then
        // ignored the keyboard, and why no test caught it — the routing itself
        // was correct.
        //
        // A cell instead: the hash stays constant, so this subscription lives
        // as long as the window does, and it reads the current state on every
        // key rather than a copy from when it was built.
        let key_state = Arc::clone(
            self.key_state
                .get_or_init(|| Arc::new(KeyStateCell(Mutex::new(self.prompt_key_state())))),
        );

        Subscription::batch([
            // `with` is unavoidable: iced requires the `filter_map` closure to
            // capture nothing, so the cell has to travel as the subscription's
            // value. What matters is *which* value. The old code carried the
            // `PromptKeyState` itself, so the hash tracked the state and
            // changed on every message; iced's tracker drops a subscription
            // whose hash changes, which killed the keyboard stream at exactly
            // the moments the app got busy — the keys pressed right after
            // opening a prompt or starting a copy went nowhere. That is why
            // the app looked alive and then ignored the keyboard.
            //
            // Carrying the cell instead gives a hash that is fixed for the
            // life of the app: the same `Arc`, hashed by its contents, which
            // never change. The stream survives every state change and reads
            // the current state on each key.
            keyboard::listen()
                .with(key_state)
                .filter_map(|(cell, event)| {
                    match event {
                        keyboard::Event::KeyPressed {
                            key,
                            physical_key,
                            modifiers,
                            repeat,
                            ..
                        } => route_press(
                            // A panic in a key handler would leave the lock
                            // poisoned. The state is a few Copy values and cannot
                            // be half-written, so reading the inner value is safe —
                            // and it keeps one panic from silencing every key
                            // afterwards.
                            &cell
                                .0
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                            key,
                            physical_key,
                            modifiers,
                            repeat,
                        ),
                        keyboard::Event::ModifiersChanged(modifiers) => {
                            Some(Message::ModifiersChanged(modifiers))
                        }
                        _ => None,
                    }
                }),
            // Keeps the status bar's progress and the open page current.
            if self.hermes.pipeline.is_some()
                || self.hermes.filing_status.page_open()
                || self.hermes.overview.is_some()
            {
                iced::time::every(SNAPSHOT_EVERY)
                    .map(|_| Message::FilingStatus(FilingStatusMessage::Tick))
            } else {
                Subscription::none()
            },
            // The bar of a running model download reads a counter the download
            // thread fills; nothing else wakes the view.
            if self.hermes_flow_is_visible() {
                iced::time::every(hermes_flow::STEP_INTERVAL).map(|_| Message::HermesFlowTick)
            } else {
                Subscription::none()
            },
            if self.settings.ai_model_download_runs() {
                iced::time::every(DOWNLOAD_BAR_EVERY)
                    .map(|_| Message::AiModel(AiModelMessage::Progress))
            } else {
                Subscription::none()
            },
            if self.settings.ocr_models_download_runs() {
                iced::time::every(DOWNLOAD_BAR_EVERY)
                    .map(|_| Message::OcrModels(OcrModelsMessage::Progress))
            } else {
                Subscription::none()
            },
            // Mouse, focus and modifier events of every status: widgets capture
            // the clicks and the pointer moves the menu has to know about.
            event::listen_with(|event, status, _window| Some((event, status)))
                .with(Arc::clone(&self.input))
                .filter_map(|(input, (event, status))| input.lock().handle(&event, status)),
            window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
            window::events().filter_map(|(_id, event)| match event {
                window::Event::Focused => Some(Message::WindowFocused),
                window::Event::FileHovered(_) => Some(Message::FilesHovered),
                window::Event::FilesHoveredLeft => Some(Message::FilesHoverLeft),
                window::Event::FileDropped(path) => Some(Message::FileDropped(path)),
                _ => None,
            }),
            // The close button and Alt+F4 end the app through Quit, which saves
            // the session first; the window does not close by itself.
            window::close_requests().map(|_id| Message::Quit),
            native_menu_events(),
            // The transfer's ticks. Taken out of the app because a subscription
            // outlives `&self`; the receiver is taken so the subscription
            // rebuilds itself when a new transfer starts.
            // The transfer's ticks. The receiver lives in an Arc because a
            // mpsc Receiver is neither Clone nor Hash, and run_with needs the
            // data to be both: the Arc for Clone, the newtype for Hash.
            match &self.progress_rx {
                None => Subscription::none(),
                Some(shared) => {
                    let rx = Arc::clone(shared);
                    struct ProgressStream(
                        Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<fs::transfer::Tick>>>,
                    );
                    impl std::hash::Hash for ProgressStream {
                        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                            "ncrs-progress".hash(state);
                        }
                    }
                    // The stream yields ticks; the app speaks messages, so
                    // the mapping happens here rather than in the app.
                    Subscription::run_with(ProgressStream(rx), |p: &ProgressStream| {
                        futures::StreamExt::map(
                            receiver_stream(Arc::clone(&p.0)),
                            Message::JobProgress,
                        )
                    })
                }
            },
        ])
    }
}
