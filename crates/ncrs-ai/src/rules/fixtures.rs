//! The eight documents every case of the matrix is judged on: their fields,
//! their text and the folder they lie in.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use ncrs_index::{DocumentFields, KindLanguage, Kinds};
use ncrs_intake::parse_date;

use super::matrix::COMMON_WORD;

struct Row {
    sender: &'static str,
    domain: &'static str,
    recipient: &'static str,
    address: &'static str,
    kind: &'static str,
    date: &'static str,
    number: &'static str,
    folder: &'static str,
    tag: &'static str,
    keyword: &'static str,
    iban: &'static str,
    tax_id: &'static str,
}

/// Days above 12 and different from the month, years and numbers all
/// different: no two placeholders render the same on these.
const ROWS: [Row; 8] = [
    Row {
        sender: "Telekom Deutschland GmbH",
        domain: "telekom.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "invoice",
        date: "2023-03-14",
        number: "R-2023-0314",
        folder: "Posteingang/Telefon",
        tag: "steuer",
        keyword: "Mobilfunk",
        iban: "DE89370400440532013000",
        tax_id: "12/345/67890",
    },
    Row {
        sender: "Allianz Versicherung",
        domain: "allianz.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "contract",
        date: "2022-11-28",
        number: "V-88231",
        folder: "Versicherungen",
        tag: "privat",
        keyword: "Haftpflicht",
        iban: "DE02120300000000202051",
        tax_id: "21/987/65432",
    },
    Row {
        sender: "Stadtwerke Müller",
        domain: "stadtwerke-mueller.de",
        recipient: "Jürgen Köhler",
        address: "Gartenweg 5\n04109 Leipzig",
        kind: "reminder",
        date: "2024-01-31",
        number: "M-4412",
        folder: "Posteingang/Energie",
        tag: "energie",
        keyword: "Stromlieferung",
        iban: "DE75512108001245126199",
        tax_id: "33/222/11110",
    },
    Row {
        sender: "Finanzamt Chemnitz",
        domain: "finanzamt-chemnitz.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "notice",
        date: "2021-06-25",
        number: "St-2021-77",
        folder: "Steuer",
        tag: "steuer",
        keyword: "Einkommensteuer",
        iban: "DE12500105170648489890",
        tax_id: "44/555/66670",
    },
    Row {
        sender: "Sparkasse Leipzig",
        domain: "sparkasse-leipzig.de",
        recipient: "Anna Weiß",
        address: "Lindenallee 7\n80331 München",
        kind: "bank_statement",
        date: "2020-09-19",
        number: "KA-2020-09",
        folder: "Bank",
        tag: "privat",
        keyword: "Kontostand",
        iban: "DE68210501700012345678",
        tax_id: "55/666/77780",
    },
    Row {
        sender: "Zalando SE",
        domain: "zalando.de",
        recipient: "Jürgen Köhler",
        address: "Gartenweg 5\n04109 Leipzig",
        kind: "receipt",
        date: "2019-05-17",
        number: "Z-558120",
        folder: "Einkäufe",
        tag: "einkauf",
        keyword: "Bestellung",
        iban: "DE27100777770209299700",
        tax_id: "66/777/88890",
    },
    Row {
        sender: "Praxis Dr. Schäfer",
        domain: "praxis-schaefer.de",
        recipient: "Anna Weiß",
        address: "Lindenallee 7\n80331 München",
        kind: "invoice",
        date: "2025-08-22",
        number: "A-2025-0822",
        folder: "Posteingang/Arzt",
        tag: "steuer",
        keyword: "Behandlung",
        iban: "DE91100000000123456789",
        tax_id: "77/888/99900",
    },
    Row {
        sender: "Stromio GmbH",
        domain: "stromio.de",
        recipient: "Marc Beispiel",
        address: "Andrestr. 12\n09112 Chemnitz",
        kind: "termination",
        date: "2017-12-29",
        number: "K-91827",
        folder: "Posteingang/Telefon",
        tag: "wichtig",
        keyword: "Vertragsende",
        iban: "DE44500105175407324931",
        tax_id: "88/999/00010",
    },
];

#[derive(Debug, Clone)]
pub struct Fixture {
    pub fields: DocumentFields,
    pub text: String,
    pub path: PathBuf,
    pub folder: &'static str,
    pub domain: &'static str,
    pub iban: &'static str,
    pub tax_id: &'static str,
    pub keyword: &'static str,
    pub tag: &'static str,
}

impl Fixture {
    fn from_row(index: usize, row: &Row) -> Self {
        let mut day_month_year = row.date.split('-').rev();
        let dotted = day_month_year.by_ref().collect::<Vec<_>>().join(".");
        let kinds = Kinds::built_in(KindLanguage::German);
        let text = format!(
            "{sender}\nMusterstr. {index}, 10115 Berlin\nservice@{domain} · Tel. 0800 1234\n\n\
             {recipient}\n{address}\n\n{kind} Nr. {number}\n{COMMON_WORD}: 4711-{index}\n\
             Datum: {dotted}\nBetreff: {keyword}\nZahlbar auf IBAN {iban}\nSteuernummer {tax_id}\n",
            sender = row.sender,
            domain = row.domain,
            recipient = row.recipient,
            address = row.address,
            kind = kinds.kind_word(row.kind),
            number = row.number,
            keyword = row.keyword,
            iban = spaced(row.iban),
            tax_id = row.tax_id,
        );
        Self {
            fields: DocumentFields {
                sender: Some(row.sender.to_owned()),
                recipient: Some(row.recipient.to_owned()),
                kind: Some(row.kind.to_owned()),
                date: parse_date(row.date),
                number: Some(row.number.to_owned()),
                amount: None,
                tags: BTreeSet::from([row.tag.to_owned()]),
            },
            text,
            path: PathBuf::from(format!("{}/scan-{index}.pdf", row.folder)),
            folder: row.folder,
            domain: row.domain,
            iban: row.iban,
            tax_id: row.tax_id,
            keyword: row.keyword,
            tag: row.tag,
        }
    }
}

pub fn spaced(iban: &str) -> String {
    iban.as_bytes()
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn fixtures() -> Vec<Fixture> {
    ROWS.iter()
        .enumerate()
        .map(|(index, row)| Fixture::from_row(index + 1, row))
        .collect()
}

/// The language the archive of the fixtures is shown in.
pub fn archive_kinds() -> Kinds {
    Kinds::built_in(KindLanguage::German)
}
