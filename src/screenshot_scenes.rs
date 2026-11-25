//! Fixture pages for the screenshot generators: the Git, filing, notes, system
//! and viewer tabs, filled by hand through the public API of their crates.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::shadow_unrelated,
    reason = "test code: a failing setup is the signal"
)]

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ncrs_filing::sorting::{ReadProblem, RowState};
use ncrs_filing_view::{Change, Row, Session, State as FilingState};
use ncrs_git_view::{Message as GitMessage, State as GitState};
use ncrs_i18n::Language;
use ncrs_index::{
    Condition, ConditionKind, ContentHash, DocumentFields, DocumentId, Index, RuleConditions,
    RuleId, RuleOrigin, RuleSpec, TargetId,
};
use ncrs_intake::{LocationTransfer, OnNoText, OnSeveralTargets, Place, SortSource, SortTarget};
use ncrs_monitor::{
    CpuInfo, Depth, DriveRow, HostInfo, LoadInfo, MonitorMessage, NetworkRow, SensorRow, Snapshot,
    TaskCounts, TaskRow, TaskStatus, Usage,
};
use ncrs_notes::store::scan;
use ncrs_notes::tree::RowKey;

/// The repository and the notes folder the pages read from, kept alive while
/// the pictures are made.
pub struct Scenes {
    _folder: tempfile::TempDir,
    repository: PathBuf,
    notes: PathBuf,
    readme: PathBuf,
}

impl Scenes {
    pub fn new() -> Self {
        let folder = tempfile::tempdir().expect("scratch folder");
        let parent = crate::e2e_tests::acceptance_v160_git::build_development_folder(folder.path());
        let notes = folder.path().join("Notes");
        write_notes(&notes);
        let readme = folder.path().join("README.md");
        fs::write(&readme, README).expect("the README");
        Self {
            repository: parent.join("ncrs-demo"),
            _folder: folder,
            notes,
            readme,
        }
    }

    /// The history graph with the details of a commit.
    pub fn git(&self) -> GitState {
        let root = &self.repository;
        let lang = Language::English;
        let mut state = GitState::default();
        let ctx = ncrs_git_view::Ctx::new(lang);
        drop(ncrs_git_view::open(
            &mut state,
            root.clone(),
            "ncrs-demo".to_owned(),
            &ctx,
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let feed = |state: &mut GitState, message: GitMessage| {
            drop(ncrs_git_view::update(state, message, &ctx));
        };

        let overview = ncrs_git::refs(root, &stop).expect("the refs");
        let generation = generation_of(|at| state.loads.refs.is_current(at));
        feed(
            &mut state,
            GitMessage::RefsLoaded(generation, Box::new(Ok(overview))),
        );
        let log = ncrs_git::log(root, state.limit, "", &stop).expect("the log");
        let generation = generation_of(|at| state.loads.log.is_current(at));
        let position = log
            .commits
            .iter()
            .position(|commit| commit.summary == "Add the command palette")
            .expect("the commit with two hunks");
        let row = usize::from(state.has_working_row()) + position;
        let sha = log.commits[position].id.clone();
        feed(
            &mut state,
            GitMessage::LogLoaded(generation, Box::new(Ok(log))),
        );

        feed(&mut state, GitMessage::CommitPressed(row));
        let detail = ncrs_git::commit_detail(root, &sha, &stop).expect("the commit");
        let generation = generation_of(|at| state.loads.detail.is_current(at));
        feed(
            &mut state,
            GitMessage::DetailLoaded(generation, Box::new(Ok(detail))),
        );

        state
    }

    /// A note rendered next to the tree of the notes folder.
    pub fn notes(&self) -> ncrs_notes::State {
        let ctx = ncrs_notes::Ctx {
            lang: Language::English,
        };
        let mut state = ncrs_notes::State::new(self.notes.clone());
        let mut found = scan(&self.notes).expect("the notes");
        // The file time of a fresh temp file would change the picture.
        for note in &mut found.notes {
            note.modified = None;
        }
        state.open = true;
        drop(ncrs_notes::update(
            &mut state,
            ncrs_notes::Message::Loaded(Box::new(Ok(found))),
            &ctx,
        ));
        for folder in ["House", "Work", "Travel"] {
            state.expanded.insert(folder.into());
        }
        let note = self.notes.join("House/Heating service.md");
        drop(ncrs_notes::update(
            &mut state,
            ncrs_notes::Message::Pressed(RowKey::Note(note)),
            &ctx,
        ));
        state
    }

    /// The README of the project, rendered.
    pub fn viewer(&self) -> ncrs_viewer::State {
        let ncrs_viewer::load::Loaded::Doc(mut doc) = ncrs_viewer::load::load(self.readme.clone())
        else {
            panic!("the README did not load");
        };
        doc.modified = None;
        doc.path = PathBuf::from("/Users/alex/Projects/ncrs/README.md");
        ncrs_viewer::State {
            doc: Some(doc),
            ..ncrs_viewer::State::default()
        }
    }
}

/// The number of the read that `is_current` accepts; the slot keeps the count to itself.
fn generation_of(is_current: impl Fn(u64) -> bool) -> u64 {
    (0..256)
        .find(|at| is_current(*at))
        .expect("a read is in flight")
}

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("a folder")).expect("note folder");
    fs::write(path, text).expect("note");
}

