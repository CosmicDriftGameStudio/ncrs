//! The chat as a messenger column, built from the kit.

use iced::widget::operation;
use iced::{Element, Task};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::hermes_kit::{
    areas_card, chat_chip, chat_header, chat_input, chat_page, chat_text, chip_row, file_card,
    folder_mention, hermes_bubble, levels_card, trial_card, user_bubble, user_folder_bubble,
    working_bubble, AreaLine, ChatInput, Choice, TrialLine,
};
use ncrs_ui_kit::palette::Palette;

use crate::message::Message;
use crate::state::{
    AreasCard, Chip, CountRow, Entry, FileCard, Part, RowOutcome, State, TrialCard,
};
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

fn file<'a>(card: &FileCard, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    file_card(&card.segments, card.highlighted, ctx.palette)
}

fn choice_of(chip: &Chip, selected: bool) -> Choice<Message> {
    Choice {
        label: chip.label.clone(),
        selected,
        on_press: Message::ChipPressed {
            label: chip.label.clone(),
            reply: chip.reply.clone(),
        },
    }
}

fn trial<'a>(card: &TrialCard, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let lines = card
        .rows
        .iter()
        .map(|row| match &row.outcome {
            RowOutcome::Filed { segments } => TrialLine::Filed {
                name: row.name.clone(),
                segments: segments.clone(),
            },
            RowOutcome::Unsure {
                question,
                choices,
                chosen,
            } => TrialLine::Unsure {
                name: row.name.clone(),
                question: question.clone(),
                choices: choices
                    .iter()
                    .enumerate()
                    .map(|(index, chip)| choice_of(chip, *chosen == Some(index)))
                    .collect(),
            },
            RowOutcome::AlreadyThere(note) | RowOutcome::Stays(note) => TrialLine::Note {
                name: row.name.clone(),
                note: note.clone(),
            },
        })
        .collect();
    trial_card(&card.header, lines, card.more.as_deref(), ctx.palette)
}

fn part<'a>(part: &Part, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    match part {
        Part::Text(content) => chat_text(content, ctx.palette.text),
        Part::Areas(card) => areas(card, ctx),
        Part::Levels(card) => {
            levels_card(&card.title, &card.rows, card.footer.as_deref(), ctx.palette)
        }
        Part::File(card) => file(card, ctx),
        Part::Trial(card) => trial(card, ctx),
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

pub fn view<'a>(state: &State, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let header = chat_header(
        AVATAR_LETTER,
        lang.text(Msg::HermesChatTitle),
        lang.text(Msg::HermesChatSubtitle),
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
    chat_page(
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
    )
}
