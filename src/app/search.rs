use crate::app::*;

impl App {
    pub(in crate::app) fn toggle_shortcuts(&self) -> ncrs_search::ToggleShortcuts {
        let label = |toggle| self.keymap.shortcut_label(&Message::Search(toggle));
        ncrs_search::ToggleShortcuts {
            case: label(SearchMessage::ToggleCase),
            whole_word: label(SearchMessage::ToggleWholeWord),
            regex: label(SearchMessage::ToggleRegex),
        }
    }

    /// The search view, while it is up.
    pub(in crate::app) fn open_search_view(&self) -> Option<&SearchView> {
        self.search.open_view(self.search_has_keys())
    }

    pub(in crate::app) fn search_has_keys(&self) -> bool {
        self.search.open && self.panels.active == self.search.side
    }

    /// What the search reads from the app.
    fn search_ctx(&self) -> ncrs_search::Ctx {
        ncrs_search::Ctx {
            window_size: self.window_size,
            side_is_remote: self.panel(self.search.side).remote().is_some(),
            has_keys: self.search_has_keys(),
            index: self
                .hermes
                .filing
                .as_ref()
                .and_then(FilingModule::query_access)
                .map(|access| Arc::new(access) as Arc<dyn ncrs_search::SearchIndex>),
        }
    }

    /// The rows of the search after its filters changed what they offer.
    pub(in crate::app) fn refresh_search_rows(&mut self) {
        self.search.resize(self.window_size);
    }

    fn focus_search_field(&self) -> Task<Message> {
        self.search.focus_field()
    }

    /// Shows the search view in place of the active panel, below that panel's
    /// directory. A view kept from before keeps its fields and options, and
    /// searches anew, so no result is older than the files. Pressed while it is
    /// up, the key moves to the next field.
    pub(in crate::app) fn open_search(&mut self) -> Task<Message> {
        if self.blocking_overlay_is_open() {
            return Task::none();
        }
        if self.search.open {
            if self.search_has_keys() {
                self.search.focus_next_field();
            } else {
                self.panels.active = self.search.side;
            }
            return self.focus_search_field();
        }
        self.panels.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let side = self.panels.active;
        if self.ops.trash_side == side {
            self.ops.close_trash();
        }
        let root = self.panel(side).path.clone();
        let indexed = self.panel(side).remote().is_none()
            && self
                .hermes
                .filing
                .as_ref()
                .and_then(FilingModule::query_access)
                .is_some();
        let kinds = Arc::clone(&self.kinds.catalog);
        self.search.show(side, root, indexed, kinds);
        self.refresh_search_rows();
        let search = self.restart_search();
        Task::batch([self.focus_search_field(), search])
    }

    /// The search opened with the words typed in the box that asked.
    pub(in crate::app) fn open_search_with_words(&mut self, text: String) -> Task<Message> {
        let opened = self.open_search();
        self.search.set_query(text);
        Task::batch([opened, self.restart_search()])
    }

    /// Searches again now, for when the files changed under the results.
    pub(in crate::app) fn restart_search(&mut self) -> Task<Message> {
        let ctx = self.search_ctx();
        self.search.restart(&ctx).map(Message::Search)
    }

    /// Lists the hits of the search in the panel on `target`.
    fn open_hit_list(&mut self, target: PanelSide) -> Task<Message> {
        let Some(view) = self.open_search_view() else {
            return Task::none();
        };
        if view.results.is_empty() {
            return Task::none();
        }
        let hits = ncrs_files::hit_list_flow::Hits {
            paths: view.results.iter().map(|file| file.path.clone()).collect(),
            query: view.query.trim().to_owned(),
            scope: view.root.clone(),
        };
        let origin = self.search.side;
        self.panels.context_menu = None;
        self.job_error = None;
        self.notice = None;
        let closed = if target == origin {
            self.close_search()
        } else {
            if self.ops.trash_view.is_some() && self.ops.trash_side == target {
                self.ops.close_trash();
            }
            Task::none()
        };
        let read = self.panels.list_hits(target, hits);
        Task::batch([closed, read.map(Message::HitList)])
    }

    pub(in crate::app) fn refresh_hit_list(&mut self, side: PanelSide) -> Task<Message> {
        self.panels.refresh_hit_list(side).map(Message::HitList)
    }

    pub(in crate::app) fn update_hit_list(&mut self, message: HitListMessage) -> Task<Message> {
        match self.panels.update_hit_list(message, self.visible_rows) {
            None => Task::none(),
            Some(HitListAsk::BackToSearch) => {
                if self.jobs.is_busy() {
                    return Task::none();
                }
                let side = self.panels.active;
                self.leave_hit_list(side, true)
            }
            Some(HitListAsk::Leave(side)) => self.leave_hit_list(side, false),
            Some(HitListAsk::ShowOnOtherSide) => self.show_hit_on_other_side(),
        }
    }

