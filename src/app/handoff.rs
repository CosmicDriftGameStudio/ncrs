//! Paths other programs hand to the running ncrs: a second start, the file
//! manager D-Bus service and, on macOS, the Finder's "Open With".

use crate::app::*;
use futures::{SinkExt as _, Stream};
use ncrs_handoff::{Request, Target};

/// How often the inbox is looked at.
const POLL_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// What was handed over since the last look: the inbox of this data folder and,
/// on macOS, the files the system opened ncrs with.
fn collect(data_dir: Option<&Path>) -> Vec<Request> {
    #[cfg_attr(
        not(target_os = "macos"),
        expect(unused_mut, reason = "only macOS adds the opened paths")
    )]
    let mut requests = data_dir.map(ncrs_handoff::take).unwrap_or_default();
    #[cfg(target_os = "macos")]
    requests.extend(
        winit::platform::macos::take_opened_paths()
            .into_iter()
            .map(Request::Open),
    );
    requests
}

pub(in crate::app) fn found_in(data_dir: PathBuf) -> impl Stream<Item = Message> {
    iced::stream::channel(1, async move |mut output| loop {
        tokio::time::sleep(POLL_EVERY).await;
        let looked_in = data_dir.clone();
        let found = tokio::task::spawn_blocking(move || collect(Some(&looked_in)))
            .await
            .unwrap_or_default();
        if !found.is_empty() && output.send(Message::HandedOver(found)).await.is_err() {
            return;
        }
    })
}

impl App {
    /// The inbox folder: next to the session state, so only the first ncrs of
    /// a data folder has one (a second one has no state path).
    fn handoff_dir(&self) -> Option<PathBuf> {
        self.state_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
    }

    /// The inbox, looked at for as long as it is there. Only a find is a
    /// message: one on every look would rebuild the window twice a second.
    pub(in crate::app) fn watch_handoffs(&self) -> Subscription<Message> {
        match self.handoff_dir() {
            Some(dir) if !self.quitting => {
                Subscription::run_with(dir, |dir| found_in(dir.clone()))
            }
            _ => Subscription::none(),
        }
    }

    /// Looks at the inbox off the UI thread; what was found comes back as
    /// `Message::HandedOver`.
    pub fn poll_handoffs(&self) -> Task<Message> {
        let data_dir = self.handoff_dir();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || collect(data_dir.as_deref()))
                    .await
                    .unwrap_or_default()
            },
            Message::HandedOver,
        )
    }

    /// Opens the last request in the active panel and brings the window to
    /// the front: a folder as the folder, a file as its folder with the cursor
    /// on it. In the App Store build the access is kept for the next start.
    pub(in crate::app) fn open_handed_over(&mut self, requests: Vec<Request>) -> Task<Message> {
        let Some((request, target)) = requests
            .into_iter()
            .rev()
            .find_map(|request| request.target().map(|target| (request, target)))
        else {
            return Task::none();
        };
        self.keep_access(request.path());
        let shown = match target {
            Target::Folder(folder) => {
                self.shown_tab = ShownTab::Panels;
                self.show_folder_in_active_panel(folder, None)
            }
            Target::File { folder, name } => {
                self.shown_tab = ShownTab::Panels;
                self.show_folder_in_active_panel(folder, Some(name))
            }
        };
        let to_front = window::latest()
            .and_then(|id| Task::batch([window::minimize(id, false), window::gain_focus(id)]));
        Task::batch([shown, to_front])
    }

    /// The system gave access to `path` for this run only; a bookmark keeps it.
    fn keep_access(&self, path: &Path) {
        if !self.sandbox.active {
            return;
        }
        if let Some(dir) = self.sandbox.grants_dir.as_deref() {
            drop((self.sandbox.grant)(dir, path));
        }
    }
}
