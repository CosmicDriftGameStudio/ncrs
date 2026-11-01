use ncrs_config::keymap::KeyAction;
use ncrs_config::SettingsFeature;

/// What the command list asks of the app, which it cannot do itself. The app
/// runs it right after `update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandPaletteAction {
    /// The list opened: close the context menu.
    CloseContextMenu,
    /// Enter on a command, after the list closed: do what its key would, and
    /// count it among the recent commands.
    Run(KeyAction),
    /// Enter on a setting, after the list closed: open the settings tab on
    /// `feature`, with `row` in view when there is one.
    ShowSetting {
        feature: SettingsFeature,
        row: Option<String>,
    },
}
