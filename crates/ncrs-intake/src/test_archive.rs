//! A deterministic test archive with an inbox: a household's paperwork as
//! letters in German, laid out the way people file them (years, insurers,
//! cars), with exact file counts, copies of documents and undecodable photos.
//! Built in memory; `write_to` puts it on disk.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

pub const OWN_NAME: &str = "Marc Beispiel";

const OWN_BLOCK: &str = "Marc Beispiel\nAndrestr. 12\n09112 Chemnitz";
const SEED: u64 = 7;
const YEARS: [i32; 9] = [2016, 2017, 2018, 2019, 2020, 2021, 2022, 2023, 2024];
const INBOX_FILES: usize = 300;
const INBOX_PHOTOS: usize = 72;
const FILING_COPIES: usize = 58;
const DOWNLOAD_COPIES: usize = 40;

/// Small, fast, and the same on every platform.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `0..n`; `n` must not be 0.
    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next_u64() % u64::try_from(n).expect("fits")).expect("below n")
    }

    pub fn range_inclusive(&mut self, lo: u32, hi: u32) -> u32 {
        let span = u64::from(hi - lo) + 1;
        lo + u32::try_from(self.next_u64() % span).expect("below span")
    }

    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// `n` distinct indices below `pool_len`, in random order.
    pub fn sample(&mut self, pool_len: usize, n: usize) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..pool_len).collect();
        for position in 0..n {
            let other = position + self.below(pool_len - position);
            indices.swap(position, other);
        }
        indices.truncate(n);
        indices
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestContent {
    Text(String),
    /// A JPEG header followed by noise: it never decodes.
    Photo(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFile {
    /// Below `archive/` or `inbox/`.
    pub relative_path: PathBuf,
    pub content: TestContent,
    /// The letter date written in the text; `None` for photos and for the
    /// texts of identity card copies.
    pub date: Option<NaiveDate>,
}

impl TestFile {
    pub fn text(&self) -> Option<&str> {
        match &self.content {
            TestContent::Text(text) => Some(text),
            TestContent::Photo(_) => None,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match &self.content {
            TestContent::Text(text) => text.as_bytes(),
            TestContent::Photo(bytes) => bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestArchive {
    pub archive: Vec<TestFile>,
    pub inbox: Vec<TestFile>,
}

impl TestArchive {
    /// `archive/` and `inbox/` below `root`.
    pub fn write_to(&self, root: &Path) -> io::Result<()> {
        for (folder, files) in [("archive", &self.archive), ("inbox", &self.inbox)] {
            let base = root.join(folder);
            std::fs::create_dir_all(&base)?;
            for file in files {
                let path = base.join(&file.relative_path);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(path, file.bytes())?;
            }
        }
        Ok(())
    }
}

struct Letter<'a> {
    sender: &'a str,
    address: &'a str,
    heading: String,
    body: String,
    extra: &'a str,
}

impl<'a> Letter<'a> {
    fn new(
        sender: &'a str,
        address: &'a str,
        heading: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            sender,
            address,
            heading: heading.into(),
            body: body.into(),
            extra: "",
        }
    }

    fn extra(mut self, extra: &'a str) -> Self {
        self.extra = extra;
        self
    }
}

fn german_date(date: NaiveDate) -> String {
    date.format("%d.%m.%Y").to_string()
}

fn date_of(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
}

struct Builder {
    rng: SplitMix64,
    files: Vec<TestFile>,
    taken: HashSet<PathBuf>,
}

impl Builder {
    fn random_date(&mut self, year: i32, month: Option<u32>) -> NaiveDate {
        let month = month.unwrap_or_else(|| self.rng.range_inclusive(1, 12));
        let day = self.rng.range_inclusive(1, 28);
        date_of(year, month, day)
    }

    fn random_year(&mut self) -> i32 {
        *self.rng.choose(&YEARS)
    }

    fn letter_text(&mut self, letter: &Letter<'_>, date: NaiveDate) -> String {
        let Letter {
            sender,
            address,
            heading,
            body,
            extra,
        } = letter;
        let register = self.rng.range_inclusive(1000, 99999);
        format!(
            "{sender}\n{address}\n\n{OWN_BLOCK}\n\nChemnitz, {}\n\n{heading}\n\n{extra}\n{body}\n\n\
             Mit freundlichen Grüßen\n{sender}\nAmtsgericht Chemnitz HRB {register}\n",
            german_date(date)
        )
    }

    fn is_free(&self, relative: &str) -> bool {
        !self.taken.contains(Path::new(relative))
    }

    fn add(&mut self, relative: &str, text: String, date: Option<NaiveDate>) {
        let path = PathBuf::from(relative);
        assert!(self.taken.insert(path.clone()), "{relative} written twice");
        self.files.push(TestFile {
            relative_path: path,
            content: TestContent::Text(text),
            date,
        });
    }

    fn add_letter(&mut self, relative: &str, letter: &Letter<'_>, date: NaiveDate) {
        let text = self.letter_text(letter, date);
        self.add(relative, text, Some(date));
    }

    fn photo(&mut self) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF];
        for _ in 0..8 {
            bytes.extend(self.rng.next_u64().to_le_bytes());
        }
        bytes
    }

    fn add_photo(&mut self, relative: &str) {
        let path = PathBuf::from(relative);
        assert!(self.taken.insert(path.clone()), "{relative} written twice");
        let bytes = self.photo();
        self.files.push(TestFile {
            relative_path: path,
            content: TestContent::Photo(bytes),
            date: None,
        });
    }

    fn bank(&mut self) {
        for year in 2018..=2024 {
            for month in 1..=12 {
                let date = date_of(year, month, 28);
                let letter = Letter::new(
                    "Sparkasse Chemnitz",
                    "Moritzhof 1, 09111 Chemnitz",
                    "Kontoauszug",
                    &format!(
                        "Kontoauszug Nr. {month} / {year}\nIBAN DE12 8705 0000 0712 3456 78\nAlter Kontostand 1.234,56 EUR"
                    ),
                )
                .extra("Kundennummer: 4711002");
                self.add_letter(
                    &format!(
                        "Bank/Sparkasse Chemnitz/Kontoauszüge/{year}/Kontoauszug_{date}.pdf"
                    ),
                    &letter,
                    date,
                );
            }
        }
        for year in 2020..=2024 {
            for _ in 0..30 {
                let date = self.random_date(year, None);
                let order = self.rng.range_inclusive(10_000_000, 99_999_999);
                let letter = Letter::new(
                    "Scalable Capital GmbH",
                    "Seitzstraße 8e, 80538 München",
                    "Wertpapierabrechnung Kauf",
                    &format!("Auftragsnummer {order}\nDepot 0815-4711\nKurswert 500,00 EUR"),
                );
                self.add_letter(
                    &format!("Bank/Scalable Capital/{year}/Wertpapierabrechnung_{date}_{order}.pdf"),
                    &letter,
                    date,
                );
            }
        }
        for year in 2019..=2024 {
            for month in (1..=12).step_by(2) {
                let date = self.random_date(year, Some(month));
                let letter = Letter::new(
                    "DKB Deutsche Kreditbank AG",
                    "Taubenstraße 7-9, 10117 Berlin",
                    "Kreditkartenabrechnung",
                    "Kartennummer 4930 **** **** 1234\nSaldo 312,40 EUR",
                );
                self.add_letter(
                    &format!(
                        "Bank/DKB/Kreditkarte/{year}/Kreditkartenabrechnung {}.pdf",
                        german_date(date)
                    ),
                    &letter,
                    date,
                );
            }
        }
        self.financing();
    }

    fn financing(&mut self) {
        const OBJECTS: [(&str, &str); 3] = [
            ("Chemnitz-Andrestr. 12", "595610814"),
            ("Leipzig-Kurt-Eisner-Str. 5", "601122334"),
            ("Dresden-Neustadt WE 3", "587700112"),
        ];
        const DOCUMENTS: [&str; 10] = [
            "perso1",
            "perso2",
            "Darlehensvertrag",
            "Grundschuldbestellung",
            "SEPA-Mandat",
            "Kaufvertrag Notar",
            "Selbstauskunft",
            "Gehaltsnachweise",
            "Objektunterlagen",
            "Wertermittlung",
        ];
        const SENDERS: [&str; 3] = ["Sparkasse Chemnitz", "Interhyp AG", "Notar Dr. Weber"];
        for (object, loan) in OBJECTS {
            for stage in ["Entwurf", "Unterschrieben", "Korrespondenz"] {
                for document in DOCUMENTS {
                    let identity_copy = document.starts_with("perso");
                    if stage == "Korrespondenz" && identity_copy {
                        continue;
                    }
                    let relative = format!("Bank/Finanzierung/{object}/{stage}/{document}.pdf");
                    if identity_copy {
                        let text = format!(
                            "BUNDESREPUBLIK DEUTSCHLAND\nPERSONALAUSWEIS\nBEISPIEL\nMARC\nDarlehensnummer {loan}\n"
                        );
                        self.add(&relative, text, None);
                        continue;
                    }
                    let date = self.random_date(2021, None);
                    let sender = *self.rng.choose(&SENDERS);
                    let letter = Letter::new(
                        sender,
                        "Hauptstraße 3, 09111 Chemnitz",
                        document,
                        &format!(
                            "Darlehensnummer {loan}\nObjekt {object}\nBitte unterschrieben zurücksenden."
                        ),
                    );
                    self.add_letter(&relative, &letter, date);
                }
            }
        }
    }

    fn tax(&mut self) {
        const KINDS: [&str; 5] = [
            "Spendenquittung",
            "Handwerkerrechnung",
            "Arbeitsmittel",
            "Fahrtkosten",
            "Kinderbetreuung",
        ];
        const SENDERS: [&str; 4] = [
            "Malerbetrieb Kunze GmbH",
            "Deutsches Rotes Kreuz e.V.",
            "Amazon EU S.a.r.l.",
            "Kita Sonnenschein",
        ];
        for year in YEARS {
            let date = self.random_date(if year < 2024 { year + 1 } else { 2024 }, None);
            let letter = Letter::new(
                "Finanzamt Chemnitz-Süd",
                "Paul-Bertz-Str. 1, 09120 Chemnitz",
                &format!("Bescheid für {year} über Einkommensteuer"),
                "Steuernummer 215/123/45678\nFestgesetzt werden 1.234 EUR",
            );
            self.add_letter(
                &format!("Steuer/{year}/Bescheid/Einkommensteuerbescheid {year}.pdf"),
                &letter,
                date,
            );
            let date = self.random_date(year, None);
            let ticket = self.rng.range_inclusive(100_000_000, 999_999_999);
            let letter = Letter::new(
                "ELSTER",
                "Bayerisches Landesamt für Steuern",
                "Übertragungsprotokoll",
                &format!("Transferticket et{ticket}"),
            );
            self.add_letter(
                &format!("Steuer/{year}/Erklärung/ELSTER_Übertragungsprotokoll.pdf"),
                &letter,
                date,
            );
            for number in 1..=25 {
                let date = self.random_date(year, None);
                let kind = *self.rng.choose(&KINDS);
                let sender = *self.rng.choose(&SENDERS);
                let invoice = self.rng.range_inclusive(1000, 9999);
                let amount = self.rng.range_inclusive(10, 900);
                let letter = Letter::new(
                    sender,
                    "Am Markt 1, 09111 Chemnitz",
                    kind,
                    &format!("Rechnungsnummer R-{invoice}\nBetrag {amount},00 EUR"),
                );
                self.add_letter(
                    &format!("Steuer/{year}/Belege/{kind}_{number:02}.pdf"),
                    &letter,
                    date,
                );
            }
        }
    }

    fn insurance(&mut self) {
        const INSURERS: [(&str, &str, [usize; 3]); 4] = [
            ("Allianz", "Allianz Versicherungs-AG", [18, 17, 16]),
            ("HUK-Coburg", "HUK-COBURG", [17, 18, 16]),
            ("Kranich Versicherung", "Kranich Versicherung AG", [17, 16, 18]),
            ("ERGO", "ERGO Versicherung AG", [17, 17, 16]),
        ];
        for (folder, name, counts) in INSURERS {
            let policy = self.rng.range_inclusive(100_000_000, 999_999_999);
            for (area, count) in ["Vertrag", "Beitragsrechnung", "Schaden"].into_iter().zip(counts) {
                for number in 1..=count {
                    let year = self.random_year();
                    let date = self.random_date(year, None);
                    let heading = if area == "Vertrag" { "Versicherungsschein" } else { area };
                    let letter = Letter::new(
                        name,
                        "Königinstraße 28, 80802 München",
                        heading,
                        &format!("Versicherungsnummer {policy}\nBeitrag 45,90 EUR"),
                    );
                    self.add_letter(
                        &format!("Versicherung/{folder}/{area}/{year} {area} {number}.pdf"),
                        &letter,
                        date,
                    );
                }
            }
        }
    }

    fn cars(&mut self) {
        const CARS: [(&str, &str, [usize; 4]); 2] = [
            ("VW Golf", "C-AB 123", [20, 15, 12, 18]),
            ("Skoda Octavia", "C-XY 987", [17, 14, 16, 17]),
        ];
        const AREAS: [(&str, &str); 4] = [
            ("Werkstatt", "Autohaus Süd GmbH"),
            ("TÜV", "TÜV SÜD Auto Service GmbH"),
            ("Kfz-Steuer", "Hauptzollamt Dresden"),
            ("Tanken", "Aral AG"),
        ];
        for (car, plate, counts) in CARS {
            for ((area, sender), count) in AREAS.into_iter().zip(counts) {
                for _ in 0..count {
                    let (date, relative) = loop {
                        let year = self.random_year();
                        let date = self.random_date(year, None);
                        let relative = format!("Auto/{car}/{area}/{date} {area}.pdf");
                        if self.is_free(&relative) {
                            break (date, relative);
                        }
                    };
                    let heading = if area == "Kfz-Steuer" { "Kraftfahrzeugsteuerbescheid" } else { "Rechnung" };
                    let chassis = self.rng.range_inclusive(100_000, 999_999);
                    let letter = Letter::new(
                        sender,
                        "Zwickauer Str. 100, 09116 Chemnitz",
                        heading,
                        &format!("Kennzeichen {plate}\nFahrgestellnummer WVWZZZ1KZ{chassis}"),
                    );
                    self.add_letter(&relative, &letter, date);
                }
            }
        }
    }

    fn home(&mut self) {
        const TRADES: [&str; 3] = ["Heizung", "Elektro", "Fenster"];
        const CRAFTSMEN: [&str; 3] = ["Heizung Schulze", "Elektro Lange GmbH", "Fensterbau Roth"];
        for year in YEARS {
            let date = self.random_date(year, None);
            let letter = Letter::new(
                "Hausverwaltung Meyer & Co. KG",
                "Brühl 5, 09111 Chemnitz",
                "Betriebskostenabrechnung",
                "Objekt Andrestr. 12, WE 4\nNachzahlung 120,00 EUR",
            );
            self.add_letter(
                &format!("Wohnung/Andrestr/Nebenkosten/{year}/Nebenkostenabrechnung_{year}.pdf"),
                &letter,
                date,
            );
            for _ in 0..6 {
                let (date, relative) = loop {
                    let date = self.random_date(year, None);
                    let trade = *self.rng.choose(&TRADES);
                    let relative = format!("Wohnung/Andrestr/Handwerker/{date}_{trade}.pdf");
                    if self.is_free(&relative) {
                        break (date, relative);
                    }
                };
                let craftsman = *self.rng.choose(&CRAFTSMEN);
                let invoice = self.rng.range_inclusive(1000, 9999);
                let letter = Letter::new(
                    craftsman,
                    "Werkstr. 2, 09117 Chemnitz",
                    "Rechnung",
                    &format!("Rechnungsnummer {invoice}"),
                );
                self.add_letter(&relative, &letter, date);
            }
        }
        for document in ["Mietvertrag", "Übergabeprotokoll", "Hausordnung", "Kaution"] {
            let date = self.random_date(2016, None);
            let letter = Letter::new(
                "Hausverwaltung Meyer & Co. KG",
                "Brühl 5, 09111 Chemnitz",
                document,
                "Objekt Andrestr. 12",
            );
            self.add_letter(&format!("Wohnung/Andrestr/Mietvertrag/{document}.pdf"), &letter, date);
        }
    }

    fn work_and_health(&mut self) {
        for year in 2017..=2024 {
            for month in 1..=12 {
                let date = date_of(year, month, 25);
                let letter = Letter::new(
                    "Cosmicdrift GmbH",
                    "Str. der Nationen 1, 09111 Chemnitz",
                    "Entgeltabrechnung",
                    "Personalnummer 00042\nNetto 3.100,00 EUR",
                );
                self.add_letter(
                    &format!("Arbeit/Gehalt/{year}/Gehaltsabrechnung_{date}.pdf"),
                    &letter,
                    date,
                );
            }
        }
        const DOCTORS: [(&str, &str, usize); 3] = [
            ("Zahnarzt Dr. Müller", "Zahnarztpraxis Dr. Müller", 30),
            ("Hausarzt", "Praxis Dr. Schmidt", 25),
            ("Krankenkasse TK", "Techniker Krankenkasse", 25),
        ];
        for (doctor, sender, count) in DOCTORS {
            for _ in 0..count {
                let (date, relative) = loop {
                    let year = self.random_year();
                    let date = self.random_date(year, None);
                    let relative = format!("Gesundheit/{doctor}/Rechnung_{}.pdf", date.format("%Y%m%d"));
                    if self.is_free(&relative) {
                        break (date, relative);
                    }
                };
                let heading = if doctor.contains("TK") { "Mitteilung" } else { "Rechnung" };
                let letter = Letter::new(
                    sender,
                    "Markt 3, 09111 Chemnitz",
                    heading,
                    "Patient Marc Beispiel\nVersichertennummer A123456789",
                );
                self.add_letter(&relative, &letter, date);
            }
        }
    }

    fn children(&mut self) {
        const COUNTS: [(&str, [usize; 3]); 2] = [("Lena", [12, 10, 9]), ("Paul", [11, 10, 10])];
        const AREAS: [(&str, &str); 3] = [
            ("Kita", "Kita Sonnenschein"),
            ("Schule/Zeugnisse", "Grundschule am Park"),
            ("Arzt", "Grundschule am Park"),
        ];
        for (child, counts) in COUNTS {
            for ((area, sender), count) in AREAS.into_iter().zip(counts) {
                let topic = area.rsplit('/').next().unwrap_or(area);
                for number in 1..=count {
                    let year = self.random_year();
                    let date = self.random_date(year, None);
                    let letter = Letter::new(
                        sender,
                        "Parkstr. 1, 09111 Chemnitz",
                        topic,
                        &format!("Kind {child} Beispiel"),
                    );
                    self.add_letter(
                        &format!("Kinder/{child}/{area}/{child}_{topic}_{year}_{number}.pdf"),
                        &letter,
                        date,
                    );
                }
            }
        }
    }

    fn unsorted(&mut self) {
        const SENDERS: [&str; 4] = [
            "Stadt Chemnitz",
            "Deutsche Rentenversicherung",
            "Rundfunk ARD ZDF Beitragsservice",
            "Vodafone GmbH",
        ];
        const HEADINGS: [&str; 4] = ["Bescheid", "Rechnung", "Mitteilung", "Renteninformation"];
        for number in 1..=150 {
            let year = self.random_year();
            let date = self.random_date(year, None);
            let sender = *self.rng.choose(&SENDERS);
            let heading = *self.rng.choose(&HEADINGS);
            let letter = Letter::new(sender, "Postfach 1, 09111 Chemnitz", heading, "Aktenzeichen 12/345");
            self.add_letter(&format!("Scans/scan{number:04}.pdf"), &letter, date);
        }
        for number in 1..=120 {
            self.add_photo(&format!("Fotos Belege/IMG_{}.jpg", 2000 + number));
        }
    }

    /// Copies of documents: filed under the tax year, and dumped in a download folder.
    fn copies(&mut self) {
        let pool: Vec<(PathBuf, String, Option<NaiveDate>)> = self
            .files
            .iter()
            .filter_map(|file| {
                let path = file.relative_path.to_str()?;
                let belongs = path.starts_with("Versicherung")
                    || path.starts_with("Gesundheit")
                    || path.starts_with("Wohnung/Andrestr/Handwerker");
                belongs.then(|| Some((file.relative_path.clone(), file.text()?.to_owned(), file.date)))?
            })
            .collect();
        let mut written = 0;
        for index in self.rng.sample(pool.len(), pool.len()) {
            if written == FILING_COPIES {
                break;
            }
            let (path, text, date) = &pool[index];
            let name = file_name(path);
            let digits: String = name.chars().take(4).collect();
            let year = digits
                .parse::<i32>()
                .ok()
                .filter(|year| (2016..=2024).contains(year))
                .unwrap_or(2021);
            let relative = format!("Steuer/{year}/Belege/{name}");
            if self.is_free(&relative) {
                self.add(&relative, text.clone(), *date);
                written += 1;
            }
        }
        assert_eq!(written, FILING_COPIES, "not enough sources for the filing copies");

        let pool: Vec<(PathBuf, String, Option<NaiveDate>)> = self
            .files
            .iter()
            .filter_map(|file| Some((file.relative_path.clone(), file.text()?.to_owned(), file.date)))
            .collect();
        let mut written = 0;
        for index in self.rng.sample(pool.len(), pool.len()) {
            if written == DOWNLOAD_COPIES {
                break;
            }
            let (path, text, date) = &pool[index];
            let relative = format!("Downloads alt/{}", file_name(path));
            if self.is_free(&relative) {
                self.add(&relative, text.clone(), *date);
                written += 1;
            }
        }
        assert_eq!(written, DOWNLOAD_COPIES, "not enough sources for the download copies");
    }

    fn inbox(&mut self) -> Vec<TestFile> {
        const LETTERS: [(&str, &str, &str, &str); 8] = [
            ("Sparkasse Chemnitz", "Moritzhof 1, 09111 Chemnitz", "Kontoauszug", "IBAN DE12 8705 0000 0712 3456 78\nKundennummer: 4711002"),
            ("Allianz Versicherungs-AG", "Königinstraße 28, 80802 München", "Beitragsrechnung", "Versicherungsnummer 123456789"),
            ("Autohaus Süd GmbH", "Zwickauer Str. 100, 09116 Chemnitz", "Rechnung", "Kennzeichen C-AB 123"),
            ("Finanzamt Chemnitz-Süd", "Paul-Bertz-Str. 1, 09120 Chemnitz", "Bescheid für 2024 über Einkommensteuer", "Steuernummer 215/123/45678"),
            ("Cosmicdrift GmbH", "Str. der Nationen 1, 09111 Chemnitz", "Entgeltabrechnung", "Personalnummer 00042"),
            ("Interhyp AG", "Domagkstr. 34, 80807 München", "Darlehensvertrag", "Darlehensnummer 595610814\nObjekt Chemnitz-Andrestr. 12"),
            ("Praxis Dr. Schmidt", "Markt 3, 09111 Chemnitz", "Rechnung", "Patient Marc Beispiel"),
            ("Vodafone GmbH", "Ferdinand-Braun-Platz 1, 40549 Düsseldorf", "Ihre Rechnung", "Kundennummer 112233445\nRechnungsnummer 9988776655"),
        ];
        let photo_indices: HashSet<usize> = self.rng.sample(INBOX_FILES, INBOX_PHOTOS).into_iter().collect();
        let mut inbox = Vec::new();
        for index in 0..INBOX_FILES {
            let number = index + 1;
            if photo_indices.contains(&index) {
                let bytes = self.photo();
                inbox.push(TestFile {
                    relative_path: PathBuf::from(format!("IMG_{}.jpg", 5000 + number)),
                    content: TestContent::Photo(bytes),
                    date: None,
                });
                continue;
            }
            let year = if number % 3 == 0 { 2024 } else { 2025 };
            let month = self.rng.range_inclusive(1, 10);
            let date = self.random_date(year, Some(month));
            let (sender, address, heading, body) = *self.rng.choose(&LETTERS);
            let first_word = sender.split_whitespace().next().unwrap_or(sender);
            let names = [
                format!("scan_{}_{number:04}.pdf", date.format("%Y%m%d")),
                format!("Dokument ({number}).pdf"),
                format!("{first_word}_{number}.pdf"),
            ];
            let name = self.rng.choose(&names).clone();
            let text = self.letter_text(&Letter::new(sender, address, heading, body), date);
            inbox.push(TestFile {
                relative_path: PathBuf::from(name),
                content: TestContent::Text(text),
                date: Some(date),
            });
        }
        inbox
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The same archive on every call.
pub fn test_archive() -> TestArchive {
    let mut builder = Builder {
        rng: SplitMix64::new(SEED),
        files: Vec::new(),
        taken: HashSet::new(),
    };
    builder.bank();
    builder.tax();
    builder.insurance();
    builder.cars();
    builder.home();
    builder.work_and_health();
    builder.children();
    builder.unsorted();
    builder.copies();
    let inbox = builder.inbox();
    TestArchive {
        archive: builder.files,
        inbox,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn top_folder(file: &TestFile) -> String {
        file.relative_path
            .components()
            .next()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    #[test]
    fn archive_has_the_exact_counts() {
        let archive = test_archive();
        assert_eq!(archive.archive.len(), 1602);
        let mut per_folder: BTreeMap<String, usize> = BTreeMap::new();
        for file in &archive.archive {
            *per_folder.entry(top_folder(file)).or_default() += 1;
        }
        let expected = BTreeMap::from(
            [
                ("Bank", 354),
                ("Steuer", 301),
                ("Versicherung", 203),
                ("Auto", 129),
                ("Fotos Belege", 120),
                ("Arbeit", 96),
                ("Gesundheit", 80),
                ("Wohnung", 67),
                ("Kinder", 62),
                ("Scans", 150),
                ("Downloads alt", 40),
            ]
            .map(|(folder, count)| (folder.to_owned(), count)),
        );
        assert_eq!(per_folder, expected);
    }

    #[test]
    fn relative_paths_are_unique() {
        let archive = test_archive();
        for files in [&archive.archive, &archive.inbox] {
            let paths: HashSet<&PathBuf> = files.iter().map(|file| &file.relative_path).collect();
            assert_eq!(paths.len(), files.len());
        }
    }

    #[test]
    fn inbox_has_photos_and_texts() {
        let archive = test_archive();
        assert_eq!(archive.inbox.len(), 300);
        let photos = archive
            .inbox
            .iter()
            .filter(|file| file.relative_path.extension().is_some_and(|ext| ext == "jpg"))
            .count();
        assert_eq!(photos, 72);
        let texts = archive.inbox.iter().filter(|file| file.text().is_some()).count();
        assert_eq!(texts, 228);
    }

    #[test]
    fn copies_repeat_an_earlier_text() {
        let archive = test_archive();
        let mut seen = HashSet::new();
        let copies = archive
            .archive
            .iter()
            .filter_map(TestFile::text)
            .filter(|text| !seen.insert(*text))
            .count();
        assert_eq!(copies, 107);
    }

    #[test]
    fn the_archive_is_the_same_on_every_call() {
        assert_eq!(test_archive(), test_archive());
    }

    #[test]
    fn the_stated_date_stands_in_the_text() {
        let archive = test_archive();
        for file in archive.archive.iter().chain(&archive.inbox) {
            if let (Some(date), Some(text)) = (file.date, file.text()) {
                assert!(
                    text.contains(&german_date(date)),
                    "{}",
                    file.relative_path.display()
                );
            }
        }
    }

    #[test]
    fn write_to_creates_both_trees() {
        let archive = test_archive();
        let root = tempfile::tempdir().unwrap();
        archive.write_to(root.path()).unwrap();
        let first = &archive.archive[0];
        assert_eq!(
            std::fs::read(root.path().join("archive").join(&first.relative_path)).unwrap(),
            first.bytes()
        );
        assert_eq!(std::fs::read_dir(root.path().join("inbox")).unwrap().count(), 300);
    }
}