const HEATING: &str = "# Heating service\n\nService every October, Krause Haustechnik, phone 089 1234567.\n\n## Open\n\n- Book the 2026 appointment\n- Invoice 2025 filed, see [invoice-2025-10.pdf](file:///Users/alex/Documents/invoice-2025-10.pdf)\n- Ask about the offer for a new boiler\n\n## Values\n\n- Pressure should be 1.5 bar, measured 1.3 bar\n- Filter last changed 10/2025\n- Next check: **October 2026**\n";

fn write_notes(root: &Path) {
    write(root, "House/Heating service.md", HEATING);
    for (name, text) in [
        ("Groceries", "Bread\nMilk\nCoffee\n"),
        ("Insurance", "Policy number in the safe.\n"),
        (
            "Garden",
            "- Prune the roses in March\n- New fence in spring\n",
        ),
        (
            "Internet contract",
            "Runs until 2027-02, notice 3 months.\n",
        ),
        ("Wifi", "Network ncrs-home, password on the router.\n"),
    ] {
        write(root, &format!("House/{name}.md"), text);
    }
    for (name, text) in [
        ("Roadmap", "# Roadmap\n\n- Notes tab\n- Git view\n"),
        ("Meeting 2026-10-12", "Agenda: release, store pictures.\n"),
        ("Ideas", "- something small\n"),
        (
            "Interview questions",
            "1. Why this team?\n2. What did you build?\n",
        ),
        ("Expenses", "Train 48 EUR, hotel 210 EUR.\n"),
    ] {
        write(root, &format!("Work/{name}.md"), text);
    }
    for (name, text) in [
        ("Italy in May", "Check the heating of the hotel.\n"),
        ("Packing list", "- passport\n- charger\n- umbrella\n"),
        ("Train tickets", "Booked, see the PDF in Downloads.\n"),
        ("Lisbon", "Tram 28, pastel de nata, the viewpoints.\n"),
    ] {
        write(root, &format!("Travel/{name}.md"), text);
    }
    for (name, text) in [
        ("Books", "- The Dispossessed\n- Piranesi\n"),
        ("Gift ideas", "Ceramic cup for Anna.\n"),
        ("Recipes", "Pasta e ceci: chickpeas, rosemary, tomato.\n"),
    ] {
        write(root, &format!("{name}.md"), text);
    }
    write(
        root,
        ".files/Offer-1a2b3c4d.md",
        "---\nfile: /Users/alex/Documents/House/Offer-Vaillant.pdf\n---\nOffer valid until 31 December, heating with installation 14,800 EUR.\n",
    );
}

