//! The generated archive of the 1.7.0 acceptance: invented senders, a German
//! folder structure and an inbox that holds every kind of question. Text files,
//! so the test needs no PDF library; the same text always comes out.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use ncrs_index::{Condition, ConditionKind, DocumentFields, RuleConditions, RuleSpec};

pub const YEARS: [i32; 4] = [2022, 2023, 2024, 2025];

/// The archive of the final run has about 2,000 files and the inbox 300; the
/// default is a tenth of it, still with several documents of every question.
/// `NCRS_ACCEPTANCE_FULL=1` builds the full one.
pub fn full_scale() -> bool {
    std::env::var_os("NCRS_ACCEPTANCE_FULL").is_some()
}

pub fn scaled(full: usize, small: usize) -> usize {
    if full_scale() {
        full
    } else {
        small
    }
}

/// One kind of letter: who writes it, where the rule files it, how many of
/// them the archive holds already.
pub struct Category {
    pub key: &'static str,
    pub sender: &'static str,
    pub street: &'static str,
    pub city: &'static str,
    pub pattern: &'static str,
    /// A confirmed rule exists for the sender.
    pub rule: bool,
    /// The rule reads the amount; the pattern needs nothing of it.
    pub amount_rule: bool,
    pub in_archive: usize,
    pub number_base: u32,
    pub heading: &'static str,
}

pub const CATEGORIES: [Category; 11] = [
    Category {
        key: "strom",
        sender: "Nordlicht Energie GmbH",
        street: "Musterweg 1",
        city: "20095 Hansestadt",
        pattern: "Verträge/Strom/{date} Stromrechnung {number}",
        rule: true,
        amount_rule: false,
        in_archive: 220,
        number_base: 10,
        heading: "Rechnung",
    },
    Category {
        key: "internet",
        sender: "Kabelnetz Brandmeier GmbH",
        street: "Leitungsallee 12",
        city: "20095 Hansestadt",
        pattern: "Verträge/Internet/{date} Internetrechnung {number}",
        rule: true,
        amount_rule: false,
        in_archive: 160,
        number_base: 11,
        heading: "Rechnung",
    },
    Category {
        key: "wasser",
        sender: "Wasserwerk Auental AG",
        street: "Quellstraße 3",
        city: "30159 Auental",
        pattern: "Finanzen/Rechnungen/{year}/{date} {sender} {number}",
        rule: true,
        amount_rule: false,
        in_archive: 260,
        number_base: 12,
        heading: "Rechnung",
    },
    Category {
        key: "elektro",
        sender: "Elektro Sommerfeld GmbH",
        street: "Handwerkerhof 7",
        city: "30159 Auental",
        pattern: "Finanzen/Rechnungen/{year}/{date} {sender} {number}",
        rule: true,
        amount_rule: false,
        in_archive: 220,
        number_base: 13,
        heading: "Rechnung",
    },
    Category {
        key: "buch",
        sender: "Buchhandlung Wiesengrund GmbH",
        street: "Marktplatz 2",
        city: "30159 Auental",
        pattern: "Finanzen/Rechnungen/{year}/{date} {sender} {number}",
        rule: true,
        amount_rule: false,
        in_archive: 200,
        number_base: 14,
        heading: "Rechnung",
    },
    Category {
        key: "konto",
        sender: "Sparkasse Mittelgau",
        street: "Am Rathaus 1",
        city: "40210 Mittelgau",
        pattern: "Finanzen/Kontoauszüge/{date} Kontoauszug {number}",
        rule: true,
        amount_rule: false,
        in_archive: 330,
        number_base: 15,
        heading: "Kontoauszug",
    },
    Category {
        key: "versicherung",
        sender: "Versicherungskammer Falkenhain AG",
        street: "Schutzgasse 9",
        city: "50667 Falkenhain",
        pattern: "Versicherungen/{sender}/{date} Police {number}",
        rule: true,
        amount_rule: true,
        in_archive: 200,
        number_base: 16,
        heading: "Rechnung",
    },
    Category {
        key: "steuer",
        sender: "Finanzamt Obertal",
        street: "Steuerplatz 4",
        city: "60311 Obertal",
        pattern: "Steuer/{year}/{date} Bescheid {number}",
        rule: true,
        amount_rule: false,
        in_archive: 160,
        number_base: 17,
        heading: "Bescheid",
    },
    Category {
        key: "gesundheit",
        sender: "Praxis Quast GmbH",
        street: "Kurallee 5",
        city: "70173 Heilbrunn",
        pattern: "Gesundheit/{date} {sender} {number}",
        rule: true,
        amount_rule: false,
        in_archive: 140,
        number_base: 18,
        heading: "Rechnung",
    },
    Category {
        key: "auto",
        sender: "Autohaus Lindenfels KG",
        street: "Werkstattring 8",
        city: "80331 Lindenfels",
        pattern: "Auto/{date} Werkstatt {number}",
        rule: true,
        amount_rule: false,
        in_archive: 110,
        number_base: 19,
        heading: "Rechnung",
    },
    // No rule: the neighbours in the archive are all it has.
    Category {
        key: "miete",
        sender: "Hausverwaltung Birkenhof GmbH",
        street: "Birkenweg 14",
        city: "90402 Birkenhof",
        pattern: "Verträge/Miete/{date} Miete {number}",
        rule: false,
        amount_rule: false,
        in_archive: 80,
        number_base: 20,
        heading: "Rechnung",
    },
];

