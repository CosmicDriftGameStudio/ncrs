use crate::app::*;

/// A message rather than a call: it is handled in `update`, on the thread
/// that owns the app's windows, once the app has started and the default menu
/// winit installed is there to be replaced.
#[cfg(target_os = "macos")]
fn then_install_native_menu(startup: Task<Message>) -> Task<Message> {
    Task::batch([startup, Task::done(Message::InstallNativeMenu)])
}

#[cfg(not(target_os = "macos"))]
fn then_install_native_menu(startup: Task<Message>) -> Task<Message> {
    startup
}

/// What is wrong with the config, kept to word it again after F9.
#[derive(Debug)]
pub(in crate::app) enum ConfigProblem {
    /// The file was not used; the defaults apply.
    Invalid(ConfigError),
    /// The file is in use, but one pairing of colors is hard to read.
    LowContrast(ContrastWarning),
    /// The file is in use, but the VS Code theme it names was not applied in full.
    VscodeTheme(VscodeWarning),
    UnknownLanguage(LanguageWarning),
    /// The file is in use, but some `[[kinds]]` entries or words were left out.
    Kinds(KindWarning),
}

/// Where the app finds its config and index and what holds the index key.
#[derive(Clone)]
pub struct Environment {
    pub config_path: Option<PathBuf>,
    pub index_path: Option<PathBuf>,
    pub state_path: Option<PathBuf>,
    /// The record of what ncrs trashed, where the system gives no list.
    pub journal_path: Option<PathBuf>,
    /// The cache folder F3 unpacks archive files into.
    pub view_copies_dir: Option<PathBuf>,
    pub key_store: Arc<dyn KeyStore + Send + Sync>,
    /// Where the passwords of SFTP servers are kept, when the user asks for it.
    pub passwords: Arc<dyn PasswordStore>,
    /// The user's OpenSSH `known_hosts`, which is only read.
    pub ssh_known_hosts: Option<PathBuf>,
    /// The host keys the user confirmed in ncrs.
    pub own_known_hosts: Option<PathBuf>,
    /// Another ncrs holds the data folder: this one runs as a plain file
    /// manager, without Hermes and the index, and leaves the session state and
    /// the trash record to the first.
    pub second_instance: bool,
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment")
            .field("config_path", &self.config_path)
            .field("index_path", &self.index_path)
            .field("state_path", &self.state_path)
            .field("journal_path", &self.journal_path)
            .field("view_copies_dir", &self.view_copies_dir)
            .field("ssh_known_hosts", &self.ssh_known_hosts)
            .field("own_known_hosts", &self.own_known_hosts)
            .finish_non_exhaustive()
    }
}

impl Environment {
    /// The config, index and keychain of the person running ncrs.
    pub fn of_the_user() -> Self {
        Self {
            config_path: config::default_path(),
            index_path: config::default_index_path(),
            state_path: config::default_state_path(),
            journal_path: config::default_journal_path(),
            view_copies_dir: config::default_view_copies_dir(),
            key_store: user_key_store(),
            passwords: Arc::new(KeyringPasswords::new()),
            ssh_known_hosts: Some(config::default_ssh_known_hosts_path()),
            own_known_hosts: config::default_known_hosts_path(),
            second_instance: false,
        }
    }

    /// The environment for a start that found another ncrs running.
    pub fn for_second_instance(self) -> Self {
        Self {
            index_path: None,
            state_path: None,
            journal_path: None,
            second_instance: true,
            ..self
        }
    }

    /// No config file, no index, a key store in memory: nothing outside the
    /// test.
    #[cfg(test)]
    pub fn isolated() -> Self {
        Self {
            config_path: None,
            index_path: None,
            state_path: None,
            journal_path: None,
            view_copies_dir: None,
            key_store: Arc::new(ncrs_index::InMemoryKeyStore::empty()),
            passwords: Arc::new(fs::remote::credentials::InMemoryPasswords::default()),
            ssh_known_hosts: None,
            own_known_hosts: None,
            second_instance: false,
        }
    }
}

