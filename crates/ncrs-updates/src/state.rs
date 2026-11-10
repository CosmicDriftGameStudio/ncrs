//! State, messages and `update` of the update check.

use std::path::Path;

use iced::Task;
use semver::Version;

use crate::channel::Channel;
use crate::fetch::FetchFn;
use crate::release::{base_version, checked_release, is_news, LatestRelease};
use crate::store::{self, Stored};

/// The App Store build makes no request.
const SANDBOXED: bool = cfg!(feature = "sandbox");

/// Whether this build can check at all; the settings page leaves its switch out
/// where it can not.
pub const CHECK_AVAILABLE: bool = !SANDBOXED;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The app asks for a check, at start and whenever the setting turns on.
    CheckDue,
    Checked(Result<LatestRelease, String>),
    SkipThisVersion,
    OpenReleaseNotes,
    /// The switch of the settings page.
    SetEnabled(bool),
    /// Sent by widgets that have to send something but do nothing.
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenUrl(String),
    /// The `check for updates` setting is to be written; the app owns it.
    SetCheckEnabled(bool),
}

#[derive(Debug)]
pub struct Ctx<'a> {
    /// The `check for updates` setting; `None` follows the channel.
    pub enabled: Option<bool>,
    pub now_unix: i64,
    pub fetch: FetchFn,
    /// Where the remembered check lives; `None` keeps it in memory only.
    pub state_file: Option<&'a Path>,
}

#[derive(Debug)]
pub struct State {
    display_version: String,
    channel: Channel,
    running: Option<Version>,
    stored: Stored,
    latest: Option<LatestRelease>,
    checking: bool,
}

impl State {
    /// `display_version` is the version the app shows (`1.2.0` or
    /// `1.2.0 (568c38e)`); `state_file` is read here once.
    pub fn new(display_version: &str, channel: Channel, state_file: Option<&Path>) -> Self {
        let stored = state_file.map(store::load).unwrap_or_default();
        let latest = match (&stored.found_version, &stored.found_notes_url) {
            (Some(version), Some(url)) => checked_release(version, url).ok(),
            _ => None,
        };
        Self {
            display_version: display_version.to_owned(),
            channel,
            running: base_version(display_version),
            stored,
            latest,
            checking: false,
        }
    }

    pub fn display_version(&self) -> &str {
        &self.display_version
    }

    pub const fn channel(&self) -> Channel {
        self.channel
    }

    pub fn is_enabled(&self, setting: Option<bool>) -> bool {
        setting.unwrap_or_else(|| self.channel.checks_by_default())
    }

    /// The newer release to tell the user about, if there is one right now.
    pub fn newer_release(&self) -> Option<&LatestRelease> {
        let running = self.running.as_ref()?;
        self.latest
            .as_ref()
            .filter(|release| is_news(release, running, self.stored.skipped_version.as_deref()))
    }

    fn should_check(&self, ctx: &Ctx<'_>) -> bool {
        !SANDBOXED
            && !self.checking
            && self.running.is_some()
            && self.is_enabled(ctx.enabled)
            && self.stored.is_due(ctx.now_unix)
    }

    fn save(&self, state_file: Option<&Path>) {
        if let Some(path) = state_file {
            store::save(path, &self.stored);
        }
    }
}