pub const NO_RULE_SENDER: &str = "Gartenbau Rosenthal GmbH";
pub const UNCONFIRMED_SENDER: &str = "Zeitungsverlag Morgenpost KG";
pub const UNCONFIRMED_PATTERN: &str = "Abos/{sender}/{date} Abo {number}";

pub fn category(key: &str) -> &'static Category {
    CATEGORIES
        .iter()
        .find(|category| category.key == key)
        .unwrap_or_else(|| panic!("no category {key}"))
}

impl Category {
    pub fn number(&self, serial: usize) -> String {
        format!("{}{:04}", self.number_base, serial)
    }

    pub fn fields(&self, date: Option<NaiveDate>, serial: usize) -> DocumentFields {
        DocumentFields {
            sender: Some(self.sender.to_owned()),
            date,
            number: Some(self.number(serial)),
            ..DocumentFields::default()
        }
    }

    /// The path below the archive that the rule files this letter in.
    pub fn target(&self, date: NaiveDate, serial: usize) -> PathBuf {
        let relative = ncrs_intake::render_pattern(self.pattern, &self.fields(Some(date), serial))
            .unwrap_or_else(|| panic!("pattern {} does not render", self.pattern));
        let mut name = relative.into_os_string();
        name.push(".txt");
        PathBuf::from(name)
    }

    pub fn letter(&self, date: Option<NaiveDate>, serial: usize, amount: Option<&str>) -> String {
        letter_of(
            (self.sender, self.street, self.city),
            self.heading,
            &self.number(serial),
            date,
            amount,
            serial,
        )
    }

    pub fn rule_spec(&self) -> RuleSpec {
        rule(
            self.sender,
            self.pattern,
            true,
            self.amount_rule
                .then(|| r"Betrag: ([\d.,]+) EUR".to_owned()),
        )
    }
}

pub fn letter_of(
    letterhead: (&str, &str, &str),
    heading: &str,
    number: &str,
    date: Option<NaiveDate>,
    amount: Option<&str>,
    salt: usize,
) -> String {
    let (sender, street, city) = letterhead;
    let mut text = format!("{sender}\n{street}\n{city}\n\n{heading} Nr. {number}\n");
    if let Some(day) = date {
        text.push_str(&format!("Datum: {}\n", day.format("%d.%m.%Y")));
    }
    if let Some(sum) = amount {
        text.push_str(&format!("Betrag: {sum} EUR\n"));
    }
    text.push_str(&format!(
        "\nVielen Dank für Ihren Auftrag. Vorgang {salt} wurde bearbeitet.\n"
    ));
    text
}

pub fn rule(
    sender: &str,
    pattern: &str,
    confirmed: bool,
    extract_amount: Option<String>,
) -> RuleSpec {
    RuleSpec {
        name: Some(sender.to_owned()),
        conditions: RuleConditions::all_of(vec![Condition::new(
            ConditionKind::Substring,
            sender.to_lowercase(),
        )]),
        extract_number: Some(r"Nr\. (\d+)".to_owned()),
        extract_date: None,
        extract_amount,
        kind: None,
        tags: BTreeSet::new(),
        filing_pattern: pattern.to_owned(),
        confirmed,
        enabled: true,
    }
}

pub fn rules() -> Vec<RuleSpec> {
    let mut specs: Vec<RuleSpec> = CATEGORIES
        .iter()
        .filter(|category| category.rule)
        .map(Category::rule_spec)
        .collect();
    specs.push(rule(UNCONFIRMED_SENDER, UNCONFIRMED_PATTERN, false, None));
    specs
}

/// A day of the archive years, spread over the calendar by `serial`.
pub fn day(serial: usize) -> NaiveDate {
    let year = YEARS[serial % YEARS.len()];
    let month = u32::try_from(serial * 5 % 12).unwrap_or(0) + 1;
    let day = u32::try_from(serial * 7 % 28).unwrap_or(0) + 1;
    NaiveDate::from_ymd_opt(year, month, day).unwrap_or_default()
}

