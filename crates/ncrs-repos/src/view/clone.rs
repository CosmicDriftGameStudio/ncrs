//! The clone dialog: an address, the folder the clone goes into, and the buttons.

use iced::widget::{column, row};
use iced::Element;
use ncrs_i18n::{fill_template, Msg};
use ncrs_ui_kit::frame::{DialogSize, Frame};
use ncrs_ui_kit::kit::{button, dim, enter_button, error_line, paragraph, text_field, Tone};
use ncrs_ui_kit::theme::spacing;

use crate::clone::{CloneDraft, CLONE_URL_ID};
use crate::ctx::ViewCtx;
use crate::message::Message;

pub fn dialog<'a>(draft: &'a CloneDraft, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let lang = ctx.lang;
    let editable = !draft.running;
    let mut content = column![
        column![
            paragraph(lang.text(Msg::ReposCloneUrl), palette),
            text_field(
                lang.text(Msg::ReposCloneUrlPlaceholder),
                &draft.url,
                CLONE_URL_ID,
                editable.then_some(Message::CloneUrlChanged),
                Message::CloneRun,
                palette,
            ),
        ]
        .spacing(spacing::XS),
        column![
            paragraph(lang.text(Msg::ReposCloneInto), palette),
            row![
                dim(draft.target.to_string_lossy().into_owned(), palette),
                button(
                    lang.text(Msg::ReposCloneChoose),
                    editable.then_some(Message::ClonePickTarget),
                    Tone::Secondary,
                    palette,
                ),
            ]
            .spacing(spacing::M),
        ]
        .spacing(spacing::XS),
    ]
    .spacing(spacing::M);
    if draft.running {
        content = content.push(dim(
            fill_template(
                lang.text(Msg::ReposCloneRunning),
                &[("{url}", draft.url.trim())],
            ),
            palette,
        ));
    }
    if let Some(problem) = &draft.problem {
        content = content.push(error_line(problem.as_str(), palette));
    }
    Frame::new(
        lang.text(Msg::ReposCloneTitle),
        content,
        DialogSize::Medium,
        ctx.window,
        lang,
        palette,
    )
    .buttons(vec![
        button(
            lang.text(Msg::GitViewDialogCancel),
            editable.then_some(Message::CloneClose),
            Tone::Secondary,
            palette,
        ),
        enter_button(
            lang.text(Msg::ReposCloneRun),
            (editable && !draft.url.trim().is_empty()).then_some(Message::CloneRun),
            Tone::Primary,
            palette,
        ),
    ])
    .on_close(Message::CloneClose)
    .into_element()
}