/// The keychain service of the App Store build. Its own name keeps that build
/// from reading, or replacing, the key an installed ncrs keeps under "ncrs":
/// the two have different indexes, and a key that does not open one is lost
/// for it.
pub(in crate::app) const APP_STORE_KEY_SERVICE: &str = "ncrs-appstore";

/// The one place the app builds the system keychain store.
fn user_key_store() -> Arc<dyn KeyStore + Send + Sync> {
    config::forbid_user_machine_in_tests("the system keychain");
    if let Some(dir) = config::data_dir_override() {
        return Arc::new(ncrs_index::FileKeyStore::new(dir.join("index.key")));
    }
    if fs::sandbox_active() {
        return Arc::new(KeyringKeyStore::with_service(APP_STORE_KEY_SERVICE));
    }
    Arc::new(
        config::portable_data_dir()
            .map_or_else(KeyringKeyStore::new, |dir| KeyringKeyStore::portable(&dir)),
    )
}

impl App {
    /// Version 0 is no state file at all: the first start of ncrs, which
    /// begins with the programs folder among the favourites.
    pub(crate) fn with_first_start_favourites(mut saved: SessionState) -> SessionState {
        if saved.version == 0 && saved.favourites.is_empty() {
            saved.favourites = crate::session_state::record_paths(
                &ncrs_files::special_folders::first_start_favourites(),
            );
        }
        saved
    }

    /// Initial state plus tasks that load both panels. The panels open where
    /// `saved` left them; without a saved folder the left panel starts in the
    /// working directory and the right in `$HOME`.
    pub fn new(environment: Environment, saved: SessionState) -> (Self, Task<Message>) {
        // The system language is the start; a `lang` entry in the config
        // replaces it in `with_config`, so a config written without one (a
        // column width, a server) keeps the system language.
        let start_language = environment
            .config_path
            .is_some()
            .then(ncrs_i18n::Language::from_system);
        let (app, loading) = Self::restored_in(
            environment,
            Self::with_first_start_favourites(saved),
            start_language,
        );
        (app, then_install_native_menu(loading))
    }

    /// `new` with the places it reads and writes given instead of looked up,
    /// and nothing remembered from an earlier run.
    #[cfg(test)]
    pub fn with_environment(environment: Environment) -> (Self, Task<Message>) {
        Self::restored(environment, SessionState::default())
    }

    /// Starts where `saved` left off: its folders (or their nearest existing
    /// parent), sort, active panel and window size.
    #[cfg(test)]
    pub fn restored(environment: Environment, saved: SessionState) -> (Self, Task<Message>) {
        Self::restored_in(environment, saved, None)
    }

