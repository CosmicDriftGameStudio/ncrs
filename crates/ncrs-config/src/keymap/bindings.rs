use super::*;

/// Why `[keys]` cannot be applied, and where in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapError {
    pub span: Range<usize>,
    pub reason: String,
}

/// Which keys do what: the built-in table, or that table with the `[keys]`
/// section of the config applied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Keymap {
    bindings: Vec<(KeyAction, Binding)>,
}

impl Default for Keymap {
    fn default() -> Self {
        (*Self::built_in()).clone()
    }
}

impl Keymap {
    /// The built-in table, built once on first use.
    ///
    /// `map_key` runs on every key press, and a binding owns a `Key`, which
    /// owns a `SmolStr`. Building the list per press meant an allocation per
    /// key repeat; holding a cursor key down made the UI visibly lag behind
    /// the keystroke.
    pub fn built_in() -> Arc<Self> {
        static BUILT_IN: OnceLock<Arc<Keymap>> = OnceLock::new();
        Arc::clone(BUILT_IN.get_or_init(|| {
            Arc::new(Keymap {
                bindings: default_bindings(),
            })
        }))
    }

    /// The built-in table with the actions named in `overrides` rebound. A
    /// named action loses all its built-in keys and gets exactly the listed
    /// ones; `[]` leaves it without a key. Two actions on one key are an
    /// error when both work in the same area (panel or search), also when the
    /// other one only has its built-in key: silently letting one win would
    /// hide the other action without a word. The exception is a built-in key
    /// that an older config may already use for something else
    /// ([`KeyAction::yields_its_built_in_key`]): the config wins and the
    /// action keeps the command list only.
    pub fn with_overrides(overrides: &KeySection) -> Result<Self, KeymapError> {
        let mut rebound: Vec<KeyAction> = Vec::new();
        for entry in &overrides.entries {
            if let Some(action) = KeyAction::from_name(entry.action.as_ref()) {
                rebound.push(action);
            }
        }

        let mut taken: Vec<(KeyAction, Binding)> = default_bindings()
            .into_iter()
            .filter(|(action, _)| !rebound.contains(action))
            .collect();
        let mut configured: Vec<(KeyAction, Vec<Binding>)> = Vec::new();
        let mut yielded: Vec<(KeyAction, Binding)> = Vec::new();
        for entry in &overrides.entries {
            let name = entry.action.as_ref();
            let action = KeyAction::from_name(name).ok_or_else(|| KeymapError {
                span: entry.action.span(),
                reason: format!("unknown action {name:?}"),
            })?;
            let mut bindings = Vec::new();
            for (index, chord) in entry.chords.iter().enumerate() {
                if index >= MAX_KEYS_PER_ACTION {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("at most {MAX_KEYS_PER_ACTION} keys per action"),
                    });
                }
                let text = chord.as_ref().trim();
                let binding = parse_chord(text).map_err(|reason| KeymapError {
                    span: chord.span(),
                    reason,
                })?;
                if bindings.contains(&binding) {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is listed twice for {name}"),
                    });
                }
                if !action.works_in(KeyScope::Panel) && is_fixed_in_search(&binding) {
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is fixed in the search view"),
                    });
                }
                let holder = taken
                    .iter()
                    .find(|(other, held)| *held == binding && shares_a_scope(*other, action))
                    .map(|(other, _)| *other);
                if let Some(other) = holder
                    .filter(|other| !rebound.contains(other) && other.yields_its_built_in_key())
                {
                    taken
                        .retain(|(held_action, held)| !(*held_action == other && *held == binding));
                    yielded.push((other, binding.clone()));
                } else if let Some(other) = holder {
                    let advice = if rebound.contains(&other) {
                        String::new()
                    } else {
                        format!("; give {} other keys too, or []", other.name())
                    };
                    return Err(KeymapError {
                        span: chord.span(),
                        reason: format!("{text:?} is already bound to {}{advice}", other.name()),
                    });
                }
                taken.push((action, binding.clone()));
                bindings.push(binding);
            }
            configured.push((action, bindings));
        }

        let mut bindings = Vec::new();
        let mut placed: Vec<KeyAction> = Vec::new();
        for (action, binding) in default_bindings() {
            if !rebound.contains(&action) {
                if !yielded.contains(&(action, binding.clone())) {
                    bindings.push((action, binding));
                }
            } else if !placed.contains(&action) {
                placed.push(action);
                for (configured_action, keys) in &configured {
                    if *configured_action == action {
                        bindings.extend(keys.iter().map(|key| (action, key.clone())));
                    }
                }
            }
        }
        // An action without a built-in key (on this platform) is not met in
        // the loop above; its configured keys go at the end.
        for (action, keys) in &configured {
            if !placed.contains(action) {
                bindings.extend(keys.iter().map(|key| (*action, key.clone())));
            }
        }
        Ok(Self { bindings })
    }

    /// Every binding, in the order that decides which one a label shows.
    pub fn bindings(&self) -> &[(KeyAction, Binding)] {
        &self.bindings
    }

    /// The action a key press triggers, among the actions that work in `scope`.
    pub fn action_for(
        &self,
        key: &Key,
        modifiers: Modifiers,
        scope: KeyScope,
    ) -> Option<KeyAction> {
        self.bindings
            .iter()
            .find(|(action, binding)| {
                action.works_in(scope) && binding.key == *key && binding.modifiers == modifiers
            })
            .map(|(action, _)| *action)
    }

    /// The keys of `action`, the first-listed first.
    pub fn bindings_of(&self, action: KeyAction) -> impl Iterator<Item = &Binding> {
        self.bindings
            .iter()
            .filter(move |(bound, _)| *bound == action)
            .map(|(_, binding)| binding)
    }

    /// Whether `action` has exactly the keys it ships with, in that order.
    pub fn has_built_in_keys(&self, action: KeyAction) -> bool {
        self.bindings_of(action)
            .eq(built_in_bindings_of(action).iter())
    }

    /// The other action that `binding` would collide with if `action` took
    /// it: one that holds it and works in an area `action` works in too.
    pub fn holder_of(&self, action: KeyAction, binding: &Binding) -> Option<KeyAction> {
        self.bindings
            .iter()
            .find(|(other, held)| {
                *other != action && held == binding && shares_a_scope(*other, action)
            })
            .map(|(other, _)| *other)
    }

    /// The labels of the function key bar, derived from the bindings: slot
    /// `n - 1` holds the hint of the bare `Fn` binding, `None` for a free key.
    /// Only bare function keys are shown; Shift+F8 and friends are in the help.
    /// Actions that do not work in `scope` leave their slot free.
    pub fn function_keys(
        &self,
        lang: Language,
        scope: KeyScope,
    ) -> [Option<&'static str>; FUNCTION_KEY_COUNT] {
        let mut slots = [None; FUNCTION_KEY_COUNT];
        for (action, binding) in &self.bindings {
            if !action.works_in(scope) {
                continue;
            }
            let Some(hint) = action.hint() else {
                continue;
            };
            if binding.modifiers != Modifiers::default() {
                continue;
            }
            let Some(number) = function_key_number(&binding.key) else {
                continue;
            };
            if let Some(slot) = slots.get_mut(number - 1) {
                slot.get_or_insert(lang.text(hint));
            }
        }
        slots
    }
}

pub use ncrs_ui_kit::layout::FUNCTION_KEY_COUNT;

/// The slot of F7, which makes a folder.
pub const NEW_FOLDER_SLOT: usize = 6;
