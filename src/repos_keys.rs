//! The keys of the Repos tab as the config asks for them: `[keys.repos]` read by
//! `ncrs-config`, named and checked here against the Repos actions and the keys
//! of the rest of the app.

use ncrs_config::keymap::{Binding, KeyAction, Keymap, TabKeys};
use ncrs_repos::Shortcut;
use ncrs_repos::{KeyOverride, ReposKeys};

/// Where in the file and why `[keys.repos]` cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReposKeysProblem {
    pub line: Option<usize>,
    pub reason: String,
}

/// The Repos keys for `overrides` of `[keys.repos]`, built on the keys of the
/// platform `mac`. A key that an action of the app also holds is refused: the
/// Repos tab would shadow it, or the app would never see it.
pub fn resolve(overrides: &TabKeys, keymap: &Keymap, mac: bool) -> Result<ReposKeys, ReposKeysProblem> {
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
    let keys = ReposKeys::with_overrides(&entries, mac).map_err(|err| ReposKeysProblem {
        line: line_of(err.place),
        reason: err.reason,
    })?;
    if let Some((shortcut, action)) = clashes(&keys, keymap.bindings()).into_iter().next() {
        let place = entries
            .iter()
            .position(|entry| entry.action == shortcut.name());
        return Err(ReposKeysProblem {
            line: place.and_then(line_of),
            reason: format!("also bound to {} in [keys]", action.name()),
        });
    }
    Ok(keys)
}

/// The Repos actions whose key is also the key of an action of the app.
pub fn clashes(
    keys: &ReposKeys,
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
