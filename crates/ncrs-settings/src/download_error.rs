//! The words for a failed model download, shared by the OCR and AI model
//! cards. Only text that comes from the system or the server stays as it is.

use ncrs_i18n::{fill_template, Language, Msg};

use crate::open_vsx::FetchError;

/// The detail line for `error`, in the language of the UI.
pub fn download_error_text(error: &FetchError, lang: Language) -> String {
    match error {
        FetchError::Status(code) => fill_template(
            lang.text(Msg::DownloadErrorStatus),
            &[("{code}", code.to_string().as_str())],
        ),
        FetchError::NoConnection(detail) => fill_template(
            lang.text(Msg::DownloadErrorNoConnection),
            &[("{detail}", detail)],
        ),
        FetchError::Timeout => lang.text(Msg::DownloadErrorTimeout).to_owned(),
        FetchError::Tls(detail) => {
            fill_template(lang.text(Msg::DownloadErrorTls), &[("{detail}", detail)])
        }
        FetchError::Network(detail) => detail.clone(),
        FetchError::DownloadRefused { host } => {
            fill_template(lang.text(Msg::DownloadErrorRefused), &[("{host}", host)])
        }
        FetchError::TooManyRedirects => lang.text(Msg::DownloadErrorRedirects).to_owned(),
        FetchError::TooLarge => lang.text(Msg::DownloadErrorTooLarge).to_owned(),
        FetchError::InvalidId
        | FetchError::NotFound(_)
        | FetchError::BadAnswer(_)
        | FetchError::NoDownloadLink
        | FetchError::BadPackage(_)
        | FetchError::NoThemes
        | FetchError::MissingFile(_)
        | FetchError::UnsafePath(_)
        | FetchError::FileTooLarge(_)
        | FetchError::BadJson { .. }
        | FetchError::NoConfigPath
        | FetchError::Config(_)
        | FetchError::WriteFailed { .. } => lang.text(Msg::DownloadErrorUnexpected).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_is_worded_in_the_ui_language() {
        let error = FetchError::Status(503);
        assert_eq!(
            download_error_text(&error, Language::German),
            "Der Server antwortete mit 503"
        );
        assert_eq!(
            download_error_text(&error, Language::English),
            "The server answered 503"
        );
    }
}