const README: &str = "# ncrs\n\nA two-panel file manager for macOS, Linux and Windows, with an index of your documents and **Hermes**, which reads, names and files them for you.\n\n## Install\n\n- macOS: `brew install cosmicdrift/ncrs/ncrs`\n- Debian, Ubuntu: `sudo apt install ncrs`\n- Fedora: `sudo dnf install ncrs`\n\n## Keys\n\n1. F3 views a file\n2. F4 edits it\n3. F5 copies, F6 moves\n4. F7 makes a folder\n5. Alt+F7 searches\n\n## Features\n\n- Two panels with tabs, favourites and a command palette\n- Compare two folders and sync the differences\n- Rename many files at once with a pattern\n- A notes tab and a system monitor\n- A Git view with history graph and diffs\n\nSee the [guide](docs/guide.md) or the [site](https://example.org).\n";

const FILING_FOLDERS: [&str; 8] = [
    "Invoices/Acme Ltd",
    "Invoices/Globex",
    "Contracts",
    "Insurance",
    "Tax/2025",
    "Tax/2026",
    "Salary",
    "Warranty",
];
const FILING_COMPANIES: [&str; 5] = ["Acme Ltd", "Globex", "Initech", "Umbrella", "Hooli"];
const FILING_FILES: usize = 60;

fn filing_target() -> TargetId {
    TargetId::new("documents").expect("the name of the target")
}

/// A session of `source` with one target and a rule per folder of it. Only an
/// index gives out the numbers of rules.
pub fn filing_session(
    source: &str,
    target: &str,
    folders: impl IntoIterator<Item = &'static str>,
) -> (Session, Vec<RuleId>) {
    let dir = tempfile::tempdir().expect("scratch folder");
    let mut index = Index::create_plain(&dir.path().join("index.db")).expect("the index");
    let mut rules = Vec::new();
    let mut names = HashMap::new();
    for folder in folders {
        let name = folder.replace('/', " ");
        let spec = RuleSpec {
            name: Some(name.clone()),
            conditions: RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Substring,
                name.to_lowercase(),
            )]),
            extract_number: None,
            extract_date: None,
            extract_amount: None,
            kind: None,
            tags: BTreeSet::new(),
            filing_pattern: folder.to_owned(),
            confirmed: true,
            enabled: true,
        };
        let rule = index
            .create_rule_in(&spec, RuleOrigin::default(), &filing_target())
            .expect("the rule");
        rules.push(rule);
        names.insert(rule, name);
    }
    let mut session = Session::new(
        SortSource {
            path: PathBuf::from(source),
            include: String::new(),
            exclude: String::new(),
            targets: vec![filing_target()],
            watch: false,
            transfer: LocationTransfer::Move,
            no_text: OnNoText::Ask,
            several: OnSeveralTargets::Ask,
            own_volume: None,
        },
        true,
    );
    session.targets = vec![SortTarget {
        id: filing_target(),
        name: "Documents".to_owned(),
        path: PathBuf::from(target),
        own_volume: Some(false),
    }];
    session.rule_names = names;
    session.trash_can_restore = true;
    (session, rules)
}

pub fn filing_place(rule: RuleId, relative: PathBuf) -> RowState {
    RowState::Place(Place {
        target: filing_target(),
        rule,
        relative,
        fields: DocumentFields::default(),
    })
}

pub fn filing_already_there(path: PathBuf) -> RowState {
    RowState::AlreadyThere {
        target: filing_target(),
        path,
    }
}

pub fn filing_row(source: &Path, index: usize, relative: PathBuf, state: RowState) -> Row {
    Row {
        path: source.join(&relative),
        relative,
        hash: Some(ContentHash([1; 32])),
        document: i64::try_from(index).ok().map(DocumentId::from_number),
        state,
    }
}

