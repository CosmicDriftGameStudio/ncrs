//! The activity page of the settings: what Hermes does now, in the words of
//! the status line, and the latest runs that moved files with what they
//! moved. Pure view over [`ActivityPage`].

use std::time::{Duration, SystemTime};

use ncrs_config::palette::Palette;
use ncrs_filing::filing::ActivityBatch;
use ncrs_i18n::{fill_template, Language, Msg};
use ncrs_index::BatchKind;
use ncrs_ui_kit::format;
use ncrs_ui_kit::kit::Tone;
use ncrs_ui_kit::setting_rows::{Group, RowButton, SettingRow};

use super::reviews::{file_name, folder_of, MAX_FILES_SHOWN};
use crate::activity_page::ActivityPage;
use crate::hermes_status::HermesStatus;
use crate::message::{ActivityMessage, Message};

fn send(message: ActivityMessage) -> Message {
    Message::Activity(message)
}

pub fn groups<'a>(
    state: &ActivityPage,
    status: Option<HermesStatus>,
    lang: Language,
    palette: &Palette,
) -> Vec<Group<'a, Message>> {
    vec![
        Group {
            intro: None,
            title: lang.text(Msg::ActivityPageGroupNow).to_owned(),
            rows: vec![now_row(status, lang, palette)],
        },
        Group {
            intro: None,
            title: lang.text(Msg::ActivityPageGroupJournal).to_owned(),
            rows: journal_rows(state, lang, palette),
        },
    ]
}

/// The status line, with the numbers that have a page of their own as links.
fn now_row<'a>(
    status: Option<HermesStatus>,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let Some(known) = status else {
        return said(
            lang.text(Msg::ActivityPageOff).to_owned(),
            Vec::new(),
            palette,
        );
    };
    let link = |template: Msg, count: usize, message: ActivityMessage| {
        (count > 0).then(|| {
            RowButton::new(
                fill_template(
                    lang.text(template),
                    &[("{count}", &format::grouped(count as u64))],
                ),
                Some(send(message)),
                Tone::Secondary,
            )
        })
    };
    let links = [
        link(
            Msg::ActivityPageLinkReviews,
            known.to_review,
            ActivityMessage::OpenReviews,
        ),
        link(
            Msg::ActivityPageLinkErrors,
            known.failed,
            ActivityMessage::OpenReadErrors,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    said(known.line(lang), links, palette)
}

/// A row that is its `line` and the `links`; the line stands as the label, so
/// no empty label line is left above it.
fn said<'a>(
    line: String,
    links: Vec<RowButton<Message>>,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    SettingRow {
        label: line,
        ..SettingRow::adding_buttons(None, None, links, palette)
    }
}

fn journal_rows<'a>(
    state: &ActivityPage,
    lang: Language,
    palette: &Palette,
) -> Vec<SettingRow<'a, Message>> {
    if let Some(problem) = state.problem() {
        return vec![SettingRow::adding(
            Some(String::new()),
            Some(problem.to_owned()),
            Vec::new(),
        )];
    }
    if state.is_loading() {
        return vec![SettingRow::adding(
            Some(lang.text(Msg::HermesPagesLoading).to_owned()),
            None,
            Vec::new(),
        )];
    }
    if state.batches().is_empty() {
        return vec![SettingRow::adding(
            Some(lang.text(Msg::ActivityPageNone).to_owned()),
            None,
            Vec::new(),
        )];
    }
    let mut rows = Vec::new();
    for batch in state.batches() {
        rows.push(batch_row(state, batch, lang, palette));
        rows.extend(move_rows(batch, lang, palette));
    }
    rows
}

fn batch_row<'a>(
    state: &ActivityPage,
    batch: &ActivityBatch,
    lang: Language,
    palette: &Palette,
) -> SettingRow<'a, Message> {
    let at = u64::try_from(batch.at)
        .ok()
        .map(|seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds));
    let mut when = state.time_text(at);
    if batch.undone {
        when = format!("{when} \u{b7} {}", lang.text(Msg::ActivityPageUndone));
    }
    SettingRow::listed_buttons(
        lang.text(kind_text(batch.kind)).to_owned(),
        Some(when),
        Some(format::files_counted(batch.moves.len(), lang)),
        Vec::new(),
        palette,
    )
}

fn move_rows<'a>(
    batch: &ActivityBatch,
    lang: Language,
    palette: &Palette,
) -> Vec<SettingRow<'a, Message>> {
    let mut rows: Vec<SettingRow<'a, Message>> = batch
        .moves
        .iter()
        .take(MAX_FILES_SHOWN)
        .map(|(from, to)| {
            SettingRow::listed_buttons(
                file_name(to),
                Some(format!("{} \u{2192} {}", folder_of(from), folder_of(to))),
                None,
                vec![RowButton::new(
                    lang.text(Msg::HermesPagesShowFile),
                    Some(send(ActivityMessage::ShowFile(to.clone()))),
                    Tone::Secondary,
                )],
                palette,
            )
            .indented()
        })
        .collect();
    if batch.moves.len() > MAX_FILES_SHOWN {
        rows.push(
            SettingRow::adding(
                Some(fill_template(
                    lang.text(Msg::HermesPagesMoreFiles),
                    &[(
                        "{count}",
                        &format::grouped((batch.moves.len() - MAX_FILES_SHOWN) as u64),
                    )],
                )),
                None,
                Vec::new(),
            )
            .indented(),
        );
    }
    rows
}

fn kind_text(kind: BatchKind) -> Msg {
    match kind {
        BatchKind::Filing => Msg::ActivityPageKindFiling,
        BatchKind::Hermes => Msg::ActivityPageKindHermes,
        BatchKind::Reorganisation => Msg::ActivityPageKindReorganisation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes_status::StateWord;

    fn idle() -> HermesStatus {
        HermesStatus {
            word: StateWord::Idle,
            working: 0,
            open: 0,
            to_review: 0,
            failed: 0,
            pdf_library_missing: false,
            stuck: false,
        }
    }

    fn now_rows(status: Option<HermesStatus>) -> Vec<SettingRow<'static, Message>> {
        let page = ActivityPage::new();
        groups(&page, status, Language::German, &Palette::default())
            .into_iter()
            .next()
            .unwrap()
            .rows
    }

    #[test]
    fn an_idle_hermes_says_its_word_and_leaves_no_empty_row() {
        let rows = now_rows(Some(idle()));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "ruht");
        assert!(rows[0].description.is_none());
    }

    #[test]
    fn a_hermes_that_is_off_says_so_in_the_label() {
        let rows = now_rows(None);
        assert_eq!(rows[0].label, Language::German.text(Msg::ActivityPageOff));
        assert!(rows[0].description.is_none());
    }
}
