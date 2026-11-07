use crate::app::*;
use ncrs_updates::Action as Ask;

impl App {
    pub(in crate::app) fn update_updates(
        &mut self,
        message: ncrs_updates::Message,
    ) -> Task<Message> {
        let state_file = self.state_path.as_deref().map(ncrs_updates::file_beside);
        let now_unix = self
            .wall_clock
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        let ctx = ncrs_updates::Ctx {
            enabled: self.update_check,
            now_unix,
            fetch: self.fetch_latest_release,
            state_file: state_file.as_deref(),
        };
        let (check, asked) = ncrs_updates::update(&mut self.updates, message, &ctx);
        let task = check.map(Message::Updates);
        match asked {
            Some(Ask::OpenUrl(url)) => {
                self.open_release_page(&url);
                task
            }
            Some(Ask::SetCheckEnabled(enabled)) => Task::batch([
                task,
                self.update_settings(SettingsMessage::Change(SettingChange::CheckForUpdates(
                    enabled,
                ))),
            ]),
            None => task,
        }
    }

    fn open_release_page(&mut self, url: &str) {
        let command = fs::system_open_command(std::ffi::OsString::from(url));
        if let Err(reason) = self.launcher.launch(&command) {
            let failure = self
                .lang
                .text(Msg::ErrorOpenFailed)
                .replace("{name}", url)
                .replace("{reason}", &reason);
            self.set_status_error(&failure);
        }
    }

    /// The press on the version in the status bar: the Updates page, or the
    /// About card where this build makes no update check.
    pub(in crate::app) fn open_version_info(&mut self) -> Task<Message> {
        if ncrs_updates::CHECK_AVAILABLE {
            self.show_setting(crate::messages::SettingsFeature::Updates, None)
        } else {
            self.update_about(AboutMessage::Open)
        }
    }

    /// The Updates page of the settings tab.
    pub(in crate::app) fn updates_page(&self) -> Element<'_, Message> {
        ncrs_updates::view(
            &self.updates,
            ncrs_updates::ViewCtx {
                lang: self.lang,
                palette: &self.palette,
                enabled: self.updates.is_enabled(self.update_check),
            },
        )
        .map(Message::Updates)
    }

    /// Whether the update check runs: always false where this build makes no
    /// request.
    pub(in crate::app) fn update_check_runs(&self) -> bool {
        ncrs_updates::CHECK_AVAILABLE && self.updates.is_enabled(self.update_check)
    }

    /// The version a dot in the status bar points at.
    pub(in crate::app) fn newer_version_text(&self) -> Option<String> {
        if !self.update_check_runs() {
            return None;
        }
        self.updates
            .newer_release()
            .map(|release| release.version.to_string())
    }
}
