//! Flow 8: free sentences that build on each other, typed after the folder is
//! read. A step fails when Hermes answers with the small-talk intro or with
//! no buttons.

use std::time::Instant;

use ncrs_hermes_chat::ChipReply;
use ncrs_i18n::Msg;

use super::thread::{chip_labels, has_chip, last_hermes_has_chips, last_text};
use super::{all, expect, Protocol, READ_LIMIT};
use crate::app::App;
use crate::messages::Message;

impl<P: iced::Program<State = App, Message = Message> + 'static> Protocol<P> {
    pub fn flow_8_free_sentences(&mut self) {
        self.back_to_start();
        let archive = self.archive.clone();
        let small_talk_intro = self.language.text(Msg::HermesChatSmallTalk);
        let before_files = self.files_below(&self.archive);
        let steps = [
            (
                "8 everywhere",
                self.say(
                    "überall erst ordner für jahr und den datei namen mit jahr_monat_tag",
                    "everywhere first a folder for the year and the file name with year_month_day",
                ),
                "a preview card, not a 'not yet' sentence",
            ),
            (
                "8 only bank",
                self.say(
                    "dann erstmal nur den bank oder?",
                    "then just bank for now, right?",
                ),
                "a preview limited to Bank, or the area buttons; never the small-talk intro",
            ),
            (
                "8 by month",
                self.say("ne, lieber nach monat", "no, rather by month"),
                "the previous request with the order by month",
            ),
            (
                "8 cancel",
                self.say("abbrechen", "cancel"),
                "nothing changed",
            ),
            (
                "8 insurance",
                self.say("versicherungen nach anbieter", "insurance by sender"),
                "a change preview for that area",
            ),
            (
                "8 names",
                self.say(
                    "und die dateinamen auch mit datum vorne",
                    "and the file names with the date in front too",
                ),
                "the same area, names with date blocks",
            ),
        ];
        for (label, sentence, expectation) in steps {
            let started = Instant::now();
            self.type_sentence(sentence);
            let (_, _, mut limit) = self.wait_idle(READ_LIMIT, None);
            if has_chip(
                self.app(),
                |reply| matches!(reply, ChipReply::Folder(path) if *path == archive),
            ) {
                let _ = self.press_chip(
                    |reply| matches!(reply, ChipReply::Folder(path) if *path == archive),
                );
                limit = self.wait_idle(READ_LIMIT, None).2;
            }
            let image = self.shot(&label.replace(' ', "-"));
            let answer = last_text(self.app());
            let met = all([
                limit,
                expect(
                    !answer.contains(small_talk_intro),
                    "Hermes answered with the small-talk intro",
                ),
                expect(
                    last_hermes_has_chips(self.app()),
                    "the answer has no buttons",
                ),
                expect(
                    label != "8 cancel" || self.files_below(&self.archive) == before_files,
                    "files changed",
                ),
            ]);
            self.record(
                label,
                &format!(
                    "typed '{sentence}'; answer: {}; buttons: {}",
                    answer.chars().take(160).collect::<String>(),
                    chip_labels(self.app())
                ),
                started.elapsed().as_secs_f64(),
                &image,
                expectation,
                met,
            );
        }
    }
}
