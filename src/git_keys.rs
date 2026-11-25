//! The keys of the Git tab as the config asks for them: `[keys.git]` read by
//! `ncrs-config`, named and checked here against the Git actions and the keys
//! of the rest of the app.

use ncrs_config::keymap::{Binding, KeyAction, Keymap, TabKeys};
use ncrs_git_view::shortcuts::Shortcut;
use ncrs_git_view::{GitKeys, KeyOverride};

/// Where in the file and why `[keys.git]` cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitKeysProblem {
    pub line: Option<usize>,
    pub reason: String,
}

/// The Git keys for `overrides` of `[keys.git]`, built on the keys of the
/// platform `mac`. A key that an action of the app also holds is refused: the
/// Git tab would shadow it, or the app would never see it.
pub fn resolve(overrides: &TabKeys, keymap: &Keymap, mac: bool) -> Result<GitKeys, GitKeysProblem> {
    let entries: Vec<KeyOverride> = overrides
        .entries
        .iter()
        .map(|entry| KeyOverride {
            action: entry.action.clone(),
            chords: entry
                .bindings
                .iter()
                .map(|binding| (binding.modifiers, binding.key.clone()))
                .collect(),
        })
        .collect();
    let line_of = |place: usize| overrides.entries.get(place)?.line;
    let keys = GitKeys::with_overrides(&entries, mac).map_err(|err| GitKeysProblem {
        line: line_of(err.place),
        reason: err.reason,
    })?;
    if let Some((shortcut, action)) = clashes(&keys, keymap.bindings()).into_iter().next() {
        let place = entries
            .iter()
            .position(|entry| entry.action == shortcut.name());
        return Err(GitKeysProblem {
            line: place.and_then(line_of),
            reason: format!("also bound to {} in [keys]", action.name()),
        });
    }
    Ok(keys)
}

/// The Git actions whose key is also the key of an action of the app.
pub fn clashes(
    keys: &GitKeys,
    app_bindings: &[(KeyAction, Binding)],
) -> Vec<(Shortcut, KeyAction)> {
    let mut found = Vec::new();
    for shortcut in Shortcut::ALL {
        if shortcut.shares_an_app_key() {
            continue;
        }
        for (modifiers, key) in keys.chords(*shortcut) {
            for (action, binding) in app_bindings {
                if binding.modifiers == *modifiers && same_key(&binding.key, key) {
                    found.push((*shortcut, *action));
                }
            }
        }
    }
    found
}

fn same_key(left: &iced::keyboard::Key, right: &iced::keyboard::Key) -> bool {
    use iced::keyboard::Key;
    match (left, right) {
        (Key::Character(a), Key::Character(b)) => a.eq_ignore_ascii_case(b),
        _ => left == right,
    }
}