pub fn update(
    state: &mut State,
    message: Message,
    ctx: &Ctx<'_>,
) -> (Task<Message>, Option<Action>) {
    match message {
        Message::CheckDue => {
            if !state.should_check(ctx) {
                return (Task::none(), None);
            }
            // Stamped before the answer: a failed request waits a day as well.
            state.stored.last_checked_unix = Some(ctx.now_unix);
            state.save(ctx.state_file);
            state.checking = true;
            let fetch = ctx.fetch;
            let running = state
                .running
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            let task = Task::perform(
                tokio::task::spawn_blocking(move || fetch(&running)),
                |joined| Message::Checked(joined.unwrap_or_else(|join| Err(join.to_string()))),
            );
            (task, None)
        }
        Message::Checked(result) => {
            state.checking = false;
            match result {
                Ok(release) => {
                    state.stored.found_version = Some(release.version.to_string());
                    state.stored.found_notes_url = Some(release.notes_url.clone());
                    state.latest = Some(release);
                    state.save(ctx.state_file);
                }
                Err(err) => eprintln!("ncrs: update check failed: {err}"),
            }
            (Task::none(), None)
        }
        Message::SkipThisVersion => {
            if let Some(release) = &state.latest {
                state.stored.skipped_version = Some(release.version.to_string());
                state.save(ctx.state_file);
            }
            (Task::none(), None)
        }
        Message::Ignored => (Task::none(), None),
        Message::SetEnabled(enabled) => (Task::none(), Some(Action::SetCheckEnabled(enabled))),
        Message::OpenReleaseNotes => {
            let action = state
                .newer_release()
                .map(|release| Action::OpenUrl(release.notes_url.clone()));
            (Task::none(), action)
        }
    }
}

