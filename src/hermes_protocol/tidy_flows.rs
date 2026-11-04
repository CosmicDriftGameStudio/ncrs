//! The flows that file, change and answer: 5 tidy, 6 structure, 7 unknown.

use std::time::Instant;

use ncrs_hermes_chat::{ChipReply, Step};
use ncrs_i18n::Msg;

use super::thread::{chat, chip_labels, has_chip, last_hermes_has_chips, last_text, whole_thread};
use super::{all, expect, Protocol, READ_LIMIT};
use crate::app::App;
use crate::messages::Message;

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    /// Whether the newest bubble ends like the text of `msg`: the words after
    /// its last placeholder.
    fn last_says(&self, msg: Msg) -> bool {
        let template = self.language.text(msg);
        let fixed = template.rsplit('}').next().unwrap_or(template);
        last_text(self.app()).contains(fixed)
    }

    pub fn flow_5_tidy(&mut self) {
        self.back_to_start();
        let before_downloads = self.files_below(&self.downloads);
        let before_archive = self.files_below(&self.archive);
        let sentence = self.say("Downloads aufräumen", "Tidy up Downloads");
        let started = Instant::now();
        self.type_sentence(sentence);
        let (_, _, mut limit) = self.wait_idle(READ_LIMIT, None);
        let mut questions = Vec::new();
        let archive = self.archive.clone();
        for round in 1..=80 {
            let image = if round <= 25 {
                self.shot(&format!("tidy-{round:02}"))
            } else {
                format!("round {round}")
            };
            let labels = chip_labels(self.app());
            questions.push(format!(
                "{image}: {} => {labels}",
                last_text(self.app()).chars().take(80).collect::<String>()
            ));
            if has_chip(self.app(), |reply| *reply == ChipReply::Undo) {
                break;
            }
            let first_press = self.press_chip(|reply| match reply {
                ChipReply::Folder(path) | ChipReply::NamedFolder(path) => *path == archive,
                _ => false,
            });
            let pressed = first_press
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::Once))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::ModeMove))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::ConfirmTidy))
                .or_else(|_| self.press_chip(|reply| matches!(reply, ChipReply::GroupArea(_))))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::FileAll))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::SinglesLeave))
                .or_else(|_| self.press_chip(|reply| matches!(reply, ChipReply::DecideTarget(_))))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::NamesLeave))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::PhotosNo))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::KeepYes));
            if pressed.is_err() {
                break;
            }
            let (_, _, again) = self.wait_idle(READ_LIMIT, None);
            limit = again;
            if limit.is_err() {
                break;
            }
        }
        let moved = before_downloads.saturating_sub(self.files_below(&self.downloads));
        let arrived = self
            .files_below(&self.archive)
            .saturating_sub(before_archive);
        let result = all([
            limit,
            expect(
                moved > 0 && arrived > 0,
                format!("{moved} left downloads, {arrived} reached the archive"),
            ),
        ]);
        self.record(
            "5 tidy",
            &format!("questions: {}", questions.join(" ;; ")),
            started.elapsed().as_secs_f64(),
            "tidy-NN",
            "files move from downloads to the archive",
            result,
        );

        let undo_started = Instant::now();
        let asked = self.press_chip(|reply| *reply == ChipReply::Undo);
        let (_, _, asked_limit) = self.wait_idle(READ_LIMIT, None);
        let confirmed = self.press_chip(|reply| *reply == ChipReply::PutBack);
        let (_, _, undo_limit) = self.wait_idle(READ_LIMIT, None);
        let undone = all([asked, asked_limit, confirmed]);
        let image = self.shot("tidy-undone");
        let restored = self.files_below(&self.downloads);
        let undo_result = all([
            undone,
            undo_limit,
            expect(
                restored >= before_downloads,
                format!("{restored} of {before_downloads} files back in downloads"),
            ),
        ]);
        self.record(
            "5 undo",
            "pressed undo",
            undo_started.elapsed().as_secs_f64(),
            &image,
            "the files are back in downloads",
            undo_result,
        );
    }

    pub fn flow_6_structure(&mut self) {
        let german = self.german();
        for (label, sentence, word) in [
            (
                "6 bank",
                self.say("Bank nach Monat", "Bank by month"),
                "Bank",
            ),
            (
                "6 insurance",
                self.say("Versicherung nach Anbieter", "Insurance by sender"),
                "Versicherung",
            ),
        ] {
            self.back_to_start();
            let started = Instant::now();
            self.type_sentence(sentence);
            let (_, _, mut limit) = self.wait_idle(READ_LIMIT, None);
            let asked_area = chat(self.app())
                .chips()
                .iter()
                .any(|chip| chip.label == word && matches!(chip.reply, ChipReply::ChangeArea(_)));
            if asked_area {
                let _ = self.press_chip(
                    |reply| matches!(reply, ChipReply::ChangeArea(area) if area == word),
                );
                limit = self.wait_idle(READ_LIMIT, None).2;
            }
            let image = self.shot(&format!("{}-preview", label.replace(' ', "-")));
            let text = whole_thread(self.app());
            let previewed = has_chip(self.app(), |reply| {
                matches!(reply, ChipReply::ConfirmChange | ChipReply::ScopeAll)
            }) || self.last_says(Msg::HermesChatChangeAlready);
            let mut met = all([
                limit,
                expect(
                    last_hermes_has_chips(self.app()),
                    "no preview or answer with buttons",
                ),
                expect(
                    previewed,
                    format!(
                        "neither a preview nor 'already so'; last: {}",
                        last_text(self.app())
                    ),
                ),
                expect(
                    !(german && text.to_lowercase().contains("statements")),
                    "an English folder word in the German thread",
                ),
            ]);
            let mut images = image;
            if has_chip(self.app(), |reply| *reply == ChipReply::ConfirmChange) {
                let pressed = self.press_chip(|reply| *reply == ChipReply::ConfirmChange);
                let (_, _, done) = self.wait_idle(READ_LIMIT, None);
                images = format!(
                    "{images} {}",
                    self.shot(&format!("{}-done", label.replace(' ', "-")))
                );
                met = all([met, pressed, done]);
                if has_chip(self.app(), |reply| *reply == ChipReply::Undo) {
                    let _ = self.press_chip(|reply| *reply == ChipReply::Undo);
                    let _ = self.wait_idle(READ_LIMIT, None);
                    images = format!(
                        "{images} {}",
                        self.shot(&format!("{}-undone", label.replace(' ', "-")))
                    );
                }
            }
            self.record(
                label,
                &format!(
                    "typed '{sentence}' ({word}); labels: {}",
                    chip_labels(self.app())
                ),
                started.elapsed().as_secs_f64(),
                &images,
                "a preview with real folder names and a confirmation, or a clear 'already so'",
                met,
            );
        }
    }

    pub fn flow_7_unknown(&mut self) {
        for (label, sentence) in [
            (
                "7 suggestion",
                self.say(
                    "Mach mal einen Vorschlag, im Root sollten die Themen stehen",
                    "Make a suggestion, the topics should be at the root",
                ),
            ),
            (
                "7 abilities",
                self.say("Was kannst du?", "What can you do?"),
            ),
        ] {
            self.back_to_start();
            let started = Instant::now();
            self.type_sentence(sentence);
            let (_, _, limit) = self.wait_idle(READ_LIMIT, None);
            let image = self.shot(&label.replace(' ', "-"));
            let met = all([
                limit,
                expect(
                    last_hermes_has_chips(self.app())
                        && !matches!(chat(self.app()).step, Step::Role)
                        && !self.last_says(Msg::HermesChatChangeAreaAsk),
                    format!(
                        "no honest answer with buttons; last: {}",
                        last_text(self.app())
                    ),
                ),
            ]);
            self.record(
                label,
                &format!("typed '{sentence}'"),
                started.elapsed().as_secs_f64(),
                &image,
                "an honest answer with buttons",
                met,
            );
        }
    }
}
