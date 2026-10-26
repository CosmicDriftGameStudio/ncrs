//! The folders a panel was in before the current one: Alt+Left goes back,
//! Alt+Right forward again, as in a browser.
use std::path::PathBuf;

/// Per stack; the oldest entry is dropped beyond it.
pub const HISTORY_LIMIT: usize = 50;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelHistory {
    /// Oldest first; the last entry is where Back goes.
    back: Vec<PathBuf>,
    /// Where Forward goes is the last entry.
    forward: Vec<PathBuf>,
}

impl PanelHistory {
    /// Restores both stacks from a saved session, capped again: the file may
    /// have been edited or written by a build with a larger limit.
    pub fn restored(back: Vec<PathBuf>, forward: Vec<PathBuf>) -> Self {
        let mut history = Self { back, forward };
        cap(&mut history.back);
        cap(&mut history.forward);
        history
    }

    pub fn back(&self) -> &[PathBuf] {
        &self.back
    }

    pub fn forward(&self) -> &[PathBuf] {
        &self.forward
    }

    /// The panel moved on from `previous` to a folder of its own choosing,
    /// which ends whatever Forward would have repeated.
    pub fn visit(&mut self, previous: PathBuf) {
        self.back.push(previous);
        cap(&mut self.back);
        self.forward.clear();
    }

    /// The folder Back goes to; `current` becomes what Forward returns to.
    pub fn step_back(&mut self, current: PathBuf) -> Option<PathBuf> {
        let target = self.back.pop()?;
        self.forward.push(current);
        cap(&mut self.forward);
        Some(target)
    }

    /// The mirror of [`Self::step_back`].
    pub fn step_forward(&mut self, current: PathBuf) -> Option<PathBuf> {
        let target = self.forward.pop()?;
        self.back.push(current);
        cap(&mut self.back);
        Some(target)
    }
}

fn cap(stack: &mut Vec<PathBuf>) {
    if stack.len() > HISTORY_LIMIT {
        stack.drain(..stack.len() - HISTORY_LIMIT);
    }
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        PathBuf::from("/").join(name)
    }

    #[test]
    fn back_and_forward_walk_the_visits_in_order() {
        let mut history = PanelHistory::default();
        history.visit(folder("a"));
        history.visit(folder("b"));

        // The panel is in "c" now.
        let first = history.step_back(folder("c"));
        assert_eq!(first, Some(folder("b")));
        let second = history.step_back(folder("b"));
        assert_eq!(second, Some(folder("a")));
        assert_eq!(history.step_back(folder("a")), None);

        assert_eq!(history.step_forward(folder("a")), Some(folder("b")));
        assert_eq!(history.step_forward(folder("b")), Some(folder("c")));
        assert_eq!(history.step_forward(folder("c")), None);
    }

    #[test]
    fn a_new_visit_clears_forward() {
        let mut history = PanelHistory::default();
        history.visit(folder("a"));
        history.step_back(folder("b"));
        assert_eq!(history.forward(), [folder("b")]);

        history.visit(folder("a"));

        assert!(history.forward().is_empty());
    }

    #[test]
    fn each_stack_keeps_only_the_newest_entries() {
        let mut history = PanelHistory::default();
        for number in 0..HISTORY_LIMIT + 5 {
            history.visit(folder(&number.to_string()));
        }
        assert_eq!(history.back().len(), HISTORY_LIMIT);
        assert_eq!(history.back()[0], folder("5"));

        let restored = PanelHistory::restored(
            (0..HISTORY_LIMIT * 2)
                .map(|n| folder(&n.to_string()))
                .collect(),
            Vec::new(),
        );
        assert_eq!(restored.back().len(), HISTORY_LIMIT);
        assert_eq!(restored.back()[0], folder(&HISTORY_LIMIT.to_string()));
    }
}
