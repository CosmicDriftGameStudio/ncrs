//! The suggestions of the AI for the scheme of a target and for a rule: what
//! the dialogs show and which of it is ticked. Nothing here is saved before
//! the user takes it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_filing::sorting::{RuleForFile, SuggestError, Suggested, Suggestions};
use ncrs_index::TargetId;
use ncrs_intake::SortSource;

/// What the dialog was opened for.
#[derive(Debug, Clone, PartialEq)]
pub enum SuggestFor {
    /// A scheme (the target has no rules) or the missing rules of a target.
    Target(TargetId),
    /// The one rule a file without a rule needs.
    File { source: SortSource, path: PathBuf },
}

/// What the background work found.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    Target(Suggestions),
    File(Box<RuleForFile>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestPhase {
    Running,
    Failed(SuggestError),
    Done,
}

/// A proposal that is in the rule form: the dialog waits for how it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    pub at: usize,
    /// The rules of the target when the form opened; known once the list is
    /// loaded.
    pub rules_before: Option<usize>,
}

#[derive(Debug)]
pub struct Suggest {
    pub subject: SuggestFor,
    /// The pattern the user asked for, tried before any other.
    pub wish: Option<String>,
    pub phase: SuggestPhase,
    pub found: Option<Found>,
    /// One tick per proposed rule.
    pub ticks: Vec<bool>,
    pub cursor: usize,
    /// The n-th other proposal is asked for.
    pub again: usize,
    /// Answers of an older run or of a closed dialog are dropped.
    pub generation: u64,
    pub stop: Arc<AtomicBool>,
    /// The index is being opened; the work starts once it is.
    pub waits_for_index: bool,
    /// The ticked rules are being saved.
    pub saving: bool,
    pub problem: Option<String>,
    pub editing: Option<Editing>,
}

impl Default for Suggest {
    fn default() -> Self {
        Self::new(SuggestFor::Target(TargetId::main()), 0)
    }
}

