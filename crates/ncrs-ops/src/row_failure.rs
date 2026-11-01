//! Why one row of a transfer or the write of an archive failed.

use ncrs_fs as fs;

/// Why one row of a transfer failed.
///
/// The failure kind is carried across the task boundary rather than formatted
/// into the message. It used to be a plain `String`, and the app told "the name
/// is taken" from a real error by looking for `AlreadyExists` at the start of
/// that string — which never matches, because `io::Error`'s `Display` is the
/// *message*, not the kind's name. Every conflict therefore fell through as a
/// hard error and the overwrite dialog was unreachable.
///
/// A typed failure is the fix that cannot rot: a wording change in the
/// filesystem layer or on the operating system leaves this matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowFailure {
    /// The target already exists, and the user can say what to do about it.
    Conflict(String),
    /// A 7z archive ncrs refuses to unpack; the text is chosen in the user's
    /// language when it is shown, not when the copy fails.
    Refused(fs::archive::SevenZipRefusal),
    /// A server sent a name that cannot be created here; the text is chosen in
    /// the user's language when it is shown.
    UnsafeName(String),
    /// Anything else, shown as it came.
    Other(String),
}

impl RowFailure {
    pub fn from_io(err: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let unsafe_name = err
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<fs::remote::transfer::UnsafeName>())
            .map(|named| named.0.clone());
        if let Some(name) = unsafe_name {
            RowFailure::UnsafeName(name)
        } else if err.kind() == ErrorKind::AlreadyExists {
            RowFailure::Conflict(err.to_string())
        } else if let Some(refusal) = fs::archive::refusal_of(&err) {
            RowFailure::Refused(refusal)
        } else {
            RowFailure::Other(err.to_string())
        }
    }
}
