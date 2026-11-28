//! The chat as a messenger column, built from the kit.

use chrono::{DateTime, Local, TimeZone as _};
use iced::widget::{column, operation, Row};
use iced::{Element, Length, Task};
use ncrs_i18n::{Language, Msg};
use ncrs_ui_kit::hermes_kit::{
    areas_card, chat_chip_keyed, chat_header, chat_input, chat_list, chat_page, chat_text,
    chip_row, day_mark, folder_mention, hermes_bubble, levels_card, user_bubble,
    user_folder_bubble, working_bubble, AreaLine, ChatInput,
};
use ncrs_ui_kit::page_status_bar::{
    hint_bar, page_status_bar, DiskWarning, PageStatusBar, PageStatusLine,
};
use ncrs_ui_kit::palette::Palette;

use crate::chat_store::{Line, Speaker};
use crate::list_view::{keys_line, list_props};
use crate::message::Message;
use crate::state::{AreasCard, ChipReply, CountRow, Entry, Part, State};
use crate::texts::{documents_label, fill};

pub const INPUT_ID: &str = "hermes-chat-input";
pub const SCROLL_ID: &str = "hermes-chat-thread";
const AVATAR_LETTER: &str = "H";

#[derive(Debug, Clone, Copy)]
pub struct ViewCtx<'a> {
    /// The used share of the drive, once it is past the warn level.
    pub disk_warning: Option<u8>,
    pub lang: Language,
    pub palette: &'a Palette,
    pub now: DateTime<Local>,
}

/// The status bar of the Hermes tab: the keys that work now, and what the last measure found.
fn status_bar<'a>(state: &State, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let shown_chip = |reply: ChipReply| state.chips().iter().any(|chip| chip.reply == reply);
    let mut keys: Vec<(&str, &str)> = vec![("\u{23ce}", lang.text(Msg::HermesChatSpaceKeySend))];
    for (reply, label) in [
        (
            ChipReply::SpaceDeleteSafe,
            Msg::HermesChatSpaceKeyDeleteSafe,
        ),
        (
            ChipReply::SpaceRemoveWorktrees,
            Msg::HermesChatSpaceKeyRemoveWorktrees,
        ),
        (ChipReply::SpaceOverview, Msg::HermesChatSpaceKeyOverview),
    ] {
        if shown_chip(reply.clone()) {
            if let Some(key) = key_label(&reply) {
                keys.push((key, lang.text(label)));
            }
        }
    }
    keys.push(("Tab", lang.text(Msg::SpaceKeyNextArea)));
    keys.push(("\u{21e7}F2", lang.text(Msg::HermesChatSpaceKeyCommandLine)));
    keys.push(("Esc", lang.text(Msg::HermesChatSpaceKeyClose)));
    let right = state.space_measured.map(|measured| {
        let time = DateTime::<Local>::from(measured.at)
            .format("%H:%M")
            .to_string();
        fill(
            lang,
            Msg::HermesChatSpaceStatusRight,
            &[
                (
                    "free",
                    &ncrs_ui_kit::format::size_shown(measured.free, lang),
                ),
                ("time", &time),
            ],
        )
    });
    page_status_bar(
        PageStatusBar {
            mode: lang.text(Msg::HermesChatTitle),
            line: PageStatusLine::Keys(&keys),
            right: right.as_deref(),
        },
        DiskWarning::of(ctx.disk_warning, ctx.lang, Message::SpaceAsked),
        ctx.palette,
    )
}

/// "Today 14:02" above the first message, or the day for an older chat.
fn day_line<'a>(state: &State, ctx: ViewCtx<'_>) -> Option<Element<'a, Message>> {
    let started = Local
        .timestamp_opt(state.chats.shown_created()?, 0)
        .single()?;
    let time = started.format("%H:%M").to_string();
    let content = if started.date_naive() == ctx.now.date_naive() {
        fill(ctx.lang, Msg::HermesChatSpaceDayToday, &[("time", &time)])
    } else {
        let date = started.format("%d.%m.%Y").to_string();
        fill(
            ctx.lang,
            Msg::HermesChatSpaceDayDated,
            &[("date", &date), ("time", &time)],
        )
    };
    Some(day_mark(&content, ctx.palette))
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

fn part<'a>(
    part: &Part,
    actions: &mut Option<Element<'a, Message>>,
    ctx: ViewCtx<'_>,
) -> Element<'a, Message> {
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
        Part::Space(card) => crate::space_card::view(card, actions.take(), ctx.palette),
    }
}

fn entry<'a>(
    entry: &Entry,
    mut actions: Option<Element<'a, Message>>,
    ctx: ViewCtx<'_>,
) -> Element<'a, Message> {
    match entry {
        Entry::Hermes { parts, .. } => hermes_bubble(
            parts
                .iter()
                .map(|shown| part(shown, &mut actions, ctx))
                .collect(),
            ctx.palette,
        ),
        Entry::User(content) => user_bubble(content, ctx.palette),
        Entry::UserFolder(mention) => user_folder_bubble(&mention.name, &mention.path, ctx.palette),
        Entry::Working(content) => working_bubble(content, ctx.palette),
    }
}

fn has_space_card(entry: &Entry) -> bool {
    matches!(entry, Entry::Hermes { parts, .. } if parts.iter().any(|shown| matches!(shown, Part::Space(_))))
}

/// The key that presses a button of the Platz card, as the platform writes it.
fn key_label(reply: &ChipReply) -> Option<&'static str> {
    let (mac, other) = match reply {
        ChipReply::SpaceDeleteSafe => ("\u{2318}\u{23ce}", "Ctrl+Enter"),
        ChipReply::SpaceRemoveWorktrees => ("\u{2318}\u{21e7}\u{23ce}", "Ctrl+Shift+Enter"),
        ChipReply::SpaceOverview => ("\u{2318}O", "Ctrl+O"),
        _ => return None,
    };
    Some(if cfg!(target_os = "macos") {
        mac
    } else {
        other
    })
}

fn chips_of<'a>(state: &State, ctx: ViewCtx<'_>) -> Option<Element<'a, Message>> {
    let chips = state.chips();
    (!chips.is_empty()).then(|| {
        chip_row(
            chips
                .iter()
                .map(|chip| {
                    chat_chip_keyed(
                        &chip.label,
                        chip.primary,
                        key_label(&chip.reply),
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
            day_line(state, ctx)
                .into_iter()
                .chain(stored.lines.iter().map(|line| stored_entry(line, ctx)))
                .collect(),
            None,
            input,
            SCROLL_ID,
            ctx.palette,
        ),
        None => {
            let mut chips = chips_of(state, ctx);
            let thread = state.thread();
            let card_is_last = thread.last().is_some_and(has_space_card);
            let entries = thread.iter().enumerate().map(|(at, shown)| {
                let inside = (card_is_last && at + 1 == thread.len())
                    .then(|| chips.take())
                    .flatten();
                entry(shown, inside, ctx)
            });
            let shown_entries: Vec<_> = day_line(state, ctx).into_iter().chain(entries).collect();
            chat_page(header, shown_entries, chips, input, SCROLL_ID, ctx.palette)
        }
    };
    let list = chat_list(list_props(state, ctx), ctx.palette);
    column![
        Row::new().push(list).push(page).height(Length::Fill),
        hint_bar(&keys_line(ctx.lang), ctx.palette),
        status_bar(state, ctx),
    ]
    .into()
}
