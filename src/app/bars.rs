use crate::app::*;

const APP_NAME: &str = "NC-rs";

impl App {
    pub fn title(&self) -> String {
        format!("{APP_NAME} – {}", self.active_panel().path.display())
    }

    pub fn theme(&self) -> Theme {
        theme::app_theme()
    }

    /// The error the status line shows: a job's, else the config's.
    pub fn status_error(&self) -> Option<&str> {
        self.job_error.as_deref().or(self.config_error.as_deref())
    }

    /// What the bar's path field says while the active panel is a hit list:
    /// the query, where it ran and how many files it left, with the way back.
    /// The keys the bar shows: the search's while it has the keys, else the
    /// panels'. A hit list is no folder, so F7 (new folder) is left out.
    pub(in crate::app) fn function_key_bar(
        &self,
    ) -> [Option<&'static str>; keymap::FUNCTION_KEY_COUNT] {
        if self.search_has_keys() {
            return self.search_function_keys;
        }
        let mut keys = self.function_keys;
        if self.active_panel().is_hit_list() {
            if let Some(slot) = keys.get_mut(keymap::NEW_FOLDER_SLOT) {
                *slot = None;
            }
        }
        keys
    }

    pub(in crate::app) fn hit_line(&self) -> Option<statusbar::HitLine> {
        let panel = self.active_panel();
        let list = panel.hit_list.as_ref()?;
        let count = panel.entries.len();
        let scope = titlebar::tab_label(&list.scope, &self.home, false, usize::MAX);
        Some(statusbar::HitLine {
            text: ncrs_files::hit_list_flow::hit_line_text(self.lang, list, count, &scope),
            back_key: "Esc".to_owned(),
            back_label: self.lang.text(Msg::HitListBackHint).to_owned(),
        })
    }

    /// What the status bar shows instead of the panel's entry while the search
    /// has the keys: the cursor's file below the root.
    /// Empty without a cursor, so the panel's entry does not pose as the
    /// search's.
    pub(in crate::app) fn search_status(&self) -> Option<String> {
        self.search.status(self.search_has_keys())
    }

    /// What the status bar shows about the queue: the running job, how far
    /// along it is, how many wait, and whether Escape will stop it.
    pub(in crate::app) fn job_status(&self) -> Option<statusbar::JobStatus> {
        let running = self.jobs.running()?;
        // The tick from the blocking thread, not a row counter: F5 on one
        // directory with fifty thousand files is one row and fifty thousand
        // items, and the bar has to say the second number.
        let (done, total) = match self.job_progress {
            Some(tick) => (tick.done, tick.total),
            None => (0, running.total.unwrap_or(0)),
        };
        Some(statusbar::JobStatus {
            label: self.lang.text(running.kind.label()),
            done,
            total,
            waiting: self.jobs.waiting(),
            // Escape belongs to the prompt while one is open, so the hint is
            // only true when nothing else is claiming the key.
            abortable: self.ops.prompt.is_none() && !self.ops.conflict_is_open(),
        })
    }

    /// Records why a job failed. The status bar reads it; nothing else does, so
    /// it is a string rather than a typed error — the layer that knew the kind
    /// already wrote the wording.
    pub(in crate::app) fn set_status_error(&mut self, reason: &str) {
        self.job_error = Some(reason.to_string());
        self.job_error_press = None;
        self.notice = None;
    }

    /// An error the user can press, for instance to open the trash in the Finder.
    pub(in crate::app) fn set_status_error_with_press(&mut self, reason: &str, press: Message) {
        self.set_status_error(reason);
        self.job_error_press = Some(press);
    }

    fn hermes_status(&self) -> Option<HermesStatus> {
        ncrs_hermes_ui::hermes_status(&self.hermes)
    }

    fn hermes_chip(&self) -> Option<String> {
        ncrs_hermes_ui::hermes_chip(&self.hermes, self.lang)
    }

