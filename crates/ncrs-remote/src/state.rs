use crate::dialog::ConnectDialog;

/// What only the connect feature holds: the dialog and the count of attempts,
/// which tells an answer to the current attempt from one to an older attempt.
#[derive(Debug, Default)]
pub struct Remote {
    pub dialog: Option<ConnectDialog>,
    pub attempts: u64,
}

impl Remote {
    pub fn next_attempt(&mut self) -> u64 {
        self.attempts = self.attempts.wrapping_add(1);
        self.attempts
    }
}