fn money(serial: usize) -> String {
    format!("{},{:02}", 20 + serial * 13 % 900, serial * 17 % 100)
}

fn put(path: &Path, text: &str) {
    super::super::write(path, text);
}

/// What the archive holds of each category, by serial number, so the inbox can
/// copy from it.
pub struct ArchiveFile {
    pub category: &'static Category,
    pub path: PathBuf,
    pub text: String,
}

/// Writes the archive below `root` and returns what it holds.
pub fn write_archive(root: &Path) -> Vec<ArchiveFile> {
    let mut written = Vec::new();
    for category in &CATEGORIES {
        for serial in 0..scaled(
            category.in_archive,
            category.in_archive.checked_div(10).unwrap_or(0),
        ) {
            let date = day(serial + category.number_base as usize);
            let amount = category.amount_rule.then(|| money(serial));
            let text = category.letter(Some(date), serial, amount.as_deref());
            let path = root.join(category.target(date, serial));
            put(&path, &text);
            written.push(ArchiveFile {
                category,
                path,
                text,
            });
        }
    }
    written
}

/// What the inbox holds, by what Hermes is to make of it.
#[derive(Default)]
pub struct Inbox {
    /// A confirmed rule files these by itself.
    pub sure: Vec<(PathBuf, PathBuf)>,
    /// The same bytes as an archive file, which lies in the place they would take.
    pub duplicates: Vec<(PathBuf, PathBuf)>,
    /// The place of an archive file with other content.
    pub clashes: Vec<(PathBuf, PathBuf)>,
    pub no_date: Vec<PathBuf>,
    pub no_amount: Vec<PathBuf>,
    /// A sender the rules and the neighbours know, which the test marks as
    /// "not sure who sent it".
    pub unsure_sender: Vec<PathBuf>,
    pub no_rule: Vec<PathBuf>,
    pub unconfirmed: Vec<PathBuf>,
    /// Like the unconfirmed ones; the test marks them as "not sure who sent
    /// it", which no scripted model can find out by itself.
    pub unsure_sender_marked: Vec<PathBuf>,
}

fn inbox_name(counter: &mut usize, label: &str) -> String {
    *counter += 1;
    format!("Scan {:04} {label}.txt", *counter)
}

/// Writes `total`-ish documents into `inbox`, in the mix the questions need.
pub fn write_inbox(inbox: &Path, archive_root: &Path, archive: &[ArchiveFile]) -> Inbox {
    let mut counter = 0;
    let mut found = Inbox::default();
    let by_key = |key: &str| -> Vec<&ArchiveFile> {
        archive
            .iter()
            .filter(|file| file.category.key == key)
            .collect()
    };
    // Letters the rules know, with numbers the archive does not have yet.
    for round in 0..scaled(16, 1) {
        for category in CATEGORIES.iter().filter(|category| category.rule) {
            let serial = 5000 + round;
            let date = day(serial);
            let amount = category.amount_rule.then(|| money(serial));
            let name = inbox_name(&mut counter, category.key);
            put(
                &inbox.join(&name),
                &category.letter(Some(date), serial, amount.as_deref()),
            );
            found.sure.push((
                inbox.join(name),
                archive_root.join(category.target(date, serial)),
            ));
        }
    }
    let strom = by_key("strom");
    let internet = by_key("internet");
    for file in strom.iter().take(scaled(10, 3)) {
        let name = inbox_name(&mut counter, "Kopie");
        put(&inbox.join(&name), &file.text);
        found.duplicates.push((inbox.join(name), file.path.clone()));
    }
    for file in internet.iter().take(scaled(10, 3)) {
        let name = inbox_name(&mut counter, "Zweitausfertigung");
        put(&inbox.join(&name), &file.text);
        found.duplicates.push((inbox.join(name), file.path.clone()));
    }
    for file in by_key("wasser").iter().take(scaled(10, 3)) {
        let name = inbox_name(&mut counter, "korrigiert");
        let text = format!("{}Korrigierte Fassung.\n", file.text);
        put(&inbox.join(&name), &text);
        found.clashes.push((inbox.join(name), file.path.clone()));
    }
    for file in by_key("elektro").iter().take(scaled(10, 3)) {
        let name = inbox_name(&mut counter, "korrigiert");
        let text = format!("{}Korrigierte Fassung.\n", file.text);
        put(&inbox.join(&name), &text);
        found.clashes.push((inbox.join(name), file.path.clone()));
    }
    for (offset, key) in ["buch", "wasser", "steuer"]
        .into_iter()
        .cycle()
        .take(scaled(25, 6))
        .enumerate()
    {
        let category = category(key);
        let serial = 6000 + offset;
        let name = inbox_name(&mut counter, "ohne Datum");
        put(&inbox.join(&name), &category.letter(None, serial, None));
        found.no_date.push(inbox.join(name));
    }
    for serial in 0..scaled(25, 4) {
        let category = category("versicherung");
        let name = inbox_name(&mut counter, "ohne Betrag");
        let date = day(7000 + serial);
        put(
            &inbox.join(&name),
            &category.letter(Some(date), 7000 + serial, None),
        );
        found.no_amount.push(inbox.join(name));
    }
    for serial in 0..scaled(15, 4) {
        let category = category("miete");
        let name = inbox_name(&mut counter, "Hausverwaltung");
        let date = day(8000 + serial);
        put(
            &inbox.join(&name),
            &category.letter(Some(date), 8000 + serial, None),
        );
        found.unsure_sender.push(inbox.join(name));
    }
    for serial in 0..scaled(20, 4) {
        let name = inbox_name(&mut counter, "Gartenbau");
        let text = letter_of(
            (NO_RULE_SENDER, "Rosenweg 6", "99084 Rosenthal"),
            "Rechnung",
            &format!("9{serial:04}"),
            Some(day(9000 + serial)),
            Some(&money(serial)),
            serial,
        );
        put(&inbox.join(&name), &text);
        found.no_rule.push(inbox.join(name));
    }
    for serial in 0..scaled(15, 5) {
        let name = inbox_name(&mut counter, "Zeitung");
        let text = letter_of(
            (UNCONFIRMED_SENDER, "Druckerstraße 2", "99084 Rosenthal"),
            "Rechnung",
            &format!("8{serial:04}"),
            Some(day(9500 + serial)),
            None,
            serial,
        );
        put(&inbox.join(&name), &text);
        found.unconfirmed.push(inbox.join(name));
    }
    for serial in 0..scaled(10, 4) {
        let name = inbox_name(&mut counter, "Zeitung Absender");
        let text = letter_of(
            (UNCONFIRMED_SENDER, "Druckerstraße 2", "99084 Rosenthal"),
            "Rechnung",
            &format!("7{serial:04}"),
            Some(day(9700 + serial)),
            None,
            serial + 100,
        );
        put(&inbox.join(&name), &text);
        found.unsure_sender_marked.push(inbox.join(name));
    }
    found
}