    pub(in crate::app) fn filing_badge(&self) -> Option<statusbar::FilingBadge<Message>> {
        ncrs_hermes_ui::filing_badge(&self.hermes, self.lang).map(|badge| statusbar::FilingBadge {
            text: badge.text,
            failing: badge.failing,
            on_press: Message::FilingStatus(FilingStatusMessage::Open),
        })
    }

    pub(in crate::app) fn hermes_line(&self) -> Option<String> {
        let cursor = self.active_panel().selected_entry()?;
        ncrs_hermes_ui::hermes_line(&self.hermes, Some(&cursor.path), self.lang)
    }

    pub(in crate::app) fn title_bar_view(&self) -> Element<'_, Message> {
        let tab = |side: PanelSide| {
            let title = match self.search.view.as_ref() {
                _ if self.panel(side).is_hit_list() => {
                    let query = self
                        .panel(side)
                        .hit_list
                        .as_ref()
                        .map_or("", |list| list.query.as_str());
                    titlebar::TabTitle::HitList(fill_template(
                        self.lang.text(Msg::HitListTab),
                        &[("{query}", query)],
                    ))
                }
                Some(search) if self.search.open && side == self.search.side => {
                    titlebar::TabTitle::Search(ncrs_search::title(search, self.lang))
                }
                _ => match self.ops.trash_view.as_ref() {
                    Some(view) if side == self.ops.trash_side => titlebar::TabTitle::Trash(
                        trash_ui::heading(view.reach, self.lang).to_owned(),
                    ),
                    _ => titlebar::TabTitle::Path {
                        path: &self.panel(side).path,
                        loading: self.panel(side).loading,
                    },
                },
            };
            titlebar::Tab {
                side,
                title,
                active: self.panels.active == side && !self.page_tab_is_open(),
            }
        };
        titlebar::view(&titlebar::TitleBar {
            tabs: [tab(PanelSide::Left), tab(PanelSide::Right)],
            page_tab: self.page_tab(),
            home: &self.home,
            hermes: self.hermes_chip(),
            hermes_tone: self
                .hermes_status()
                .map_or(titlebar::HermesTone::Fine, |status| status.tone()),
            lab_shortcut: self
                .keymap
                .shortcut_label(&KeyAction::CommandPalette.message()),
            lang: self.lang,
            palette: &self.palette,
        })
    }

    fn page_tab_is_open(&self) -> bool {
        self.settings.page.is_some() || self.ablage_tab_is_open()
    }

    fn page_tab(&self) -> Option<titlebar::PageTab> {
        if self.settings.page.is_some() {
            Some(titlebar::PageTab::Settings)
        } else {
            self.open_hermes_page()
                .map(|page| titlebar::PageTab::Filing {
                    close: Box::new(Self::close_message_of(page)),
                })
        }
    }

    /// What Escape sends on the top level of the page of filing.
    fn close_message_of(page: HermesPage) -> Message {
        match page {
            HermesPage::Overview => Message::HermesOverview(HermesOverviewMessage::Close),
            HermesPage::Inspection => Message::FilingStatus(FilingStatusMessage::Close),
            HermesPage::Review => Message::ReviewList(ReviewListMessage::Close),
            HermesPage::SavedQueries => Message::SavedQueries(SavedQueriesMessage::Close),
            HermesPage::Rules => Message::Rules(RulesMessage::Close),
            HermesPage::RuleSuggestions => Message::RuleSuggestions(RuleSuggestionsMessage::Close),
            HermesPage::Tags => Message::Tags(TagsMessage::Close),
            HermesPage::Reorganisation => {
                Message::ReorganisationPlan(ReorganisationPlanMessage::Close)
            }
        }
    }

    /// The name of the colour theme when the palette is one of the presets.
    pub(in crate::app) fn theme_name(&self) -> Option<String> {
        ThemePreset::ALL
            .into_iter()
            .find(|preset| preset.palette() == self.palette)
            .map(|preset| {
                let name = preset.name();
                let mut letters = name.chars();
                letters.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(letters).collect()
                })
            })
    }

    #[cfg(test)]
    pub(crate) fn hermes_chip_for_test(&self) -> Option<String> {
        self.hermes_chip()
    }
}