    /// Drops the list of `side` and shows the folder it came from. With
    /// `search` the search comes back, with the fields it was left with.
    fn leave_hit_list(&mut self, side: PanelSide, search: bool) -> Task<Message> {
        let Some(return_to) = self.panels.leave_hit_list(side) else {
            return Task::none();
        };
        let load = self.load(side, return_to, None);
        if !search || self.search.view.is_none() {
            return load;
        }
        if self.search.open {
            self.panels.active = self.search.side;
            return load;
        }
        self.search.open = true;
        self.panels.active = self.search.side;
        self.refresh_search_rows();
        let restarted = self.restart_search();
        Task::batch([load, restarted, self.focus_search_field()])
    }

    /// Shows the file under the cursor in its folder on the other side. The
    /// list stays.
    fn show_hit_on_other_side(&mut self) -> Task<Message> {
        let side = self.panels.active;
        let target = match self.open_search_view() {
            Some(search) => search.cursor_target().map(|(path, _)| path),
            None => self
                .panel(side)
                .selected_entry()
                .map(|entry| entry.path.clone()),
        };
        let Some((folder, name)) = target
            .as_deref()
            .and_then(ncrs_files::hit_list_flow::folder_and_name)
        else {
            return Task::none();
        };
        let other = side.other();
        let closed = if self.search.open && self.search.side == other {
            self.close_search()
        } else {
            Task::none()
        };
        if self.ops.trash_view.is_some() && self.ops.trash_side == other {
            self.ops.close_trash();
        }
        self.panels.leave_hit_list_for_hit(other);
        let load = self.load(other, folder, Some(name));
        Task::batch([closed, load])
    }

    fn close_search(&mut self) -> Task<Message> {
        self.search.close().map(Message::Search)
    }

    /// The report page after the search handed its criteria over.
    fn hand_search_to_reports(
        &mut self,
        unsaved: Option<Draft>,
        report: Option<SavedQueryId>,
    ) -> Task<Message> {
        let closed = self.close_search();
        let opened = self.open_saved_queries();
        if let Some(view) = self.hermes.saved_queries.as_mut() {
            if let Some(draft) = unsaved {
                view.prefill(draft);
            }
            if let Some(id) = report {
                view.select_when_loaded(id);
            }
        }
        Task::batch([closed, opened])
    }

    /// The search opened on `spec`, in place of the reports page. With a
    /// `report` the search changes that one.
    pub(in crate::app) fn open_search_for(
        &mut self,
        spec: &QuerySpec,
        report: Option<SavedQueryId>,
    ) -> Task<Message> {
        self.hermes.saved_queries = None;
        let opened = self.open_search();
        self.search.start_from(spec, report);
        self.refresh_search_rows();
        Task::batch([opened, self.restart_search()])
    }

    pub(in crate::app) fn update_search(&mut self, message: SearchMessage) -> Task<Message> {
        if !self.search.open {
            return Task::none();
        }
        let ctx = self.search_ctx();
        // Using the view makes its panel the active one, unless the keys it
        // asks for are not its own.
        if ncrs_search::activates_panel(&message)
            && !(ncrs_search::needs_keys(&message) && !ctx.has_keys)
        {
            self.panels.active = self.search.side;
        }
        let (task, asked) = ncrs_search::update(&mut self.search, message, &ctx);
        let mapped = task.map(Message::Search);
        match asked {
            Some(action) => Task::batch([mapped, self.run_search_action(action)]),
            None => mapped,
        }
    }

    /// What the search asked for because it reaches outside the search.
    fn run_search_action(&mut self, action: ncrs_search::Action) -> Task<Message> {
        match action {
            ncrs_search::Action::Close => self.close_search(),
            ncrs_search::Action::OpenHitList(side) => self.open_hit_list(side),
            ncrs_search::Action::ActivateCursor => self.activate_search_cursor(),
            ncrs_search::Action::OpenContextMenu => {
                self.open_search_context_menu();
                Task::none()
            }
            ncrs_search::Action::HandToReports { unsaved, report } => {
                let draft = unsaved.map(|spec| Draft {
                    name: String::new(),
                    spec,
                });
                self.hand_search_to_reports(draft, report)
            }
        }
    }

    /// Enter on a result: the file is shown in the other panel.
    fn activate_search_cursor(&mut self) -> Task<Message> {
        let Some((directory, name)) = self.search.cursor_location(self.search_has_keys()) else {
            return Task::none();
        };
        self.load(self.search.side.other(), directory, Some(name))
    }
}