/// What the test adds while the continuous filing runs.
#[derive(Default)]
pub struct Arrivals {
    /// The sender has no rule but many neighbours in one folder: the place is
    /// sure. The path below the archive each should land in.
    pub neighbours: Vec<(PathBuf, PathBuf)>,
    /// A rule files these.
    pub by_rule: Vec<(PathBuf, PathBuf)>,
    /// Nobody knows the sender.
    pub strangers: Vec<PathBuf>,
}

pub const STRANGER_SENDER: &str = "Tierarztpraxis Hasenbau GmbH";

pub fn write_arrivals(inbox: &Path, archive_root: &Path) -> Arrivals {
    let mut arrivals = Arrivals::default();
    let miete = category("miete");
    for serial in 0..scaled(12, 4) {
        let number = 8500 + serial;
        let date = day(number);
        let path = inbox.join(format!("Neu {number} Hausverwaltung.txt"));
        put(&path, &miete.letter(Some(date), number, None));
        arrivals
            .neighbours
            .push((path, archive_root.join(miete.target(date, number))));
    }
    let strom = category("strom");
    for serial in 0..scaled(6, 3) {
        let number = 8600 + serial;
        let date = day(number);
        let path = inbox.join(format!("Neu {number} Strom.txt"));
        put(&path, &strom.letter(Some(date), number, None));
        arrivals
            .by_rule
            .push((path, archive_root.join(strom.target(date, number))));
    }
    for serial in 0..scaled(6, 3) {
        let path = inbox.join(format!("Neu {serial} Tierarzt.txt"));
        let text = letter_of(
            (STRANGER_SENDER, "Pfotenweg 3", "99084 Rosenthal"),
            "Rechnung",
            &format!("7{serial:04}"),
            Some(day(8700 + serial)),
            None,
            serial,
        );
        put(&path, &text);
        arrivals.strangers.push(path);
    }
    arrivals
}

/// One more letter that a confirmed rule files: where it comes from and where
/// it lands.
pub fn write_rule_arrival(inbox: &Path, archive_root: &Path, number: usize) -> (PathBuf, PathBuf) {
    let strom = category("strom");
    let date = day(number);
    let path = inbox.join(format!("Neu {number} Strom.txt"));
    put(&path, &strom.letter(Some(date), number, None));
    (path, archive_root.join(strom.target(date, number)))
}
