//! The "filing" scene of the trailer: the real Filing tab, fed the way the planning
//! feeds it (what the index knows, the reading of the rest, then the whole plan), with
//! scripted rows instead of the rules. Every name here is made up.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use ncrs_filing::sorting::RowState;
use ncrs_filing_view::decide::Decided;
use ncrs_filing_view::Row;
use ncrs_index::RuleId;

use super::{Focus, Recorder};
use crate::messages::Message;
use crate::screenshot_scenes::{
    filing_already_there, filing_is_reading, filing_place, filing_row, filing_session,
};
use crate::trailer_data::dense_showcase_app;

const SOURCE: &str = "/Users/alex/Scans";
const TARGET: &str = "/Users/alex/Documents";

/// One new picture per file that is read.
const SECONDS_PER_FILE: f32 = 0.24;
/// The index knows these files already, the others are read.
const KNOWN: usize = 18;
const STRIDE: usize = 37;

/// Both halves of the tab and the bar.
const FILING_TAB: Focus = Focus::new(0.0, 60.0, 1920.0, 1020.0);

struct Folder {
    path: &'static str,
    /// A file that is in the folder already: what the duplicates point at.
    already: &'static str,
    /// The names of the new files, without the extension.
    names: &'static [&'static str],
}

/// 42 files in the source and 28 new files in the tree: all of them fit on the screen at once,
/// the tab keeps the top of both halves in view.
static FOLDERS: [Folder; 6] = [
    Folder {
        path: "Bank/Savings/Statements/2025",
        already: "2024-12 Bank statement.pdf",
        names: &[
            "2025-01 Bank statement",
            "2025-02 Bank statement",
            "2025-03 Bank statement",
            "2025-04 Bank statement",
            "2025-05 Bank statement",
        ],
    },
    Folder {
        path: "Tax/2024",
        already: "2024 Wage statement.pdf",
        names: &[
            "2024 Donation receipt",
            "2024 Commuting costs",
            "2024 Childcare costs",
            "2024 Interest certificate",
            "2024 Tax advisor invoice",
        ],
    },
    Folder {
        path: "Insurance/Acme Mutual/Car",
        already: "2024 Car policy.pdf",
        names: &[
            "2025 Car policy",
            "2025 Premium notice",
            "Claim 2025-06 Windscreen",
            "Cover note 2025",
        ],
    },
    Folder {
        path: "Car/Garage",
        already: "Inspection report 2023.pdf",
        names: &[
            "2025-02 Service invoice",
            "2025-04 Tyre change",
            "2025-09 Battery invoice",
            "Inspection report 2025",
            "Warranty extension 2025",
        ],
    },
    Folder {
        path: "Home/Utilities",
        already: "2024 Heating invoice.pdf",
        names: &[
            "2025 Utility statement",
            "2025-01 Electricity bill",
            "2025 Heating invoice",
            "2025 Water meter reading",
            "2025 Waste collection fees",
        ],
    },
    Folder {
        path: "Health/Invoices",
        already: "2024-09 Dentist invoice.pdf",
        names: &[
            "2025-02 Dentist invoice",
            "2025-03 Pharmacy receipt",
            "2025-06 Optician invoice",
            "2025 Reimbursement notice",
        ],
    },
];

const DUPLICATES: usize = 7;
const WITHOUT_RULE: usize = 5;
const UNREADABLE: usize = 2;

/// What the planning decides for a file, before the files are put in the order
/// of the source.
#[derive(Clone)]
enum Fate {
    Filed {
        folder: &'static Folder,
        name: String,
    },
    Duplicate {
        folder: &'static Folder,
    },
    NoRule,
    Unreadable,
}
fn fates() -> Vec<Fate> {
    let mut fates = Vec::new();
    for folder in &FOLDERS {
        for name in folder.names {
            fates.push(Fate::Filed {
                folder,
                name: (*name).to_owned(),
            });
        }
    }
    fates.extend(
        FOLDERS
            .iter()
            .cycle()
            .take(DUPLICATES)
            .map(|folder| Fate::Duplicate { folder }),
    );
    fates.extend(std::iter::repeat_n(Fate::NoRule, WITHOUT_RULE));
    fates.extend(std::iter::repeat_n(Fate::Unreadable, UNREADABLE));
    fates
}

/// The name a scanner or a phone gives the file at this place of the source.
fn source_name(index: usize) -> String {
    match index % 5 {
        0 => format!("Scan_{:04}.pdf", 40 + index),
        1 => format!("IMG_{}.jpg", 2200 + index * 7),
        2 => format!("Document ({index}).pdf"),
        3 => format!("scan{index:03}.pdf"),
        _ => format!("IMG_2025{:04}.jpg", 300 + index),
    }
}

/// The files in the order of the source: a fixed shuffle of the fates.
fn plan_rows(rules: &[RuleId]) -> Vec<Row> {
    let fates = fates();
    let total = fates.len();
    let mut shuffled: Vec<(usize, Fate)> = fates
        .into_iter()
        .enumerate()
        .map(|(index, fate)| (index * STRIDE % total, fate))
        .collect();
    shuffled.sort_by_key(|(place, _)| *place);
    shuffled
        .into_iter()
        .enumerate()
        .map(|(index, (_, fate))| {
            let from = source_name(index);
            let extension = if from.ends_with(".jpg") { "jpg" } else { "pdf" };
            let state = match fate {
                Fate::Filed { folder, name } => {
                    let rule = FOLDERS
                        .iter()
                        .zip(rules)
                        .find(|(other, _)| other.path == folder.path)
                        .map(|(_, rule)| *rule)
                        .expect("every folder has its rule");
                    filing_place(
                        rule,
                        PathBuf::from(folder.path).join(format!("{name}.{extension}")),
                    )
                }
                Fate::Duplicate { folder } => filing_already_there(
                    PathBuf::from(TARGET).join(folder.path).join(folder.already),
                ),
                Fate::NoRule => RowState::NoRule,
                Fate::Unreadable => RowState::NoText,
            };
            filing_row(&PathBuf::from(SOURCE), index, PathBuf::from(from), state)
        })
        .collect()
}

pub(in crate::trailer) fn filing(recorder: &mut Recorder) {
    recorder.begin_scene("filing", "Hermes tidies up", "Scans in, folders out");
    let (mut session, rules) =
        filing_session(SOURCE, TARGET, FOLDERS.iter().map(|folder| folder.path));
    let rows = plan_rows(&rules);
    session.rows = rows.clone();
    session.planned = true;
    filing_is_reading(&mut session, KNOWN);
    let (id, progress) = (session.id, Arc::clone(&session.progress));
    let decided = Decided {
        rows,
        waiting: Vec::new(),
        targets: session.targets.clone(),
        rule_names: session.rule_names.clone(),
        trash_can_restore: session.trash_can_restore,
    };
    recorder.show_app(
        dense_showcase_app().showing_filing_for_test(ncrs_filing_view::State {
            session: Some(session),
        }),
    );

    recorder.aim(FILING_TAB);
    recorder.frame(1.4);

    for read in 1..=progress.total() {
        progress.done.store(read, Ordering::Relaxed);
        recorder.send(Message::FilingView(ncrs_filing_view::Message::Tick));
        recorder.frame(SECONDS_PER_FILE);
    }

    recorder.send(Message::FilingView(ncrs_filing_view::Message::Planned {
        session: id,
        read: true,
        result: Ok(Box::new(decided)),
    }));
    recorder.frame(2.6);
}
