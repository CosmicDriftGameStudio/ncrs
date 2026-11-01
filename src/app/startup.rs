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
            key_store: Arc::new(user_key_store()),
            passwords: Arc::new(KeyringPasswords::new()),
            ssh_known_hosts: Some(config::default_ssh_known_hosts_path()),
            own_known_hosts: config::default_known_hosts_path(),
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
        }
    }
}

/// The keychain service of the App Store build. Its own name keeps that build
/// from reading, or replacing, the key an installed ncrs keeps under "ncrs":
/// the two have different indexes, and a key that does not open one is lost
/// for it.
pub(in crate::app) const APP_STORE_KEY_SERVICE: &str = "ncrs-appstore";

/// The one place the app builds the system keychain store.
fn user_key_store() -> KeyringKeyStore {
    config::forbid_user_machine_in_tests("the system keychain");
    if fs::sandbox_active() {
        return KeyringKeyStore::with_service(APP_STORE_KEY_SERVICE);
    }
    config::portable_data_dir()
        .map_or_else(KeyringKeyStore::new, |dir| KeyringKeyStore::portable(&dir))
}

impl App {
    /// Initial state plus tasks that load both panels. The panels open where
    /// `saved` left them; without a saved folder the left panel starts in the
    /// working directory and the right in `$HOME`.
    pub fn new(environment: Environment, saved: SessionState) -> (Self, Task<Message>) {
        let (app, loading) = Self::restored(environment, saved);
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
    pub fn restored(environment: Environment, saved: SessionState) -> (Self, Task<Message>) {
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
        } = environment;
        let loaded = config_path
            .as_deref()
            .map_or_else(|| Ok(Config::default()), config::load);
        if let Err(err) = &loaded {
            eprintln!("ncrs: {err}");
        }
        let filing = loaded
            .as_ref()
            .map(|config| config.filing.clone())
            .unwrap_or_default();
        let home = fs::home_dir();
        let (mut panels, panel_loads) = Self::starting_at(
            saved.left.start_directory().unwrap_or_else(fs::start_dir),
            saved
                .right
                .start_directory()
                .unwrap_or_else(|| home.clone()),
            home,
            [saved.left.start_cursor(), saved.right.start_cursor()],
        );
        panels.config_path = config_path;
        panels.state_path = state_path;
        panels.trash = fs::system_trash(journal_path);
        panels.sandbox = fs::SandboxAccess::detect(index_path.as_deref());
        panels.remote = Arc::new(Pool::over_tcp(KnownHostsFiles {
            ssh: ssh_known_hosts,
            own: own_known_hosts,
        }));
        panels.passwords = passwords;
        panels.open_grant_dialog_if_needed();
        panels.ops.view_copies = view_copies_dir
            .as_deref()
            .and_then(|dir| fs::archive::ViewCopies::open(dir).ok());
        panels.apply_session_state(&saved);
        let (app, filing_start) = panels
            .with_config(loaded)
            .with_filing(&filing, index_path, key_store);
        (app, Task::batch([panel_loads, filing_start]))
    }

    /// Starts the filing module when `filing` switches it on. The database
    /// and its directory are not created here: only after the user chose at
    /// the encryption question. Switched on without locations, the module
    /// has nothing to work on, so the setup opens.
    pub fn with_filing(
        mut self,
        filing: &Filing,
        db_path: Option<PathBuf>,
        store: Arc<dyn KeyStore + Send + Sync>,
    ) -> (Self, Task<Message>) {
        self.hermes.index_path = db_path;
        self.hermes.key_store = Some(ncrs_hermes_ui::KeyStoreHandle(store));
        let task = self.start_filing_module(filing);
        if filing.enabled && filing.locations.is_empty() {
            self.open_hermes_setup();
        }
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
                self.theme_preset = config.theme_preset();
                self.vscode_theme = config.vscode_theme_path().map(str::to_owned);
                self.open_programs = config.open;
                self.saved_servers = config.servers;
                self.file_list_tags = config.file_list_tags;
                self.kinds = Kinds::new(config.kinds, self.lang.kind_language());
                self.keymap = config.keymap;
                self.refresh_function_keys();
                self.palette = config.palette;
                self.report_look_warnings(config.contrast_warning, config.vscode_warning);
                self.report_language_warning(config.language_warning);
                self.report_kind_warning(config.kind_warning);
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
        if let Some(module) = &mut self.hermes.filing {
            module.set_kind_language(lang.kind_language());
        }
        self.kinds = self.kinds.in_language(lang.kind_language());
        let kinds = self.kinds.clone();
        if let Some(list) = &mut self.hermes.review_list {
            list.set_kinds(kinds.clone());
        }
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
        cursors: [Option<String>; 2],
    ) -> (Self, Task<Message>) {
        let [left_cursor, right_cursor] = cursors;
        let mut app = Self {
            panels: Panels::new(
                PanelState::new(left.clone()),
                PanelState::new(right.clone()),
                PanelSide::Left,
            ),
            visible_rows: layout::visible_rows(layout::INITIAL_WINDOW_SIZE),
            lang: Language::default(),
            function_keys: Keymap::built_in().function_keys(Language::default(), KeyScope::Panel),
            search_function_keys: Keymap::built_in()
                .function_keys(Language::default(), KeyScope::Search),
            keymap: Keymap::built_in(),
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
            state_path: None,
            session_scheduled: SessionState::default(),
            session_save_generation: 0,
            session_writes: Arc::default(),
            input: Arc::default(),
            list_volumes: Arc::new(fs::list_volumes),
            search: ncrs_search::Search::new(layout::INITIAL_WINDOW_SIZE),
            quick_open: None,
            quick_open_walk_generation: 0,
            quick_open_walk: None,
            command_palette: None,
            recent_commands: Vec::new(),
            wall_clock: Arc::new(ncrs_pipeline::SystemClock),
            config_path: None,
            file_list_tags: FileListTags::default(),
            kinds: Kinds::default(),
            hermes_flow_step: 0,
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
            hermes_focus: HermesFocus::default(),
            fetch_extension: open_vsx::fetch_extension,
            install_theme: ncrs_settings::theme_fetch::install::install_picked_theme,
            about: None,
            find_third_party_licenses: crate::about::installed_third_party_licenses,
            app_version: env!("CARGO_PKG_VERSION"),
            download_ocr_models: download_models,
            download_ai_model,
            grant_dialog: false,
            sandbox: fs::SandboxAccess::detect(None),
            remote: Arc::new(Pool::over_tcp(KnownHostsFiles::default())),
            passwords: Arc::new(KeyringPasswords::new()),
            remote_ui: ncrs_remote::Remote::default(),
            saved_servers: Vec::new(),
        };

        let tasks = Task::batch([
            app.load(PanelSide::Left, left, left_cursor),
            app.load(PanelSide::Right, right, right_cursor),
        ]);
        (app, tasks)
    }
}
