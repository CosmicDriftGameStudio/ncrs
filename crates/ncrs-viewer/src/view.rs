//! The viewer tab: header, the file, status line.

use iced::widget::{column, container, row};
use iced::{alignment, Element, Length};
use ncrs_i18n::Msg;
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::{button, key_button, segmented, Tone};
use ncrs_ui_kit::markdown_view::markdown_view;
use ncrs_ui_kit::page_scroll::page_scroll;
use ncrs_ui_kit::page_status_bar::{page_status_bar, PageStatusBar, PageStatusLine};
use ncrs_ui_kit::page_text::{page_heading, page_line, page_source};
use ncrs_ui_kit::theme::{self, font_size, spacing};

use crate::ctx::ViewCtx;
use crate::load::Doc;
use crate::message::{Message, Mode};
use crate::state::State;

/// The id of the scrollable, for the scroll tasks.
pub const PAGE_ID: &str = "viewer-page";

/// The key of the viewer in the status bar's mode field.
const MODE_FIELD: &str = "F3";

/// The key cap of the editor button.
const EDIT_KEY: &str = "F4";

pub fn view<'a>(state: &'a State, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let Some(doc) = state.doc.as_ref() else {
        return container("").into();
    };
    let palette = ctx.palette;
    let hint = ctx.lang.text(if doc.markdown {
        Msg::ViewerStatusMarkdown
    } else {
        Msg::ViewerStatusText
    });
    let body: Element<'a, Message> = match state.mode {
        Mode::Rendered if doc.markdown => markdown_view(&doc.items, palette, Message::LinkClicked),
        Mode::Rendered | Mode::Source => page_source(&doc.text, palette),
    };
    let bar = PageStatusBar {
        mode: MODE_FIELD,
        line: PageStatusLine::Hint(hint),
        right: None,
    };
    column![
        header(state, doc, ctx),
        theme::horizontal_rule(palette.border_inactive),
        page_scroll(PAGE_ID, body),
        page_status_bar(bar, palette),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn header<'a>(state: &State, doc: &Doc, ctx: ViewCtx<'a>) -> Element<'a, Message> {
    let palette = ctx.palette;
    let facts = [
        format::shown_path(&doc.path, ctx.home),
        format::size(doc.size),
        format::time(doc.modified),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    let titles = column![
        page_heading(&doc.name, palette),
        page_line(facts, font_size::SMALL, palette.dim_text),
    ]
    .spacing(spacing::XS);
    let mut line = row![titles, container("").width(Length::Fill)]
        .spacing(spacing::M)
        .align_y(alignment::Vertical::Center);
    if doc.markdown {
        let selected = usize::from(state.mode == Mode::Source);
        line = line.push(segmented(
            &[
                (
                    ctx.lang.text(Msg::ViewerModeRendered),
                    Message::SetMode(Mode::Rendered),
                ),
                (
                    ctx.lang.text(Msg::ViewerModeSource),
                    Message::SetMode(Mode::Source),
                ),
            ],
            selected,
            true,
            palette,
        ));
    }
    line = line
        .push(button(
            ctx.lang.text(Msg::ViewerOpenExternal),
            Some(Message::OpenExternal),
            Tone::Secondary,
            palette,
        ))
        .push(key_button(
            ctx.lang.text(Msg::ViewerEdit),
            EDIT_KEY,
            Some(Message::Edit),
            Tone::Secondary,
            palette,
        ));
    container(line)
        .padding([spacing::L, spacing::XXL])
        .width(Length::Fill)
        .into()
}