    /// [`App::restored`] with the language to start in when the config names none.
    fn restored_in(
        environment: Environment,
        saved: SessionState,
        start_language: Option<ncrs_i18n::Language>,
    ) -> (Self, Task<Message>) {
        let Environment {
            config_path,
            index_path,
            state_path,
            journal_path,
            view_copies_dir,
            key_store,
            passwords,
            ssh_known_hosts,
            own_known_hosts,
            second_instance,
        } = environment;
        let loaded = config_path
            .as_deref()
            .map_or_else(|| Ok(Config::default()), config::load);
        if let Err(err) = &loaded {
            eprintln!("ncrs: {err}");
        }
        let mut filing = loaded
            .as_ref()
            .map(|config| config.filing.clone())
            .unwrap_or_default();
        if second_instance {
            filing.enabled = false;
            filing.first_run = false;
        }
        let home = fs::home_dir();
        let left_directory = saved.left.start_directory();
        let right_directory = saved.right.start_directory();
        let cursors = [
            saved.left.start_cursor_in(left_directory.as_deref()),
            saved.right.start_cursor_in(right_directory.as_deref()),
        ];
        let (mut panels, panel_loads) = Self::starting_at(
            left_directory.unwrap_or_else(fs::start_dir),
            right_directory.unwrap_or_else(|| home.clone()),
            home,
            cursors,
        );
        panels.config_path = config_path;
        if let Some(language) = start_language {
            panels.lang = language;
        }
        panels.updates = ncrs_updates::State::new(
            panels.app_version,
            ncrs_updates::Channel::detect_here(env!("NCRS_BUILD_KIND") != "release"),
            state_path
                .as_deref()
                .map(ncrs_updates::file_beside)
                .as_deref(),
        );
        panels.store = super::store::StoreState::in_folder(
            state_path.as_deref().and_then(std::path::Path::parent),
        );
        panels.state_path = state_path;
        panels.trash = fs::system_trash(journal_path);
        panels.sandbox = fs::SandboxAccess::detect(index_path.as_deref());
        // The system-wide file is read wherever the user's own is: a start
        // that has no user file (tests) must not read this machine's either.
        let global = if ssh_known_hosts.is_some() {
            config::default_global_ssh_known_hosts_path()
        } else {
            None
        };
        panels.remote = Arc::new(Pool::over_tcp(KnownHostsFiles {
            ssh: ssh_known_hosts,
            global,
            own: own_known_hosts,
        }));
        panels.passwords = passwords;
        panels.open_grant_dialog_if_needed();
        if let Some(dir) = view_copies_dir.as_deref() {
            match fs::archive::ViewCopies::open(dir) {
                Ok(copies) => panels.ops.view_copies = Some(copies),
                Err(err) => {
                    panels.ops.view_copies_problem = Some((err.kind(), err.to_string()));
                }
            }
        }
        panels.apply_session_state(&saved);
        let (mut app, filing_start) = panels
            .with_config(loaded)
            .with_filing(&filing, index_path, key_store);
        app.refresh_special_folders();
        let first_check = Task::done(Message::Updates(ncrs_updates::Message::CheckDue));
        let notes_start = app.start_notes();
        let store_start = app.start_store();
        (
            app,
            Task::batch([
                panel_loads,
                filing_start,
                first_check,
                Task::done(Message::SpaceHintDue),
                notes_start,
                store_start,
            ]),
        )
    }

    /// Starts the filing module when `filing` switches it on. The module
    /// creates a missing index by itself, encrypted, and works on no folder
    /// until the user picks one in the chat.
    pub fn with_filing(
        mut self,
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: Arc<dyn KeyStore + Send + Sync>,
    ) -> (Self, Task<Message>) {
        if let Some(data_dir) = db_path.as_deref().and_then(std::path::Path::parent) {
            self.hermes_chat
                .use_chat_store(data_dir.join("hermes").join("chats"), chrono::Local::now());
        }
        self.hermes.index_path = db_path;
        self.hermes.key_store = Some(ncrs_hermes_ui::KeyStoreHandle(store));
        let task = self.start_filing_module(filing);
        (self, task)
    }

    /// Applies a loaded config. A broken one leaves the defaults and says why
    /// in the status line.
    pub fn with_config(mut self, loaded: Result<Config, ConfigError>) -> Self {
        match loaded {
            Ok(config) => {
                // First: `function_keys` and `report` word themselves in `self.lang`.
                if let Some(lang) = config.lang {
                    self.lang = lang;
                }
                self.apply_look(config.clone());
                self.open_programs = config.open;
                let notes_settings = config.notes.settings();
                if let Some(folder) = config.notes.folder {
                    self.notes = ncrs_notes::State::new(folder);
                }
                ncrs_notes::apply_settings(&mut self.notes, &notes_settings);
                self.saved_servers = config.servers;
                self.file_list_tags = config.file_list_tags;
                self.columns = config.columns;
                self.git_view_settings = config.git_view;
                self.git_settings = config.git;
                self.start_appearance(config.appearance);
                self.keymap = config.keymap;
                self.apply_git_keys(&config.git_keys);
                self.repos_settings = config.repos;
                self.apply_repos_keys(&config.repos_keys);
                self.space_settings = config.space;
                self.apply_space_keys(&config.space_keys);
                self.refresh_space_facts();
                self.monitor.set_settings(config.system);
                self.update_check = config.check_for_updates;
                self.favourites_bar_on = config.favourites_bar.unwrap_or(true);
                self.hermes_chat
                    .set_chat_keep_days(config.hermes.chat_keep_days);
                self.refresh_function_keys();
            }
            Err(err) => self.report(ConfigProblem::Invalid(err)),
        }
        self
    }

