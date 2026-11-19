//! The words of the tab, in the user's language.

use std::collections::BTreeSet;

use ncrs_filing::chat_work::LiveTags;
use ncrs_i18n::{fill_template, Language, Msg};

pub fn say(lang: Language, msg: Msg, values: &[(&str, &str)]) -> String {
    let named: Vec<(String, &str)> = values
        .iter()
        .map(|(name, value)| (format!("{{{name}}}"), *value))
        .collect();
    let borrowed: Vec<(&str, &str)> = named
        .iter()
        .map(|(name, value)| (name.as_str(), *value))
        .collect();
    fill_template(lang.text(msg), &borrowed)
}

pub fn tags(lang: Language) -> LiveTags {
    LiveTags {
        unreadable: lang.text(Msg::FilingViewTagUnreadable).to_owned(),
        no_folder: lang.text(Msg::FilingViewTagNoFolder).to_owned(),
        no_folder_in_every_language: Language::ALL
            .into_iter()
            .map(|each| each.text(Msg::FilingViewTagNoFolder).to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    }
}

/// The room a half's header leaves besides its text: padding, the checkbox
/// and the gaps.
pub const HEADER_MARGIN: f32 = 48.0;
