//! The left half: every file of the read folder with what happens to it.

use iced::widget::column;
use iced::Element;
use ncrs_filing::chat_work::{LiveFile, LivePlan, LiveState};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::frame::AVERAGE_GLYPH_WIDTH;
use ncrs_ui_kit::kit::{dim, Columns};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::plan_rows::{pane_scroll, plan_file_row, PlanFileRow};

use crate::message::Message;
use crate::state::Session;
use crate::texts::say;

/// The portions of the name and the what-happens column.
const PORTIONS: [u16; 2] = [1, 1];

/// What happens to a file, as a line, and the chip it gets.
pub fn what_happens(
    lang: Language,
    palette: &Palette,
    file: &LiveFile,
) -> (String, iced::Color, Option<String>) {
    let (plain, color, chip) = match &file.state {
        LiveState::Waiting => (
            lang.text(Msg::FilingViewStateWaiting).to_owned(),
            palette.dim_text,
            None,
        ),
        LiveState::Looking => (
            lang.text(Msg::FilingViewStateLooking).to_owned(),
            palette.text,
            None,
        ),
        LiveState::Filed { folder, name, .. } => {
            let path = folder.join(name).display().to_string();
            (
                say(lang, Msg::FilingViewStateFiled, &[("path", &path)]),
                palette.success,
                None,
            )
        }
        LiveState::AlreadyThere { at } => (
            say(
                lang,
                Msg::FilingViewStateAlready,
                &[("at", &at.display().to_string())],
            ),
            palette.dim_text,
            None,
        ),
        LiveState::NoFolder => (
            lang.text(Msg::FilingViewStateNoFolder).to_owned(),
            palette.text,
            Some(lang.text(Msg::FilingViewTagNoFolder).to_owned()),
        ),
        LiveState::Unreadable => (
            lang.text(Msg::FilingViewStateUnreadable).to_owned(),
            palette.error,
            Some(lang.text(Msg::FilingViewTagUnreadable).to_owned()),
        ),
    };
    let line = if file.by_user {
        format!("{plain}{}", lang.text(Msg::FilingViewByUser))
    } else {
        plain
    };
    (line, color, chip)
}

pub fn left<'a>(
    session: &Session,
    plan: &LivePlan,
    half_width: f32,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: (half_width / AVERAGE_GLYPH_WIDTH) as usize,
    };
    let rows = plan
        .files
        .iter()
        .map(|file| {
            let (what, what_color, chip) = what_happens(lang, palette, file);
            plan_file_row(
                PlanFileRow {
                    name: file.from.display().to_string(),
                    what,
                    what_color,
                    chip,
                    chosen: session.selected.as_ref() == Some(&file.from),
                    on_press: Message::Select(file.from.clone()),
                },
                &columns,
                palette,
            )
        })
        .collect();
    column![
        dim(
            say(
                lang,
                Msg::FilingViewReadFolder,
                &[("path", &session.source.display().to_string())],
            ),
            palette,
        ),
        columns.header(
            &[
                lang.text(Msg::ColumnName),
                lang.text(Msg::FilingViewColumnWhat)
            ],
            palette,
        ),
        pane_scroll(rows),
    ]
    .into()
}
