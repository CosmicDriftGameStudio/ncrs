//! The flows that file, change and answer: 5 tidy, 6 structure, 7 unknown.

use std::time::Instant;

use ncrs_hermes_chat::{ChipReply, Step};
use ncrs_i18n::Msg;

use super::thread::{chat, chip_labels, has_chip, last_hermes_has_chips, last_text};
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
        let mut stuck_chips_ok: Option<bool> = None;
        let later_again = self.language.text(Msg::HermesChatLaterAgain);
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
            if has_chip(self.app(), |reply| *reply == ChipReply::TrialOtherTarget) {
                let chips = chat(self.app()).chips();
                stuck_chips_ok = Some(
                    chips.len() >= 3
                        && chips.iter().any(|chip| {
                            chip.reply == ChipReply::Later && chip.label == later_again
                        }),
                );
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
            expect(
                stuck_chips_ok.unwrap_or(true),
                format!(
                    "the end of a trial without filing needs at least 3 buttons including '{later_again}'; labels: {}",
                    chip_labels(self.app())
                ),
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
        for (label, sentence) in [
            (
                "6 bank",
                self.say(
                    "Kontoauszüge der Sparkasse kommen nach Bank, dann Jahr und Monat",
                    "Statements from the Sparkasse go to Bank, then year and month",
                ),
            ),
            (
                "6 insurance",
                self.say(
                    "Versicherungspost nach Versicherung und Absender",
                    "Insurance mail to Versicherung and sender",
                ),
            ),
        ] {
            self.back_to_start();
            let started = Instant::now();
            self.type_sentence(sentence);
            let (_, _, limit) = self.wait_idle(READ_LIMIT, None);
            let image = self.shot(&format!("{}-proposal", label.replace(' ', "-")));
            let proposed = has_chip(self.app(), |reply| *reply == ChipReply::RuleCreate);
            let asked = has_chip(self.app(), |reply| matches!(reply, ChipReply::RuleWhat(_)));
            let met = all([
                limit,
                expect(
                    last_hermes_has_chips(self.app()),
                    "no proposal or question with buttons",
                ),
                expect(
                    proposed || asked,
                    format!(
                        "neither a proposed rule nor the question what goes in; last: {}",
                        last_text(self.app())
                    ),
                ),
            ]);
            // The rule is only proposed here: nothing is saved without a click
            // on "Create rule", and the protocol never makes it.
            if has_chip(self.app(), |reply| *reply == ChipReply::Cancel) {
                let _ = self.press_chip(|reply| *reply == ChipReply::Cancel);
            }
            self.record(
                label,
                &format!("typed '{sentence}'; labels: {}", chip_labels(self.app())),
                started.elapsed().as_secs_f64(),
                &image,
                "a rule proposal with levels and buttons, or the question what goes in",
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