// reason: a failing assertion is the signal in a test
#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    /// The tests look at the state and the action, not at the task.
    fn update(state: &mut State, message: Message, ctx: &Ctx<'_>) -> Option<Action> {
        super::update(state, message, ctx).1
    }
    use crate::release::TRUSTED_NOTES_PREFIX;
    use crate::store::THROTTLE_SECONDS;

    fn release(version: &str) -> LatestRelease {
        LatestRelease {
            version: Version::parse(version).unwrap(),
            notes_url: format!("{TRUSTED_NOTES_PREFIX}tag/v{version}"),
        }
    }

    fn never_fetch(_app_version: &str) -> Result<LatestRelease, String> {
        Err("no network in tests".to_owned())
    }

    fn ctx(enabled: Option<bool>, now_unix: i64, file: Option<&Path>) -> Ctx<'_> {
        Ctx {
            enabled,
            now_unix,
            fetch: never_fetch,
            state_file: file,
        }
    }

    fn direct() -> State {
        State::new("1.2.0", Channel::Direct, None)
    }

    fn within_runtime(test: impl FnOnce()) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        test();
    }

    #[cfg(not(feature = "sandbox"))]
    #[test]
    fn a_due_check_starts_and_is_stamped() {
        within_runtime(|| {
            let mut state = direct();
            update(&mut state, Message::CheckDue, &ctx(None, 500, None));
            assert!(state.checking);
            assert_eq!(state.stored.last_checked_unix, Some(500));
        });
    }

    #[test]
    fn a_check_that_is_off_or_throttled_does_not_start() {
        within_runtime(|| {
            let mut state = direct();
            update(&mut state, Message::CheckDue, &ctx(Some(false), 500, None));
            assert!(!state.checking);

            state.stored.last_checked_unix = Some(500);
            update(
                &mut state,
                Message::CheckDue,
                &ctx(Some(true), 500 + THROTTLE_SECONDS - 1, None),
            );
            assert!(!state.checking);
        });
    }

    #[test]
    fn packages_and_development_builds_check_only_when_asked() {
        within_runtime(|| {
            let mut apt = State::new(
                "1.2.0",
                Channel::PackageManager(crate::PackageManager::Apt),
                None,
            );
            update(&mut apt, Message::CheckDue, &ctx(None, 0, None));
            assert!(!apt.checking);
            let mut dev = State::new("1.2.0 (abc)", Channel::Development, None);
            update(&mut dev, Message::CheckDue, &ctx(None, 0, None));
            assert!(!dev.checking);
            assert!(dev.is_enabled(Some(true)));
        });
    }

    #[test]
    fn the_switch_asks_the_app_to_write_the_setting() {
        let mut state = direct();
        assert_eq!(
            update(&mut state, Message::SetEnabled(false), &ctx(None, 0, None)),
            Some(Action::SetCheckEnabled(false))
        );
    }

    #[test]
    fn a_failed_check_shows_nothing() {
        let mut state = direct();
        state.checking = true;
        update(
            &mut state,
            Message::Checked(Err("offline".to_owned())),
            &ctx(None, 0, None),
        );
        assert!(!state.checking);
        assert!(state.newer_release().is_none());
    }

    #[test]
    fn a_newer_release_is_offered_and_its_notes_open() {
        let mut state = direct();
        update(
            &mut state,
            Message::Checked(Ok(release("1.3.0"))),
            &ctx(None, 0, None),
        );
        assert_eq!(
            state.newer_release().map(|r| r.version.to_string()),
            Some("1.3.0".into())
        );
        let action = update(&mut state, Message::OpenReleaseNotes, &ctx(None, 0, None));
        assert_eq!(
            action,
            Some(Action::OpenUrl(format!("{TRUSTED_NOTES_PREFIX}tag/v1.3.0")))
        );
    }

    #[test]
    fn the_running_release_is_not_news_and_a_dev_build_compares_by_its_base() {
        let mut state = State::new("1.2.0 (568c38e)", Channel::Development, None);
        update(
            &mut state,
            Message::Checked(Ok(release("1.2.0"))),
            &ctx(None, 0, None),
        );
        assert!(state.newer_release().is_none());
        update(
            &mut state,
            Message::Checked(Ok(release("1.2.1"))),
            &ctx(None, 0, None),
        );
        assert!(state.newer_release().is_some());
    }

    #[test]
    fn skipping_silences_that_version_across_restarts_but_not_a_newer_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.toml");
        let mut state = State::new("1.2.0", Channel::Direct, Some(&file));
        update(
            &mut state,
            Message::Checked(Ok(release("1.3.0"))),
            &ctx(None, 0, Some(&file)),
        );
        update(
            &mut state,
            Message::SkipThisVersion,
            &ctx(None, 0, Some(&file)),
        );
        assert!(state.newer_release().is_none());

        let mut restarted = State::new("1.2.0", Channel::Direct, Some(&file));
        update(
            &mut restarted,
            Message::Checked(Ok(release("1.3.0"))),
            &ctx(None, 0, None),
        );
        assert!(restarted.newer_release().is_none());
        update(
            &mut restarted,
            Message::Checked(Ok(release("1.3.1"))),
            &ctx(None, 0, None),
        );
        assert!(restarted.newer_release().is_some());
    }

    #[test]
    fn a_found_release_survives_a_restart_until_the_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.toml");
        let mut state = State::new("1.2.0", Channel::Direct, Some(&file));
        update(
            &mut state,
            Message::Checked(Ok(release("1.3.0"))),
            &ctx(None, 0, Some(&file)),
        );

        let restarted = State::new("1.2.0", Channel::Direct, Some(&file));
        assert_eq!(
            restarted.newer_release().map(|r| r.notes_url.clone()),
            Some(format!("{TRUSTED_NOTES_PREFIX}tag/v1.3.0"))
        );
        let upgraded = State::new("1.3.0", Channel::Direct, Some(&file));
        assert!(upgraded.newer_release().is_none());
    }

    #[test]
    fn a_stored_release_with_a_foreign_link_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("updates.toml");
        std::fs::write(
            &file,
            "found_version = \"1.3.0\"\nfound_notes_url = \"https://evil.example/x\"\n",
        )
        .unwrap();
        let state = State::new("1.2.0", Channel::Direct, Some(&file));
        assert!(state.newer_release().is_none());
    }

    #[cfg(not(feature = "sandbox"))]
    #[test]
    fn the_last_check_survives_a_restart() {
        within_runtime(|| {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("updates.toml");
            let mut state = State::new("1.2.0", Channel::Direct, Some(&file));
            update(&mut state, Message::CheckDue, &ctx(None, 900, Some(&file)));

            let mut restarted = State::new("1.2.0", Channel::Direct, Some(&file));
            update(
                &mut restarted,
                Message::CheckDue,
                &ctx(None, 901, Some(&file)),
            );
            assert!(!restarted.checking);
        });
    }

    #[cfg(feature = "sandbox")]
    #[test]
    fn the_sandboxed_build_never_checks() {
        within_runtime(|| {
            let mut state = direct();
            update(&mut state, Message::CheckDue, &ctx(Some(true), 0, None));
            assert!(!state.checking);
        });
    }
}
