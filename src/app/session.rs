use crate::app::*;

/// How long the state waits after the last change before it is written. A
/// resize drag or a quick walk through folders writes once.
const SESSION_SAVE_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// Writes the session state. No error reaches the UI: the state is a
/// convenience, and a failed write must not interrupt the user.
pub(in crate::app) fn write_session_state(
    order: &WriteOrder,
    generation: u64,
    path: &Path,
    state: &SessionState,
) {
    if let Err(err) = order.save(generation, path, state) {
        eprintln!("ncrs: {}: {err}", path.display());
    }
}

impl App {
    pub(in crate::app) fn apply_window_size(&mut self, size: Size) {
        self.window_size = size;
        self.resize_command_palette(size);
        self.visible_rows = layout::visible_rows(size);
        self.search.resize(size);
        self.panels.left.ensure_visible(self.visible_rows);
        self.panels.right.ensure_visible(self.visible_rows);
    }

    /// Puts what an earlier run remembered on top of the fresh start. The first
    /// listing of each panel arrives sorted the default way and is re-sorted by
    /// `apply_sorted_listing`, which compares its `sorted_by` with the panel's.
    pub(in crate::app) fn apply_session_state(&mut self, saved: &SessionState) {
        self.panels.left.sort = saved.left.sort_key();
        self.panels.right.sort = saved.right.sort_key();
        self.panels.left.history = saved.left.history();
        self.panels.right.history = saved.right.history();
        // Not checked for existence: a favourite on a drive that is not
        // plugged in now is still wanted when it is.
        self.panels.favourites = saved.favourites.iter().map(PathBuf::from).collect();
        if let Some(side) = saved.active {
            self.panels.active = side;
        }
        if let Some(size) = saved.window_size() {
            self.apply_window_size(size);
        }
        // Starting is not a change: nothing is written until something differs.
        self.session_scheduled = self.session_state();
    }

    /// What would be remembered if ncrs quit now.
    pub(in crate::app) fn session_state(&self) -> SessionState {
        let record = |panel: &PanelState| {
            // A restart must not unpack a large tar.zst before the panel shows
            // anything, and the archive may be gone or replaced by then, so a
            // panel inside one is remembered as standing on the archive file.
            let (path, cursor) = match panel.archive.as_deref().and_then(archive_file_place) {
                Some((folder, name)) => (session_state::record_path(folder), Some(name)),
                // A server is never written to the file; the panel comes back
                // in the home folder.
                None if panel.remote().is_some() => (session_state::record_path(&self.home), None),
                None => (
                    session_state::record_path(
                        panel
                            .hit_list
                            .as_ref()
                            .map_or(&panel.path, |list| &list.return_to),
                    ),
                    panel
                        .selected_entry()
                        .filter(|entry| !entry.is_parent)
                        .and_then(|entry| entry.name.to_str().map(str::to_owned)),
                ),
            };
            PanelRecord {
                path,
                sort: panel.sort.column,
                descending: panel.sort.descending,
                back: session_state::record_history(panel.history.back()),
                forward: session_state::record_history(panel.history.forward()),
                cursor,
            }
        };
        let current = WindowSize {
            width: self.window_size.width,
            height: self.window_size.height,
        };
        // A minimised window reports no usable size; the last good one stays.
        let window = if current.restorable().is_some() {
            Some(current)
        } else {
            self.session_scheduled.window
        };
        SessionState {
            version: session_state::STATE_VERSION,
            active: Some(self.panels.active),
            favourites: session_state::record_paths(&self.panels.favourites),
            window,
            left: record(&self.panels.left),
            right: record(&self.panels.right),
        }
    }

    /// Plans a write when the remembered state differs from the last one
    /// planned. Debounced: the timer carries the generation, so a resize drag
    /// or a quick walk through folders writes once, after it settles.
    pub(in crate::app) fn schedule_session_save(&mut self) -> Task<Message> {
        if self.state_path.is_none() {
            return Task::none();
        }
        let snapshot = self.session_state();
        if snapshot == self.session_scheduled {
            return Task::none();
        }
        self.session_scheduled = snapshot;
        self.session_save_generation = self.session_save_generation.wrapping_add(1);
        let generation = self.session_save_generation;
        Task::perform(
            async { tokio::time::sleep(SESSION_SAVE_DELAY).await },
            move |()| Message::SaveSessionState(generation),
        )
    }

    /// The write a timer is entitled to: only the newest generation, and only
    /// with a state path.
    pub(in crate::app) fn due_session_state(
        &self,
        generation: u64,
    ) -> Option<(PathBuf, SessionState)> {
        if generation != self.session_save_generation {
            return None;
        }
        Some((self.state_path.clone()?, self.session_scheduled.clone()))
    }
}