/// While the rows from `known` on are still read.
pub fn filing_is_reading(session: &mut Session, known: usize) {
    session.planning = true;
    session.reading = true;
    session
        .progress
        .total
        .store(session.rows.len().saturating_sub(known), Ordering::Relaxed);
    for row in session.rows.iter_mut().skip(known) {
        row.state = RowState::NotRead(ReadProblem::Unknown);
    }
}

fn filing_state(index: usize, rules: &[RuleId]) -> RowState {
    let at = index % FILING_FOLDERS.len();
    let folder = PathBuf::from(FILING_FOLDERS[at]);
    match index % 10 {
        3 => filing_already_there(
            Path::new("/Users/alex/Documents")
                .join(folder)
                .join(format!("old-{index:03}.pdf")),
        ),
        6 => RowState::NoRule,
        9 => RowState::NoText,
        _ => filing_place(
            rules[at],
            folder.join(format!(
                "{} {} 2026-{:02} {index:03}.pdf",
                ["Invoice", "Contract", "Policy", "Notice"][index % 4],
                FILING_COMPANIES[index % FILING_COMPANIES.len()],
                index % 12 + 1
            )),
        ),
    }
}

/// The Filing tab with a source, a target and 60 scans. The first `read` ones
/// are known, the others are being read; all of them are decided when `done`.
pub fn filing_tab(read: usize, done: bool) -> FilingState {
    let (mut session, rules) =
        filing_session("/Users/alex/Scans", "/Users/alex/Documents", FILING_FOLDERS);
    let source = session.source.path.clone();
    session.rows = (0..FILING_FILES)
        .map(|index| {
            filing_row(
                &source,
                index,
                PathBuf::from(format!("scan-{index:03}.pdf")),
                filing_state(index, &rules),
            )
        })
        .collect();
    session.planned = true;
    for (at, folder) in FILING_FOLDERS.iter().enumerate() {
        let there = (0..at % 6 + 1)
            .map(|number| format!("old-{number:03}.pdf"))
            .collect();
        session
            .existing
            .insert((filing_target(), PathBuf::from(folder)), there);
    }
    session.selected = Some(PathBuf::from("scan-004.pdf"));
    if done {
        session.changes.insert(
            PathBuf::from("scan-004.pdf"),
            Change::Places(vec![(
                filing_target(),
                PathBuf::from(FILING_FOLDERS[4]).join("Invoice Acme Ltd 2026-05 Heating.pdf"),
            )]),
        );
    } else {
        filing_is_reading(&mut session, read);
        session
            .progress
            .done
            .store((FILING_FILES - read) / 3, Ordering::Relaxed);
    }
    FilingState {
        session: Some(session),
    }
}

const PROCESSES: [(&str, &str, f32, u64); 14] = [
    ("WindowServer", "_windowserver", 14.2, 612),
    ("Safari", "alex", 9.8, 1_480),
    ("ncrs", "alex", 6.1, 214),
    ("Xcode", "alex", 5.4, 3_210),
    ("kernel_task", "root", 4.9, 1_820),
    ("Slack", "alex", 3.7, 902),
    ("Music", "alex", 2.2, 388),
    ("Mail", "alex", 1.9, 540),
    ("Finder", "alex", 1.4, 256),
    ("Terminal", "alex", 1.1, 120),
    ("mds_stores", "root", 0.9, 410),
    ("cloudd", "alex", 0.6, 88),
    ("Dock", "alex", 0.4, 96),
    ("launchd", "root", 0.1, 18),
];