    /// Says what is off about the look a config asks for. The VS Code warning
    /// goes last, so it wins: it explains why the look is not the one asked for.
    pub(in crate::app) fn report_look_warnings(
        &mut self,
        contrast: Option<ContrastWarning>,
        vscode: Option<VscodeWarning>,
    ) {
        if let Some(warning) = contrast {
            self.report(ConfigProblem::LowContrast(warning));
        }
        if let Some(warning) = vscode {
            self.report(ConfigProblem::VscodeTheme(warning));
        }
    }

    pub(in crate::app) fn report_kind_warning(&mut self, found: Option<KindWarning>) {
        if let Some(warning) = found {
            self.report(ConfigProblem::Kinds(warning));
        }
    }

    pub(in crate::app) fn report_language_warning(&mut self, found: Option<LanguageWarning>) {
        if let Some(warning) = found {
            self.report(ConfigProblem::UnknownLanguage(warning));
        }
    }

    /// F9. A config error still up is worded again in the new language.
    pub(in crate::app) fn switch_language(&mut self) {
        self.set_language(self.lang.other());
    }

    pub(in crate::app) fn set_language(&mut self, lang: Language) {
        self.lang = lang;
        // The open page worded its facts (the order's areas) in the old language.
        if let Some(page) = self.settings.page.as_mut() {
            page.facts_requested = false;
        }
        if let Some(module) = &mut self.hermes.filing {
            module.set_kind_language(lang.kind_language());
        }
        self.kinds = self.kinds.in_language(lang.kind_language());
        let kinds = self.kinds.clone();
        if let Some(view) = &mut self.hermes.saved_queries {
            view.set_kinds(kinds.clone());
        }
        if let Some(page) = &mut self.hermes.tags {
            page.set_kinds(kinds);
        }
        self.refresh_function_keys();
        if let Some(problem) = &self.config_problem {
            self.config_error = Some(self.config_error_text(problem));
        }
    }

    pub(in crate::app) fn refresh_function_keys(&mut self) {
        self.function_keys = self.keymap.function_keys(self.lang, KeyScope::Panel);
        self.search_function_keys = self.keymap.function_keys(self.lang, KeyScope::Search);
    }

    pub(in crate::app) fn report(&mut self, problem: ConfigProblem) {
        self.config_error = Some(self.config_error_text(&problem));
        self.config_problem = Some(problem);
    }

