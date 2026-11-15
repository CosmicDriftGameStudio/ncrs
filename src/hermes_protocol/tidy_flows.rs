//! The flows that file, change and answer: 5 tidy, 6 structure, 7 unknown.

use std::time::{Duration, Instant};

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

    /// The chat asks for the folders and hands over to the tab; the tab plans
    /// live. Nothing is filed: the run never presses copy or move.
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
        for round in 1..=20 {
            let labels = chip_labels(self.app());
            questions.push(format!(
                "round {round}: {} => {labels}",
                last_text(self.app()).chars().take(80).collect::<String>()
            ));
            if self.app().filing_session_for_test().is_some() {
                break;
            }
            let first_press = self.press_chip(|reply| match reply {
                ChipReply::Folder(path) | ChipReply::NamedFolder(path) => *path == archive,
                _ => false,
            });
            let pressed = first_press
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::Once))
                .or_else(|_| self.press_chip(|reply| *reply == ChipReply::ConfirmTidy))
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
        let handed_over = self.last_says(Msg::HermesChatHandover);
        // The tab is the shown page now; the chat is the next tab after it.
        self.deliver(Message::NextTab);
        let chat_image = self.shot("tidy-handover");
        self.deliver(Message::PreviousTab);

        let mid_image = self.plan_in_tab_until_done();
        let done_image = self.shot("filing-done");
        let tally = self.filing_tally();
        println!("[{}] 5 tidy filing tally: {tally}", self.language.code());

        let planned = self
            .app()
            .filing_session_for_test()
            .is_some_and(|session| session.plan_is_done());
        let moved = before_downloads.saturating_sub(self.files_below(&self.downloads));
        let arrived = self
            .files_below(&self.archive)
            .saturating_sub(before_archive);
        let result = all([
            limit,
            expect(handed_over, "the chat did not say it hands over to the tab"),
            expect(planned, "the tab did not finish planning"),
            expect(
                mid_image.is_some(),
                "no screenshot of the tab at about half decided",
            ),
            expect(
                moved == 0 && arrived == 0,
                format!(
                    "{moved} left downloads, {arrived} reached the archive: nothing may be filed"
                ),
            ),
        ]);
        self.record(
            "5 tidy",
            &format!("questions: {} ;; tally: {tally}", questions.join(" ;; ")),
            started.elapsed().as_secs_f64(),
            &format!(
                "{chat_image}, {}, {done_image}",
                mid_image.unwrap_or_else(|| "no mid shot".to_owned())
            ),
            "the chat hands over; the tab plans every file; nothing is filed",
            result,
        );
    }

    /// Feeds the tab its ticks until the plan is done. The first screenshot
    /// is taken once half of the files are decided.
    fn plan_in_tab_until_done(&mut self) -> Option<String> {
        let started = Instant::now();
        let mut mid = None;
        while started.elapsed() < READ_LIMIT {
            self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
            self.pump(Duration::from_millis(100));
            let Some(session) = self.app().filing_session_for_test() else {
                return mid;
            };
            let (looked_at, all_files) = session.read_count();
            if session.plan_is_done() {
                break;
            }
            if mid.is_none() && all_files > 0 && looked_at * 2 >= all_files {
                mid = Some(self.shot("filing-mid"));
            }
        }
        self.deliver(Message::FilingView(ncrs_filing_view::Message::Tick));
        mid
    }

    /// One line: how many files, and where the plan puts them.
    fn filing_tally(&self) -> String {
        use ncrs_filing::chat_work::LiveState;
        let Some(plan) = self
            .app()
            .filing_session_for_test()
            .and_then(|session| session.shown_plan())
        else {
            return "no plan".to_owned();
        };
        let count = |is: fn(&LiveState) -> bool| plan.files.iter().filter(|f| is(&f.state)).count();
        format!(
            "{} files, {} filed ({} unsure), {} already there, {} without folder, {} unreadable",
            plan.files.len(),
            count(|state| matches!(state, LiveState::Filed { .. })),
            count(|state| matches!(state, LiveState::Filed { unsure: true, .. })),
            count(|state| matches!(state, LiveState::AlreadyThere { .. })),
            count(|state| matches!(state, LiveState::NoFolder)),
            count(|state| matches!(state, LiveState::Unreadable)),
        )
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