fn task(index: usize, name: &str, user: &str, cpu: f32, megabytes: u64) -> TaskRow {
    let pid = 100 + 37 * index as u32;
    TaskRow {
        pid,
        start_time: 1_000 + u64::from(pid),
        name: name.to_owned(),
        user: user.to_owned(),
        cpu: Some(cpu),
        resident: megabytes << 20,
        cpu_seconds: 61 * u64::from(pid),
        status: Some(if index < 3 {
            TaskStatus::Running
        } else {
            TaskStatus::Sleeping
        }),
        command: format!("/Applications/{name}.app/Contents/MacOS/{name}"),
        program: Some(PathBuf::from(format!("/Applications/{name}.app/Contents/MacOS")).join(name)),
    }
}

/// The system page of a Mac with 24 programs, drives and sensors.
pub fn monitor(window: iced::Size) -> ncrs_monitor::State {
    let mut tasks: Vec<TaskRow> = PROCESSES
        .iter()
        .enumerate()
        .map(|(index, (name, user, cpu, megabytes))| task(index, name, user, *cpu, *megabytes))
        .collect();
    for index in 0..12 {
        tasks.push(task(
            PROCESSES.len() + index,
            &format!("helper-{index:02}"),
            "alex",
            0.3 - index as f32 * 0.02,
            20 + 9 * index as u64,
        ));
    }
    let total = tasks.len();
    let snapshot = Snapshot {
        host: Some(HostInfo {
            name: "studio".to_owned(),
            os: "macOS 15.4".to_owned(),
            kernel: "Darwin 24.4.0".to_owned(),
            arch: "arm64".to_owned(),
            uptime_seconds: Some(3 * 86_400 + 4 * 3_600),
        }),
        cpu: Some(CpuInfo {
            brand: "Apple M3 Pro".to_owned(),
            cores: 12,
            percent: Some(41.0),
        }),
        memory: Some(Usage {
            total: 36 << 30,
            used: 24 << 30,
            free: 12 << 30,
        }),
        swap: Some(Usage {
            total: 4 << 30,
            used: 1 << 30,
            free: 3 << 30,
        }),
        load: Some(LoadInfo {
            one: 2.4,
            five: 1.9,
            fifteen: 1.5,
        }),
        networks: vec![NetworkRow {
            name: "en0".to_owned(),
            received_per_second: Some(1_240_000),
            transmitted_per_second: Some(86_000),
            has_traffic: true,
        }],
        sensors: vec![
            SensorRow {
                label: "CPU efficiency cores".to_owned(),
                celsius: 41.0,
            },
            SensorRow {
                label: "CPU performance cores".to_owned(),
                celsius: 54.0,
            },
            SensorRow {
                label: "GPU".to_owned(),
                celsius: 46.0,
            },
        ],
        tasks,
        counts: Some(TaskCounts {
            total,
            running: 3,
            sleeping: total - 3,
            other: 0,
        }),
    };
    let ctx = ncrs_monitor::Ctx {
        window_size: window,
        pointer: iced::Point::ORIGIN,
        now: std::time::Instant::now(),
    };
    let mut state = ncrs_monitor::State::default();
    for message in [
        MonitorMessage::Toggle,
        MonitorMessage::Measured(Depth::Full, Box::new(snapshot)),
        MonitorMessage::DrivesMeasured(vec![
            drive("Macintosh HD", "/", "apfs", 994, 412),
            drive("Backup", "/Volumes/Backup", "apfs", 2_000, 1_150),
        ]),
    ] {
        drop(ncrs_monitor::update(&mut state, message, &ctx));
    }
    state
}

fn drive(name: &str, mount: &str, file_system: &str, total_gb: u64, available_gb: u64) -> DriveRow {
    DriveRow {
        name: name.to_owned(),
        mount: PathBuf::from(mount),
        file_system: file_system.to_owned(),
        total: total_gb << 30,
        available: available_gb << 30,
        read_per_second: Some(3 << 20),
        written_per_second: Some(1 << 20),
        is_system_volume: false,
    }
}

/// A document: file, month, sender, kind, tags and euro cents.
type Document = (
    &'static str,
    u32,
    &'static str,
    &'static str,
    &'static [&'static str],
    i64,
);

