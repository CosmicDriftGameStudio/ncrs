//! Paths other programs hand to the running ncrs: a second start, the file
//! manager D-Bus service and, on macOS, the Finder's "Open With".

use crate::app::*;
use ncrs_handoff::{Request, Target};

/// How often the inbox is looked at.
pub(in crate::app) const POLL_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// What was handed over since the last look: the inbox of this data folder and,
/// on macOS, the files the system opened ncrs with.
fn collect(data_dir: Option<&Path>) -> Vec<Request> {
    let mut requests = data_dir.map(ncrs_handoff::take).unwrap_or_default();
    #[cfg(target_os = "macos")]
    requests.extend(
        winit::platform::macos::take_opened_paths()
            .into_iter()
            .map(Request::Open),
    );
    requests
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

    /// Whether the poll runs at all.
    pub(in crate::app) fn takes_handoffs(&self) -> bool {
        self.state_path.is_some()
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