    pub(in crate::app) fn config_error_text(&self, problem: &ConfigProblem) -> String {
        match problem {
            ConfigProblem::Invalid(err) => {
                let path = err.path().display().to_string();
                match err.line() {
                    Some(line) => fill_template(
                        self.lang.text(Msg::ErrorConfigAtLine),
                        &[
                            ("{path}", &path),
                            ("{line}", &line.to_string()),
                            ("{reason}", err.reason()),
                        ],
                    ),
                    None => fill_template(
                        self.lang.text(Msg::ErrorConfig),
                        &[("{path}", &path), ("{reason}", err.reason())],
                    ),
                }
            }
            ConfigProblem::VscodeTheme(warning) => {
                let path = warning.path.display().to_string();
                let theme = warning.theme.display().to_string();
                let values = [
                    ("{path}", path.as_str()),
                    ("{theme}", theme.as_str()),
                    ("{reason}", warning.reason.as_str()),
                ];
                match warning.line {
                    Some(number) => {
                        let line = number.to_string();
                        let mut with_line = values.to_vec();
                        with_line.push(("{line}", line.as_str()));
                        fill_template(self.lang.text(Msg::ErrorVscodeThemeAtLine), &with_line)
                    }
                    None => fill_template(self.lang.text(Msg::ErrorVscodeTheme), &values),
                }
            }
            ConfigProblem::UnknownLanguage(warning) => {
                let path = warning.path.display().to_string();
                let allowed = Language::ALL
                    .map(|language| format!("\"{}\"", language.code()))
                    .join(", ");
                let values = [
                    ("{path}", path.as_str()),
                    ("{code}", warning.code.as_str()),
                    ("{allowed}", allowed.as_str()),
                ];
                match warning.line {
                    Some(number) => {
                        let line = number.to_string();
                        let mut with_line = values.to_vec();
                        with_line.push(("{line}", line.as_str()));
                        fill_template(self.lang.text(Msg::ErrorUnknownLanguageAtLine), &with_line)
                    }
                    None => fill_template(self.lang.text(Msg::ErrorUnknownLanguage), &values),
                }
            }
            ConfigProblem::Kinds(warning) => {
                let path = warning.path.display().to_string();
                let skipped = warning
                    .skips
                    .iter()
                    .map(|skip| match skip {
                        KindSkip::EmptyId => self.lang.text(Msg::ErrorKindsNoId).to_owned(),
                        KindSkip::Word { id, word } => fill_template(
                            self.lang.text(Msg::ErrorKindsWordTaken),
                            &[("{word}", word), ("{id}", id)],
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                fill_template(
                    self.lang.text(Msg::ErrorKinds),
                    &[("{path}", &path), ("{skipped}", &skipped)],
                )
            }
            ConfigProblem::LowContrast(warning) => {
                let path = warning.path.display().to_string();
                let ratio = format!("{:.1}", warning.ratio);
                let colors = [
                    ("{path}", path.as_str()),
                    ("{foreground}", warning.foreground.name()),
                    ("{background}", warning.background.name()),
                    ("{ratio}", ratio.as_str()),
                ];
                match warning.line {
                    Some(number) => {
                        let line = number.to_string();
                        let mut values = colors.to_vec();
                        values.push(("{line}", line.as_str()));
                        fill_template(self.lang.text(Msg::ErrorContrastAtLine), &values)
                    }
                    None => fill_template(self.lang.text(Msg::ErrorContrast), &colors),
                }
            }
        }
    }

    /// Like `new`, with both panels' start directories given. Tests use this
    /// to stay inside their own scratch directory instead of the process-wide
    /// working directory and `$HOME`.
    #[cfg(test)]
    pub fn starting_in(start: PathBuf, home: PathBuf) -> (Self, Task<Message>) {
        let right = home.clone();
        Self::starting_at(start, right, home, [None, None])
    }

    /// `starting_in` with the right panel's folder of its own; `home` stays
    /// what `~` means.
    /// `cursors` name the entries the first listings put the cursor on.
    fn starting_at(
        left: PathBuf,
        right: PathBuf,
        home: PathBuf,
        cursors: [Option<OsString>; 2],
    ) -> (Self, Task<Message>) {
        let [left_cursor, right_cursor] = cursors;
        let notes_root = home.join("Notes");
        let mut app = Self {
            panels: Panels::new(
                PanelState::new(left.clone()),
                PanelState::new(right.clone()),
                PanelSide::Left,
            ),
            visible_rows: layout::visible_rows(layout::INITIAL_WINDOW_SIZE, 0.0),
            lang: Language::default(),
            function_keys: Keymap::built_in().function_keys(Language::default(), KeyScope::Panel),
            search_function_keys: Keymap::built_in()
                .function_keys(Language::default(), KeyScope::Search),
            keymap: Keymap::built_in(),
            git_keys: Arc::default(),
            repos_keys: Arc::default(),
            repos: None,
            repos_settings: ncrs_config::repos::ReposSettings::default(),
            space_keys: Arc::default(),
            space: None,
            space_settings: ncrs_config::space::SpaceSettings::default(),
            space_processes_known: space::processes_known(),
            space_last_scan: None,
            space_run_ask: None,
            space_progress: None,
            last_own_trash: Vec::new(),
            space_hint: None,
            home_volume: None,
            jobs: jobs::Queue::new(),
            start_over_dialog: false,
            dialog_focus: 0,
            modifiers: Modifiers::default(),
            trash: fs::system_trash(None),
            launcher: Arc::new(fs::SystemLauncher),
            open_programs: config::OpenPrograms::default(),
            config_error: None,
            config_problem: None,
            palette: Palette::default(),
            job_error: None,
            job_error_press: None,
            store: super::store::StoreState::unrestricted(),
            quitting: false,
            notice: None,
            notice_generation: 0,
            job_done: 0,
            ops: ncrs_ops::Ops::default(),
            job_progress: None,
            progress_rx: None,
            progress_tx: None,
            key_state: OnceLock::new(),
            command_line: CommandLine::default(),
            home,
            clock: Arc::new(std::time::Instant::now),
            window_size: layout::INITIAL_WINDOW_SIZE,
            title_tabs_first: 0,
            state_path: None,
            session_scheduled: SessionState::default(),
            session_save_generation: 0,
            session_writes: Arc::default(),
            input: Arc::default(),
            list_volumes: Arc::new(fs::list_volumes),
            search: ncrs_search::Search::new(layout::INITIAL_WINDOW_SIZE),
            monitor: ncrs_monitor::State::default(),
            viewer: ncrs_viewer::State::default(),
            notes: ncrs_notes::State::new(notes_root),
            filing_view: ncrs_filing_view::State::default(),
            git_tabs: git_tabs::GitTabs::default(),
            shown_tab: crate::messages::ShownTab::default(),
            updates: ncrs_updates::State::new(
                env!("NCRS_DISPLAY_VERSION"),
                ncrs_updates::Channel::Direct,
                None,
            ),
            update_check: None,
            favourites_bar_on: true,
            favourites_bar_applied: 0.0,
            favourite_menu: None,
            fetch_latest_release: ncrs_updates::latest_release,
            hermes_chat: ncrs_hermes_chat::State::default(),
            chat_model: None,
            chat_model_setting: None,
            chat_model_error: None,
            turning_hermes_on: false,
            protection_quit_warned: None,
            quick_open: None,
            command_palette: None,
            recent_commands: Vec::new(),
            repos_read_only: false,
            wall_clock: Arc::new(ncrs_pipeline::SystemClock),
            facts_requested_at: None,
            system_reads_scans: crate::ocr_models::system_reads_scans(),
            config_path: None,
            file_list_tags: FileListTags::default(),
            kinds: Kinds::default(),
            columns: ncrs_config::panel_columns::PanelColumns::default(),
            git_view_settings: ncrs_config::git_view::GitViewSettings::default(),
            git_settings: ncrs_config::git::GitSettings::default(),
            special_folders: Vec::new(),
            special_folders_source: (Vec::new(), Vec::new()),
            appearance: ncrs_config::panel_columns::Appearance::default(),
            ui_scale: 1.0,
            settings: Settings::default(),
            hermes: ncrs_hermes_ui::Hermes::default(),
            theme_preset: ThemePreset::DEFAULT,
            vscode_theme: None,
            resolve_folder: filing_setup::resolve_folder,
            pick_folder: crate::system_dialogs::pick_folder_with_system_dialog,
            pick_file: crate::system_dialogs::pick_file_with_system_dialog,
            read_clipboard: clipboard_paste::read_clipboard_off_the_ui_thread,
            write_clipboard_files: {
                let mut writer = FileListWriter::default();
                Box::new(move |files| writer.write(files))
            },
            tag_exceptions: TagExceptions::new(),
            fetch_extension: open_vsx::fetch_extension,
            install_theme: ncrs_settings::theme_fetch::install::install_picked_theme,
            about: None,
            find_third_party_licenses: ncrs_settings::about::installed_third_party_licenses,
            app_version: env!("NCRS_DISPLAY_VERSION"),
            download_ocr_models: download_models,
            download_ai_model,
            grant_dialog: false,
            sandbox: fs::SandboxAccess::detect(None),
            remote: Arc::new(Pool::over_tcp(KnownHostsFiles::default())),
            passwords: Arc::new(KeyringPasswords::new()),
            remote_ui: ncrs_remote::Remote::default(),
            compare: ncrs_compare::Compare::default(),
            saved_servers: Vec::new(),
        };

        let tasks = Task::batch([
            app.load(PanelSide::Left, left, left_cursor),
            app.load(PanelSide::Right, right, right_cursor),
        ]);
        (app, tasks)
    }
}
