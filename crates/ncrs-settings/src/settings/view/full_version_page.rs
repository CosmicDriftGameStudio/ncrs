use super::*;
use crate::message::StoreButton;
use ncrs_store::{Entitlement, FullKind};

pub fn full_version_groups<'a>(view: &SettingsView<'a>) -> Vec<Group<'a>> {
    let lang = view.lang;
    let entitlement = view.entitlement;
    let button_row = |label: Msg, button: StoreButton, caption: Msg| {
        let message = SettingsMessage::Store(button);
        simple_row(
            label,
            None,
            ncrs_ui_kit::kit::button(
                lang.text(caption),
                Some(message.clone()),
                Tone::Secondary,
                view.palette,
            ),
            lang,
        )
        .keyed(RowKey::Press(message))
    };

    let mut rows = vec![simple_row(
        Msg::StoreRowStatus,
        None,
        status_text(status_sentence(entitlement, lang), view.palette.text),
        lang,
    )];
    if entitlement == Entitlement::TrialNotStarted {
        rows.push(button_row(
            Msg::StoreRowTrial,
            StoreButton::StartTrial,
            Msg::StoreButtonStartTrial,
        ));
    }
    if !matches!(entitlement, Entitlement::Full(_)) {
        rows.push(button_row(
            Msg::StoreRowBuy,
            StoreButton::Buy,
            Msg::StoreButtonBuy,
        ));
    }
    rows.push(button_row(
        Msg::StoreRowRestore,
        StoreButton::Restore,
        Msg::StoreButtonRestore,
    ));
    vec![Group {
        intro: Some(lang.text(Msg::StoreModelNote).to_owned()),
        title: String::new(),
        rows,
    }]
}

/// Where the trial or the purchase stands, in a sentence.
fn status_sentence(entitlement: Entitlement, lang: Language) -> String {
    match entitlement {
        Entitlement::Unrestricted | Entitlement::TrialNotStarted => {
            lang.text(Msg::StoreStatusNotStarted).to_owned()
        }
        Entitlement::Trial { days_left: 1 } => lang.text(Msg::StoreStatusTrialOne).to_owned(),
        Entitlement::Trial { days_left } => fill_template(
            lang.text(Msg::StoreStatusTrial),
            &[("{days}", &days_left.to_string())],
        ),
        Entitlement::Expired => lang.text(Msg::StoreStatusExpired).to_owned(),
        Entitlement::Full(FullKind::Purchased) => lang.text(Msg::StoreStatusBought).to_owned(),
        Entitlement::Full(FullKind::EarlierPurchase) => {
            lang.text(Msg::StoreStatusEarlier).to_owned()
        }
    }
}
