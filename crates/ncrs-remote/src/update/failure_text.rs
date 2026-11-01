use ncrs_fs::remote::session::ConnectFailure;
use ncrs_i18n::{fill_template, Language, Msg};

pub fn connect_failure_text(lang: Language, failure: &ConnectFailure, host: &str) -> String {
    match failure {
        ConnectFailure::UnknownHost { .. } => String::new(),
        ConnectFailure::HostKeyChanged { file, line, .. } => fill_template(
            lang.text(Msg::ErrorRemoteHostkeyChanged),
            &[
                ("{host}", host),
                ("{file}", &file.display().to_string()),
                ("{line}", &line.to_string()),
            ],
        ),
        ConnectFailure::HostKeyOtherType { file, line, .. } => fill_template(
            lang.text(Msg::ErrorRemoteHostkeyOtherType),
            &[
                ("{host}", host),
                ("{file}", &file.display().to_string()),
                ("{line}", &line.to_string()),
            ],
        ),
        ConnectFailure::KnownHostsUnreadable { file, reason } => fill_template(
            lang.text(Msg::ErrorRemoteKnownHostsUnreadable),
            &[
                ("{file}", &file.display().to_string()),
                ("{reason}", reason),
            ],
        ),
        ConnectFailure::PasswordNotOffered => fill_template(
            lang.text(Msg::ErrorRemotePasswordNotOffered),
            &[("{host}", host)],
        ),
        ConnectFailure::HostKeyRevoked => fill_template(
            lang.text(Msg::ErrorRemoteHostkeyRevoked),
            &[("{host}", host)],
        ),
        ConnectFailure::AuthFailed => lang.text(Msg::ErrorRemoteAuthFailed).to_owned(),
        ConnectFailure::Timeout => {
            fill_template(lang.text(Msg::ErrorRemoteTimeout), &[("{host}", host)])
        }
        ConnectFailure::NoCommonAlgorithm => fill_template(
            lang.text(Msg::ErrorRemoteNoCommonAlgorithm),
            &[("{host}", host)],
        ),
        ConnectFailure::Unreachable(reason) => fill_template(
            lang.text(Msg::ErrorRemoteUnreachable),
            &[("{host}", host), ("{reason}", reason)],
        ),
        ConnectFailure::Keychain(reason) => {
            fill_template(lang.text(Msg::ErrorRemoteKeychain), &[("{reason}", reason)])
        }
    }
}
