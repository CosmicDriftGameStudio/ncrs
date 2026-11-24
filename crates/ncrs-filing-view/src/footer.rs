//! The bar under both halves: the figures, the box for the duplicates and the
//! buttons.

use iced::widget::row;
use iced::Element;
use ncrs_filing::chat_work::{tally, LivePlan};
use ncrs_i18n::{Language, Msg};
use ncrs_index::Transfer;
use ncrs_ui_kit::compare_bar::{compare_bar, Tally};
use ncrs_ui_kit::kit::{button, checkbox, key_button, paragraph, Check, Tone};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::theme::spacing;

use crate::message::Message;
use crate::state::Session;
use crate::texts::say;
use crate::update::can_file;

/// The figures of the bar as text, in the order shown; they add up to the
/// files of the plan.
pub fn figures(session: &Session, plan: &LivePlan, lang: Language) -> Vec<String> {
    let counts = tally(plan);
    let (done, total) = session.read_count();
    let number = |count: usize| count.to_string();
    vec![
        say(
            lang,
            Msg::FilingViewTallyRead,
            &[("done", &number(done)), ("total", &number(total))],
        ),
        say(
            lang,
            Msg::FilingViewTallyGoes,
            &[
                ("count", &number(counts.filed)),
                ("folders", &number(counts.folders)),
            ],
        ),
        say(
            lang,
            Msg::FilingViewTallyAlready,
            &[("count", &number(counts.already_there))],
        ),
        say(
            lang,
            Msg::FilingViewTallyNoFolder,
            &[("count", &number(counts.no_folder))],
        ),
        say(
            lang,
            Msg::FilingViewTallyUnreadable,
            &[("count", &number(counts.unreadable))],
        ),
    ]
}

pub fn footer<'a>(
    session: &Session,
    plan: &LivePlan,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let texts = figures(session, plan, lang);
    let colors = [
        palette.text,
        palette.success,
        palette.dim_text,
        palette.text,
        palette.error,
    ];
    let counts = tally(plan);
    let filed = counts.filed;
    let mut tallies: Vec<Tally> = texts
        .into_iter()
        .zip(colors)
        .map(|(text, color)| Tally { text, color })
        .collect();
    if session.filing {
        tallies.push(Tally {
            text: say(
                lang,
                Msg::FilingViewFiling,
                &[("count", &filed.to_string())],
            ),
            color: palette.text,
        });
    }
    let ready = can_file(session) && filed > 0;
    let carry = |transfer, label: Msg, key: &str| {
        key_button(
            &say(lang, label, &[("count", &filed.to_string())]),
            key,
            (ready && !(session.copy_only && transfer == Transfer::Move))
                .then_some(Message::File(transfer)),
            Tone::Primary,
            palette,
        )
    };
    let sure = filed - counts.unsure;
    let mut controls = vec![
        row![
            checkbox(
                if session.trash_dupes {
                    Check::On
                } else {
                    Check::Off
                },
                (!session.copy_only).then_some(Message::ToggleTrashDupes),
                palette,
            ),
            paragraph(lang.text(Msg::FilingViewTrashDupes), palette),
        ]
        .spacing(spacing::XS)
        .into(),
        button(
            lang.text(Msg::DialogCancel),
            (!session.filing).then_some(Message::Cancel),
            Tone::Secondary,
            palette,
        ),
    ];
    if counts.unsure > 0 && sure > 0 {
        controls.push(button(
            &say(
                lang,
                Msg::FilingViewMoveSure,
                &[("count", &sure.to_string())],
            ),
            (ready && !session.copy_only).then_some(Message::FileSure),
            Tone::Secondary,
            palette,
        ));
    }
    controls.push(carry(Transfer::Copy, Msg::FilingViewCopy, "↵"));
    controls.push(carry(Transfer::Move, Msg::FilingViewMove, "⇧↵"));
    compare_bar(lang.text(Msg::FilingViewTitle), tallies, controls, palette)
}