const DOCUMENTS: [Document; 14] = [
    (
        "telekom-2026-04.pdf",
        4,
        "Telekom",
        "invoice",
        &["paid"],
        10_320,
    ),
    (
        "stadtwerke-2026-04.pdf",
        4,
        "Stadtwerke",
        "invoice",
        &[],
        8_940,
    ),
    (
        "allianz-2026-04.pdf",
        4,
        "Allianz",
        "insurance",
        &["paid"],
        12_450,
    ),
    (
        "payslip-2026-04.pdf",
        4,
        "Initech GmbH",
        "payslip",
        &[],
        312_080,
    ),
    (
        "telekom-2026-03.pdf",
        3,
        "Telekom",
        "invoice",
        &["paid"],
        10_320,
    ),
    (
        "stadtwerke-2026-03.pdf",
        3,
        "Stadtwerke",
        "invoice",
        &["paid"],
        9_270,
    ),
    ("vodafone-2026-03.pdf", 3, "Vodafone", "invoice", &[], 3_999),
    (
        "payslip-2026-03.pdf",
        3,
        "Initech GmbH",
        "payslip",
        &[],
        312_080,
    ),
    (
        "telekom-2026-02.pdf",
        2,
        "Telekom",
        "invoice",
        &["paid"],
        10_320,
    ),
    (
        "stadtwerke-2026-02.pdf",
        2,
        "Stadtwerke",
        "invoice",
        &["paid"],
        10_110,
    ),
    (
        "allianz-2026-02.pdf",
        2,
        "Allianz",
        "insurance",
        &["paid"],
        12_450,
    ),
    (
        "payslip-2026-02.pdf",
        2,
        "Initech GmbH",
        "payslip",
        &[],
        312_080,
    ),
    (
        "telekom-2026-01.pdf",
        1,
        "Telekom",
        "invoice",
        &["paid"],
        10_320,
    ),
    (
        "tax-notice-2026-01.pdf",
        1,
        "Finanzamt",
        "notice",
        &["tax"],
        48_700,
    ),
];

/// The document search with the year filter on and 14 hits of several senders and kinds.
pub fn dense_document_search(base: crate::app::App) -> crate::app::App {
    use ncrs_index::{Amount, Currency, FileStamp};
    use ncrs_search::filters::{FacetValue, SearchFilters};
    use ncrs_search::test_support::found_hit;

    let mut app = base.searching_filters_for_test("2026", |root| {
        let mut filters = SearchFilters::new(Default::default());
        filters.ask("2026");
        let hits: Vec<_> = DOCUMENTS
            .iter()
            .enumerate()
            .map(|(index, (name, month, sender, kind, tags, cents))| {
                let mut hit = found_hit(
                    name,
                    Some((2026, *month, 5)),
                    Some(sender),
                    Some(kind),
                    tags,
                );
                hit.path = root.join(name);
                let file = crate::app::fixed_result_entry(
                    hit.path.clone(),
                    98_304 + index as u64 * 2_048,
                    (2026, *month, 6, 8, 30),
                );
                hit.stamp = file.modified.map(|mtime| FileStamp {
                    size: file.size,
                    mtime,
                });
                hit.fields.amount = Some(Amount {
                    minor_units: *cents,
                    currency: Currency::new("EUR").expect("a currency"),
                });
                hit
            })
            .collect();
        filters.ran(hits);
        filters.toggle(FacetValue::Year(2026));
        filters.add_snippets(
            DOCUMENTS
                .iter()
                .map(|(name, month, ..)| {
                    (
                        root.join(name),
                        format!("Billing period {month}/\u{2}2026\u{3}, amount due on the 5th"),
                    )
                })
                .collect(),
        );
        filters
    });
    let rows = app.search_rows_for_test();
    if let Some(view) = app.search_for_test_mut() {
        view.mirror_index(rows);
    }
    app
}
