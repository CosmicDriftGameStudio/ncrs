//! The reason of a failed file operation in the user's language. The system's
//! own wording is English on macOS and Linux whatever the app speaks, so the
//! usual kinds are told here and only an unknown kind keeps the system's text.

use std::io::ErrorKind;

use ncrs_i18n::{Language, Msg};

pub fn io_reason_text(kind: ErrorKind, system_text: &str, lang: Language) -> String {
    let known = match kind {
        ErrorKind::NotFound => Msg::IoReasonNotFound,
        ErrorKind::PermissionDenied => Msg::IoReasonPermissionDenied,
        ErrorKind::AlreadyExists => Msg::IoReasonAlreadyExists,
        ErrorKind::StorageFull => Msg::IoReasonStorageFull,
        ErrorKind::ReadOnlyFilesystem => Msg::IoReasonReadOnly,
        ErrorKind::TimedOut => Msg::IoReasonTimedOut,
        ErrorKind::DirectoryNotEmpty => Msg::IoReasonDirectoryNotEmpty,
        ErrorKind::CrossesDevices => Msg::IoReasonCrossesDevices,
        ErrorKind::QuotaExceeded => Msg::IoReasonQuotaExceeded,
        _ => return system_text.to_owned(),
    };
    lang.text(known).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_kind_is_told_in_the_language_and_an_unknown_one_keeps_the_system_text() {
        let known = io_reason_text(
            ErrorKind::PermissionDenied,
            "Permission denied (os error 13)",
            Language::English,
        );
        let unknown = io_reason_text(ErrorKind::Other, "something odd", Language::English);

        assert_eq!(known, Language::English.text(Msg::IoReasonPermissionDenied));
        assert_eq!(unknown, "something odd");
    }
}
