//! The words of the tab, in the user's language.

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
    }
}
