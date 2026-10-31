//! The rules page: every confirmed rule as one readable sentence, with where
//! it came from, how many documents it has filed and a switch. The order is
//! the order Hermes tries the rules in. A page of the Hermes card. Pure view
//! over [`RulesPage`].

use iced::widget::{column, container, row, Space};
use iced::{alignment, Element, Length, Size};

use super::frame::{self, DialogSize, Frame, KeyHints, PageHint};
use super::hermes::{self, Badges};
use super::kit::{self, dim, error_line, paragraph, Check, Columns, Tone};
use super::theme::spacing;
use crate::app::fill_template;
use crate::i18n::{FuturamaName, Language, Msg};
use crate::messages::{HermesFocus, HermesPage, Message, RulesMessage};
use crate::palette::Palette;
use crate::rules_page::{hits_text, origin_text, rule_sentence, RulesPage, VISIBLE_ROWS};
use crate::rules_work::ListedRule;

const SIZE: DialogSize = DialogSize::Card;

/// How the width of a row is shared: rule, origin, hits.
const PORTIONS: [u16; 3] = [12, 4, 3];

/// The room of the switch left of a row, in pixels and in characters.
const SWITCH_WIDTH: f32 = 28.0;
const SWITCH_CHARS: usize = 4;

const PAGE_HINTS: &[PageHint] = &[
    PageHint {
        key: "Space",
        verb: Msg::RulesHintToggle,
    },
    PageHint {
        key: "Alt ↑↓",
        verb: Msg::RulesHintOrder,
    },
    PageHint {
        key: "Del",
        verb: Msg::RulesHintDelete,
    },
];

fn send(message: RulesMessage) -> Message {
    Message::Rules(message)
}

pub fn view<'a>(
    state: &'a RulesPage,
    window: Size,
    badges: Badges,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let line_chars = frame::content_chars(SIZE, window, frame::SIDEBAR_ROOM);
    let mut body = column![
        paragraph(lang.text(Msg::RulesExplain), palette),
        dim(
            fill_template(
                lang.text(Msg::RulesExample),
                &[("{rule}", lang.text(Msg::RulesExampleRule))],
            ),
            palette,
        ),
    ]
    .spacing(spacing::LINE_GAP);

    if state.rows().is_empty() {
        body = body.push(dim(
            lang.text(if state.is_busy() {
                Msg::RulesLoading
            } else {
                Msg::RulesEmpty
            }),
            palette,
        ));
    } else {
        body = body.push(list(state, line_chars, badges.focus, lang, palette));
        if let Some(selected) = state.selected().filter(|_| !state.is_confirming_delete()) {
            body = body.push(paragraph(sentence_of(state, selected, lang), palette));
        }
    }
    if state.is_confirming_delete() {
        body = body.push(delete_question(state, lang, palette));
    }
    if let Some(detail) = state.problem() {
        body = body.push(error_line(
            fill_template(lang.text(Msg::RulesProblemFailed), &[("{detail}", detail)]),
            palette,
        ));
    }

    Frame::new(
        lang.text(Msg::ActionRules),
        body,
        SIZE,
        window,
        lang,
        palette,
    )
    .title_theme(lang.futurama_name(FuturamaName::ActionRules))
    .buttons(buttons(state, lang, palette))
    .sidebar(hermes::nav(HermesPage::Rules, badges, lang, palette))
    .on_close(send(RulesMessage::Close))
    .hints(KeyHints {
        enter: false,
        tab: false,
    })
    .page_hints(PAGE_HINTS)
    .into_element()
}

fn sentence_of(state: &RulesPage, listed: &ListedRule, lang: Language) -> String {
    rule_sentence(&listed.rule.spec, state.tax_contexts(), lang)
}

fn list<'a>(
    state: &'a RulesPage,
    line_chars: usize,
    focus: HermesFocus,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let columns = Columns {
        portions: &PORTIONS,
        line_chars: line_chars.saturating_sub(SWITCH_CHARS),
    };
    let header = row![
        Space::new().width(SWITCH_WIDTH),
        columns.header(
            &[
                lang.text(Msg::RulesColumnRule),
                lang.text(Msg::RulesColumnOrigin),
                lang.text(Msg::RulesColumnHits),
            ],
            palette,
        ),
    ];

    let first = state.first_visible();
    let rows = state
        .rows()
        .iter()
        .enumerate()
        .skip(first)
        .take(VISIBLE_ROWS)
        .map(|(index, listed)| {
            let selected = index == state.cursor();
            let enabled = listed.rule.spec.enabled;
            let color = if selected && focus == HermesFocus::Content {
                palette.cursor_text
            } else if enabled {
                palette.text
            } else {
                palette.dim_text
            };
            let switch = kit::checkbox(
                if enabled { Check::On } else { Check::Off },
                Some(send(RulesMessage::ToggleAt(index))),
                palette,
            );
            let content = row![
                container(switch).width(SWITCH_WIDTH),
                columns.cell(0, &sentence_of(state, listed, lang), color),
                columns.cell(1, origin_text(listed.rule.origin, lang), color),
                columns.cell(2, &hits_text(listed.hits, lang), color),
            ]
            .align_y(alignment::Vertical::Center);
            kit::list_row(
                content.into(),
                send(RulesMessage::Select(index)),
                selected,
                focus == HermesFocus::Content,
                palette,
            )
        })
        .collect();
    kit::scroll_list(
        header.into(),
        rows,
        kit::ListView {
            len: state.rows().len(),
            visible: VISIBLE_ROWS,
            offset: first,
        },
        |delta| send(RulesMessage::Scrolled(delta)),
        palette,
    )
}

fn delete_question<'a>(
    state: &RulesPage,
    lang: Language,
    palette: &Palette,
) -> Element<'a, Message> {
    let rule = state
        .selected()
        .map(|listed| sentence_of(state, listed, lang))
        .unwrap_or_default();
    container(paragraph(
        fill_template(lang.text(Msg::RulesDeleteQuestion), &[("{rule}", &rule)]),
        palette,
    ))
    .width(Length::Fill)
    .into()
}

fn buttons<'a>(state: &RulesPage, lang: Language, palette: &Palette) -> Vec<Element<'a, Message>> {
    let button = |label: Msg, on_press: Option<RulesMessage>, tone: Tone| {
        kit::button(lang.text(label), on_press.map(send), tone, palette)
    };
    if state.is_confirming_delete() {
        return vec![
            button(
                Msg::RulesCancel,
                Some(RulesMessage::CancelDelete),
                Tone::Secondary,
            ),
            button(
                Msg::RulesDeleteConfirm,
                Some(RulesMessage::ConfirmDelete),
                Tone::Danger,
            ),
        ];
    }
    let selected = !state.is_busy() && state.selected().is_some();
    let can_move = |delta: isize| {
        selected
            && state
                .cursor()
                .checked_add_signed(delta)
                .is_some_and(|neighbour| neighbour < state.rows().len())
    };
    vec![
        button(Msg::RulesClose, Some(RulesMessage::Close), Tone::Secondary),
        button(
            Msg::RulesDelete,
            selected.then_some(RulesMessage::AskDelete),
            Tone::Secondary,
        ),
        button(
            Msg::RulesMoveDown,
            can_move(1).then_some(RulesMessage::Reorder(1)),
            Tone::Secondary,
        ),
        button(
            Msg::RulesMoveUp,
            can_move(-1).then_some(RulesMessage::Reorder(-1)),
            Tone::Secondary,
        ),
    ]
}