/// Taken from one counter, so the late answer of a dialog that was closed
/// never fits the generation of the next one.
static LAST_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Suggest {
    pub fn new(subject: SuggestFor, again: usize) -> Self {
        Self {
            subject,
            wish: None,
            phase: SuggestPhase::Running,
            found: None,
            ticks: Vec::new(),
            cursor: 0,
            again,
            generation: LAST_GENERATION.fetch_add(1, Ordering::Relaxed) + 1,
            stop: Arc::new(AtomicBool::new(false)),
            waits_for_index: false,
            saving: false,
            problem: None,
            editing: None,
        }
    }

    /// Ends the work in the background; its answer is dropped.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Starts over with another proposal: the old work ends, the new one has
    /// its own generation and stop flag.
    pub fn restart(&mut self) {
        self.stop();
        let again = self.again + 1;
        *self = Self {
            wish: self.wish.take(),
            ..Self::new(self.subject.clone(), again)
        };
    }

    pub fn loaded(&mut self, found: Found) {
        let count = match &found {
            Found::Target(suggestions) => suggestions.items.len(),
            Found::File(_) => 1,
        };
        self.ticks = vec![true; count];
        self.cursor = 0;
        self.found = Some(found);
        self.phase = SuggestPhase::Done;
    }

    pub fn failed(&mut self, error: SuggestError) {
        self.phase = SuggestPhase::Failed(error);
    }

    pub fn items(&self) -> &[Suggested] {
        match &self.found {
            Some(Found::Target(suggestions)) => &suggestions.items,
            _ => &[],
        }
    }

    /// The proposals for the rule dialog of a file: just its one rule.
    pub fn file_rule(&self) -> Option<&RuleForFile> {
        match &self.found {
            Some(Found::File(found)) => Some(found),
            _ => None,
        }
    }

    pub fn ticked(&self) -> Vec<Suggested> {
        self.picked(true)
    }

    pub fn unticked(&self) -> Vec<Suggested> {
        self.picked(false)
    }

    fn picked(&self, ticked: bool) -> Vec<Suggested> {
        self.items()
            .iter()
            .zip(&self.ticks)
            .filter(|(_, tick)| **tick == ticked)
            .map(|(item, _)| item.clone())
            .collect()
    }

    pub fn toggle(&mut self, at: usize) {
        if let Some(tick) = self.ticks.get_mut(at) {
            *tick = !*tick;
            self.cursor = at;
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let last = self.ticks.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
    }

    /// The target the proposals are for; for a file it is known once the
    /// rule was found.
    pub fn target(&self) -> Option<TargetId> {
        match (&self.found, &self.subject) {
            (Some(Found::File(found)), _) => Some(found.target.clone()),
            (_, SuggestFor::Target(target)) => Some(target.clone()),
            (_, SuggestFor::File { .. }) => None,
        }
    }

    /// Drops a proposal the user took into the rule form.
    pub fn remove(&mut self, at: usize) {
        if let Some(Found::Target(suggestions)) = self.found.as_mut() {
            if at < suggestions.items.len() {
                suggestions.items.remove(at);
                self.ticks.remove(at);
                self.move_cursor(0);
            }
        }
    }

    /// Whether the target had rules before: then the dialog asks for the
    /// ones that are missing instead of a whole scheme.
    pub fn has_rules(&self) -> bool {
        matches!(&self.found, Some(Found::Target(found)) if found.rules_existing > 0)
    }

    pub fn ticked_count(&self) -> usize {
        self.ticks.iter().filter(|tick| **tick).count()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestMessage {
    Loaded {
        generation: u64,
        result: Result<Box<Found>, SuggestError>,
    },
    Toggle(usize),
    CursorUp,
    CursorDown,
    /// Space: ticks or unticks the proposal under the cursor.
    ToggleCursor,
    /// "Ändern": the proposal goes into the rule form, unsaved.
    Edit(usize),
    /// "Verwerfen": the user turns the proposal down for good.
    Reject(usize),
    Rejected {
        generation: u64,
        result: Result<(), String>,
    },
    /// "Anders vorschlagen".
    Again,
    /// "n Regeln übernehmen" and "Regel anlegen", also Enter.
    Adopt,
    Adopted {
        generation: u64,
        result: Result<(), String>,
    },
    /// Esc and "liegen lassen".
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suggested(name: &str) -> Suggested {
        Suggested {
            name: name.to_owned(),
            spec: ncrs_index::RuleSpec {
                name: None,
                conditions: ncrs_index::RuleConditions::all_of(Vec::new()),
                extract_number: None,
                extract_date: None,
                extract_amount: None,
                kind: None,
                tags: std::collections::BTreeSet::new(),
                filing_pattern: String::new(),
                confirmed: false,
                enabled: true,
            },
            example: None,
            files: 3,
            group_documents: 3,
            elsewhere: 0,
        }
    }

    fn done(names: &[&str]) -> Suggest {
        let mut state = Suggest::new(SuggestFor::Target(TargetId::main()), 0);
        state.loaded(Found::Target(Suggestions {
            files_read: 9,
            sources_read: 1,
            rules_existing: 0,
            fit_suggested: 9,
            fit_none: 0,
            items: names.iter().map(|name| suggested(name)).collect(),
            model: ncrs_filing::sorting::ModelNote::NotAvailable,
        }));
        state
    }

    #[test]
    fn everything_is_ticked_at_first_and_unticking_splits_the_taken_from_the_rest() {
        let mut state = done(&["a", "b", "c"]);
        state.toggle(1);

        let taken: Vec<_> = state.ticked().into_iter().map(|item| item.name).collect();
        let rest: Vec<_> = state.unticked().into_iter().map(|item| item.name).collect();

        assert_eq!((taken, rest), (vec!["a".into(), "c".into()], vec!["b".into()]));
        assert_eq!(state.ticked_count(), 2);
    }

    #[test]
    fn the_cursor_stays_inside_the_list_and_a_removed_row_keeps_it_valid() {
        let mut state = done(&["a", "b"]);
        state.move_cursor(5);
        assert_eq!(state.cursor, 1);
        state.move_cursor(-5);
        assert_eq!(state.cursor, 0);
        state.move_cursor(1);
        state.remove(1);
        assert_eq!((state.cursor, state.items().len(), state.ticks.len()), (0, 1, 1));
    }

    #[test]
    fn asking_again_gets_a_new_generation_and_ends_the_old_work() {
        let mut state = done(&["a"]);
        let (generation, stop) = (state.generation, Arc::clone(&state.stop));

        state.restart();

        assert_ne!(state.generation, generation);
        assert!(stop.load(Ordering::Relaxed));
        assert_eq!((state.again, &state.phase), (1, &SuggestPhase::Running));
    }
}
