//! The chat as a messenger column, built from the kit.

use iced::widget::{column, operation, Row};
use iced::{Element, Length, Task};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::hermes_kit::{
    areas_card, chat_chip, chat_header, chat_input, chat_list, chat_page, chat_text, chip_row,
    folder_mention, hermes_bubble, levels_card, user_bubble, user_folder_bubble, working_bubble,
    AreaLine, ChatInput,
};
use ncrs_ui_kit::palette::Palette;
use ncrs_ui_kit::panel_footer::panel_footer;

use crate::chat_store::{Line, Speaker};
use crate::list_view::list_props;
use crate::message::Message;
use crate::state::{AreasCard, CountRow, Entry, Part, State};
use crate::texts::documents_label;

pub const INPUT_ID: &str = "hermes-chat-input";
pub const SCROLL_ID: &str = "hermes-chat-thread";
const AVATAR_LETTER: &str = "H";

#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    pub lang: Language,
    pub palette: &'a Palette,
}

/// Puts the cursor in the message field.
pub fn focus_input<T: Send + 'static>() -> Task<T> {
    operation::focus(INPUT_ID)
}

fn count_line(row: &CountRow, dim: bool, lang: Language) -> AreaLine {
    AreaLine {
        name: row.text.clone(),
        detail: String::new(),
        example_label: String::new(),
        example: String::new(),
        count: documents_label(lang, row.documents),
        dim,
    }
}

fn areas<'a>(card: &AreasCard, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let mut lines: Vec<AreaLine> = card
        .rows
        .iter()
        .map(|row| AreaLine {
            name: row.name.clone(),
            detail: row.description.clone(),
            example_label: ctx.lang.text(Msg::HermesChatExampleShort).to_owned(),
            example: row.example.clone(),
            count: documents_label(ctx.lang, row.documents),
            dim: false,
        })
        .collect();
    lines.extend(card.more.iter().map(|row| count_line(row, false, ctx.lang)));
    lines.extend(
        card.unordered
            .iter()
            .map(|row| count_line(row, true, ctx.lang)),
    );
    areas_card(&lines, ctx.palette)
}

fn part<'a>(part: &Part, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    match part {
        Part::Text(content) => chat_text(content, ctx.palette.text),
        Part::Areas(card) => areas(card, ctx),
        Part::Levels(card) => levels_card(
            &card.title,
            &card.rows,
            card.moves.as_ref(),
            card.footer.as_deref(),
            ctx.palette,
        ),
        Part::Folder(mention) => folder_mention(&mention.name, &mention.path, ctx.palette),
    }
}

fn entry<'a>(entry: &Entry, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    match entry {
        Entry::Hermes { parts, .. } => hermes_bubble(
            parts.iter().map(|shown| part(shown, ctx)).collect(),
            ctx.palette,
        ),
        Entry::User(content) => user_bubble(content, ctx.palette),
        Entry::UserFolder(mention) => user_folder_bubble(&mention.name, &mention.path, ctx.palette),
        Entry::Working(content) => working_bubble(content, ctx.palette),
    }
}

fn chips_of<'a>(state: &State, ctx: ViewCtx<'_>) -> Option<Element<'a, Message>> {
    let chips = state.chips();
    (!chips.is_empty()).then(|| {
        chip_row(
            chips
                .iter()
                .map(|chip| {
                    chat_chip(
                        &chip.label,
                        chip.primary,
                        Message::ChipPressed {
                            label: chip.label.clone(),
                            reply: chip.reply.clone(),
                        },
                        ctx.palette,
                    )
                })
                .collect(),
        )
    })
}

fn stored_entry<'a>(line: &Line, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    match line.speaker {
        Speaker::User => user_bubble(&line.text, ctx.palette),
        Speaker::Hermes => {
            hermes_bubble(vec![chat_text(&line.text, ctx.palette.text)], ctx.palette)
        }
    }
}

pub fn view<'a>(state: &State, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let (name, tagline) = match state.chats.title_of_shown() {
        Some(title) => (
            title.to_owned(),
            format!(
                "{} \u{b7} {}",
                lang.text(Msg::HermesChatTitle),
                lang.text(Msg::HermesChatSubtitle)
            ),
        ),
        None => (
            lang.text(Msg::HermesChatTitle).to_owned(),
            lang.text(Msg::HermesChatSubtitle).to_owned(),
        ),
    };
    let header = chat_header(
        AVATAR_LETTER,
        &name,
        &tagline,
        lang.text(Msg::HermesChatEscHint),
        ctx.palette,
    );
    let waiting_for_answer = !state.chips().is_empty();
    let placeholder = lang.text(if waiting_for_answer {
        Msg::HermesChatPlaceholderReply
    } else {
        Msg::HermesChatPlaceholder
    });
    let input = chat_input(
        ChatInput {
            value: &state.input,
            placeholder,
            id: INPUT_ID,
            on_input: Message::InputChanged,
            on_submit: Message::Submit,
            enabled: state.pending_work().is_none(),
            send_label: lang.text(Msg::HermesChatSend),
        },
        ctx.palette,
    );
    let page = match state.chats.shown_stored() {
        Some(stored) => chat_page(
            header,
            stored
                .lines
                .iter()
                .map(|line| stored_entry(line, ctx))
                .collect(),
            None,
            input,
            SCROLL_ID,
            ctx.palette,
        ),
        None => chat_page(
            header,
            state
                .thread()
                .iter()
                .map(|shown| entry(shown, ctx))
                .collect(),
            chips_of(state, ctx),
            input,
            SCROLL_ID,
            ctx.palette,
        ),
    };
    let list = chat_list(list_props(state, ctx), ctx.palette);
    let keys = crate::list_view::keys_line(lang);
    column![
        Row::new().push(list).push(page).height(Length::Fill),
        panel_footer(keys, ctx.palette),
    ]
    .into()
}
