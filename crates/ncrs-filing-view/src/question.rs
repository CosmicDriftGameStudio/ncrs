//! The question about one file that the rules of several targets match.

use std::path::PathBuf;

use iced::widget::{column, opaque};
use iced::{Element, Length};
use ncrs_filing::sorting::RowPlace;
use ncrs_i18n::{Language, Msg};
use ncrs_index::TargetId;
use ncrs_ui_kit::frame::{content_width, DialogSize, Frame, KeyHints};
use ncrs_ui_kit::hermes_kit::{target_place_row, TargetPlace};
use ncrs_ui_kit::kit::{button, enter_button, scrim, Check, Tone};
use ncrs_ui_kit::theme::spacing;

use crate::ctx::ViewCtx;
use crate::message::Message;
use crate::run::file_name;
use crate::state::{place_of, Session};
use crate::texts::{count, say};

/// Up to this many places the answer is one button per place.
const PLACES_AS_BUTTONS: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// Below the source.
    pub relative: PathBuf,
    pub places: Vec<AskedPlace>,
    pub marked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskedPlace {
    place: (TargetId, PathBuf),
    /// The colour of the chip.
    index: usize,
    number: String,
    name: String,
    /// The rule and where the file would go.
    line: String,
    /// Only a longer list is ticked.
    ticked: bool,
}

impl Question {
    pub fn new(session: &Session, relative: PathBuf, places: &[RowPlace], lang: Language) -> Self {
        let asked_places = places
            .iter()
            .map(|place| {
                let index = session.target_index(&place.target).unwrap_or(usize::MAX);
                let line = say(
                    lang,
                    Msg::SortingQuestionOption,
                    &[
                        ("rule", session.rule_name(place.rule)),
                        ("path", &place.relative.display().to_string()),
                    ],
                );
                AskedPlace {
                    place: place_of(place),
                    index,
                    number: session.target_number(&place.target),
                    name: session
                        .targets
                        .get(index)
                        .map_or_else(String::new, |target| target.name.clone()),
                    line,
                    ticked: false,
                }
            })
            .collect();
        Self {
            relative,
            places: asked_places,
            marked: 0,
        }
    }

    pub fn is_list(&self) -> bool {
        self.places.len() > PLACES_AS_BUTTONS
    }

    pub fn toggle(&mut self, at: usize) {
        if let Some(place) = self.places.get_mut(at) {
            place.ticked = !place.ticked;
        }
    }

    pub fn move_mark(&mut self, forward: bool) {
        let last = self.places.len().saturating_sub(1);
        self.marked = if forward {
            (self.marked + 1).min(last)
        } else {
            self.marked.saturating_sub(1)
        };
    }

    /// The places Enter answers with.
    pub fn confirmed(&self) -> Vec<usize> {
        if !self.is_list() {
            return vec![0];
        }
        (0..)
            .zip(&self.places)
            .filter_map(|(at, place)| place.ticked.then_some(at))
            .collect()
    }

    pub fn places_at(&self, chosen: &[usize]) -> Vec<(TargetId, PathBuf)> {
        chosen
            .iter()
            .filter_map(|at| self.places.get(*at))
            .map(|asked| asked.place.clone())
            .collect()
    }
}

pub fn question_dialog<'a>(question: &'a Question, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let mut frame = Frame::new(
        file_name(&question.relative),
        places(question, ctx),
        DialogSize::Wide,
        ctx.window,
        lang,
        ctx.palette,
    )
    .buttons(buttons(question, ctx))
    .on_close(Message::QuestionLeave)
    .hints(KeyHints {
        enter: true,
        tab: false,
    });
    match question.places.len() {
        0 | 1 => {}
        2 => frame = frame.subtitle(lang.text(Msg::SortingQuestionMatches)),
        many => frame = frame.subtitle(count(lang, Msg::FilingViewQuestionMatchesMany, many)),
    }
    opaque(scrim(frame.into_element()))
}

fn places<'a>(question: &'a Question, ctx: ViewCtx<'_>) -> Element<'a, Message> {
    let width = content_width(DialogSize::Wide, ctx.window);
    let is_list = question.is_list();
    let lines = question.places.iter().enumerate().map(|(at, asked)| {
        let state = if asked.ticked { Check::On } else { Check::Off };
        target_place_row(
            TargetPlace {
                check: is_list.then_some((state, Message::QuestionToggle(at))),
                index: asked.index,
                label: &format!("{} {}", asked.number, asked.name),
                line: &asked.line,
                marked: is_list && question.marked == at,
            },
            width,
            ctx.palette,
        )
    });
    column(lines).spacing(spacing::M).width(Length::Fill).into()
}

fn buttons<'a>(question: &Question, ctx: ViewCtx<'_>) -> Vec<Element<'a, Message>> {
    let (lang, palette) = (ctx.lang, ctx.palette);
    let all: Vec<usize> = (0..question.places.len()).collect();
    let mut shown = vec![button(
        lang.text(Msg::SortingQuestionLeave),
        Some(Message::QuestionLeave),
        Tone::Secondary,
        palette,
    )];
    if question.is_list() {
        shown.push(button(
            lang.text(Msg::FilingViewQuestionAll),
            Some(Message::QuestionAnswer(all)),
            Tone::Secondary,
            palette,
        ));
        let chosen = question.confirmed();
        shown.push(enter_button(
            lang.text(Msg::FilingViewQuestionFile),
            (!chosen.is_empty()).then_some(Message::QuestionAnswer(chosen)),
            Tone::Primary,
            palette,
        ));
        return shown;
    }
    let only =
        |asked: &AskedPlace| say(lang, Msg::SortingQuestionOnly, &[("target", &asked.number)]);
    if question.places.len() > 1 {
        shown.push(button(
            lang.text(Msg::SortingQuestionBoth),
            Some(Message::QuestionAnswer(all)),
            Tone::Secondary,
            palette,
        ));
    }
    if let Some(second) = question.places.get(1) {
        shown.push(button(
            &only(second),
            Some(Message::QuestionAnswer(vec![1])),
            Tone::Secondary,
            palette,
        ));
    }
    if let Some(first) = question.places.first() {
        shown.push(enter_button(
            &only(first),
            Some(Message::QuestionAnswer(vec![0])),
            Tone::Primary,
            palette,
        ));
    }
    shown
}
