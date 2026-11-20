//! Asks the person at the computer to prove it is them before something
//! private is opened: Touch ID (or the Mac password) on macOS, Windows Hello
//! on Windows. Linux has no such system call; there the check passes and the
//! credential store of the desktop does the asking.
//!
//! The check is a trait so tests never reach the real prompt.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Mutex, PoisonError};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows_hello;

/// What the system asks the person for, so a button can say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Nothing is asked here; the credential store may ask by itself.
    Nothing,
    TouchId,
    WindowsHello,
}

/// Why the person was not verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    /// The person closed the prompt.
    Cancelled,
    /// The system could not ask, or the answer was no; the system's own words.
    Failed(String),
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Failed(reason) => f.write_str(reason),
        }
    }
}

pub trait UserVerifier: fmt::Debug + Send + Sync {
    fn check(&self) -> Check;
    /// Blocks until the person answered: run it off the UI thread. `reason`
    /// is the line the system prompt shows.
    fn verify(&self, reason: &str) -> Result<(), Denied>;
}

/// The verification of the operating system this runs on.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemVerifier;

impl UserVerifier for SystemVerifier {
    fn check(&self) -> Check {
        if cfg!(target_os = "macos") {
            Check::TouchId
        } else if cfg!(target_os = "windows") {
            Check::WindowsHello
        } else {
            Check::Nothing
        }
    }

    fn verify(&self, reason: &str) -> Result<(), Denied> {
        platform_verify(reason)
    }
}

#[cfg(target_os = "macos")]
fn platform_verify(reason: &str) -> Result<(), Denied> {
    macos::verify(reason)
}

#[cfg(target_os = "windows")]
fn platform_verify(reason: &str) -> Result<(), Denied> {
    windows_hello::verify(reason)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_verify(_reason: &str) -> Result<(), Denied> {
    Ok(())
}

/// Asks nothing and lets everyone in.
#[derive(Debug, Default, Clone, Copy)]
pub struct Unchecked;

impl UserVerifier for Unchecked {
    fn check(&self) -> Check {
        Check::Nothing
    }

    fn verify(&self, _reason: &str) -> Result<(), Denied> {
        Ok(())
    }
}

/// A verifier for tests: says `check`, answers with the queued outcomes in
/// order and lets everyone in once the queue is empty.
#[derive(Debug)]
pub struct Scripted {
    check: Check,
    outcomes: Mutex<VecDeque<Result<(), Denied>>>,
    asked: Mutex<Vec<String>>,
}

impl Scripted {
    pub fn new(check: Check) -> Self {
        Self {
            check,
            outcomes: Mutex::new(VecDeque::new()),
            asked: Mutex::new(Vec::new()),
        }
    }

    pub fn answering(self, outcome: Result<(), Denied>) -> Self {
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(outcome);
        self
    }

    /// The reasons the prompt was asked with, oldest first.
    pub fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl UserVerifier for Scripted {
    fn check(&self) -> Check {
        self.check
    }

    fn verify(&self, reason: &str) -> Result<(), Denied> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(reason.to_owned());
        self.outcomes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scripted_verifier_answers_in_order_and_then_lets_in() {
        let verifier = Scripted::new(Check::TouchId)
            .answering(Err(Denied::Cancelled))
            .answering(Err(Denied::Failed("no".to_owned())));
        assert_eq!(verifier.verify("a"), Err(Denied::Cancelled));
        assert_eq!(verifier.verify("b"), Err(Denied::Failed("no".to_owned())));
        assert_eq!(verifier.verify("c"), Ok(()));
        assert_eq!(verifier.asked(), ["a", "b", "c"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_verifier_answers_check_on_this_mac_without_asking_anyone() {
        let name = match SystemVerifier.check() {
            Check::Nothing => "Nothing",
            Check::TouchId => "TouchId",
            Check::WindowsHello => "WindowsHello",
        };
        eprintln!("SystemVerifier::check() on this Mac: {name}");
    }

    #[test]
    fn the_system_verifier_names_what_this_os_asks() {
        let expected = if cfg!(target_os = "macos") {
            Check::TouchId
        } else if cfg!(target_os = "windows") {
            Check::WindowsHello
        } else {
            Check::Nothing
        };
        assert_eq!(SystemVerifier.check(), expected);
    }
}
