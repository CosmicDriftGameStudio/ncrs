use super::*;
use ncrs_filing::review_work::TextState;
use ncrs_index::KindLanguage;
use ncrs_index::{Index, ReviewReason};

/// Ids come from a real index: only the index hands them out.
pub fn ids() -> Vec<(ReviewEntryId, DocumentId)> {
    ids_of(2)
}

pub fn ids_of(count: u8) -> Vec<(ReviewEntryId, DocumentId)> {
    use chrono::{TimeZone as _, Utc};
    use ncrs_index::{ContentHash, DocumentSource, NewDocument};

    let dir = tempfile::TempDir::new().unwrap();
    let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    (1..=count)
        .map(|seed| {
            let document = index
                .insert_document(&NewDocument {
                    hash: ContentHash([seed; 32]),
                    path: dir.path().join(format!("scan-{seed}.pdf")),
                    source: DocumentSource::Inbox,
                    received_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    stamp: None,
                })
                .unwrap();
            let entry = index
                .add_review(document, ReviewReason::NoMatchingRule)
                .unwrap();
            (entry, document)
        })
        .collect()
}

pub fn item(
    (entry, document_id): (ReviewEntryId, DocumentId),
    name: &str,
    sender: &str,
) -> ReviewItem {
    ReviewItem {
        entry,
        document_id,
        path: PathBuf::from(format!("inbox/{name}")),
        reason: ReviewReason::UnconfirmedRule,
        text_state: TextState::Read,
        fields: DocumentFields {
            sender: (!sender.is_empty()).then(|| sender.to_owned()),
            kind: Some("invoice".to_owned()),
            date: chrono::NaiveDate::from_ymd_opt(2024, 3, 9),
            ..DocumentFields::default()
        },
        proposed_pattern: Some("{sender}/{date}_{kind}".to_owned()),
        origins: ncrs_index::FieldOrigins::default(),
        passages: Vec::new(),
    }
}

pub fn loaded_list() -> ReviewList {
    let [one, two] = <[_; 2]>::try_from(ids()).unwrap();
    let mut list = ReviewList::new();
    list.loaded(vec![
        item(one, "scan-1.pdf", "Company A"),
        item(two, "scan-2.pdf", "Agency B"),
    ]);
    list
}

/// One document whose recipient is read, with two tags of the user's own,
/// that no rule covers: the default target is proposed.
pub fn tags_list() -> ReviewList {
    let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
    let mut tagged = item(one, "scan-1.pdf", "Telekom");
    tagged.proposed_pattern = None;
    tagged.reason = ReviewReason::NoMatchingRule;
    tagged.fields.recipient = Some("Firma A GmbH".to_owned());
    tagged.fields.tags = ["steuer", "büro"].map(str::to_owned).into();
    let mut list = ReviewList::new();
    list.loaded(vec![tagged]);
    list
}

/// Two documents no rule covers: one the default pattern fills in, one
/// without a date that it cannot.
pub fn default_pattern_list() -> ReviewList {
    let [one, two] = <[_; 2]>::try_from(ids()).unwrap();
    let mut frost = item(one, "scan-1.pdf", "Frost");
    frost.proposed_pattern = None;
    frost.reason = ReviewReason::NoMatchingRule;
    frost.fields.number = Some("3318".to_owned());
    let mut undated = item(two, "scan-2.pdf", "Agency B");
    undated.proposed_pattern = None;
    undated.reason = ReviewReason::NoMatchingRule;
    undated.fields.date = None;
    let mut list = ReviewList::new();
    list.loaded(vec![frost, undated]);
    list
}

/// One document whose text gave nothing.
pub fn nothing_recognised_list() -> ReviewList {
    let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
    let mut blank = item(one, "scan-1.pdf", "");
    blank.proposed_pattern = None;
    blank.reason = ReviewReason::NoMatchingRule;
    blank.fields = DocumentFields::default();
    let mut list = ReviewList::new();
    list.loaded(vec![blank]);
    list
}

/// One document whose fields come from a rule, the AI and the user, with
/// the passages of its text that led there.
pub fn origins_list() -> ReviewList {
    use ncrs_index::{FieldOrigin, FieldOrigins};

    let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
    let mut origin_item = item(one, "scan-1.pdf", "Company A");
    origin_item.fields.number = Some("4711".to_owned());
    origin_item.origins = FieldOrigins::default()
        .with(Field::Sender, FieldOrigin::Rule(None))
        .with(
            Field::Kind,
            FieldOrigin::Ai {
                confidence_percent: 82,
            },
        )
        .with(Field::Number, FieldOrigin::Manual)
        .with(Field::Date, FieldOrigin::Rule(None));
    origin_item.passages = vec!["Company A".to_owned(), "4711".to_owned()];
    let mut list = ReviewList::new();
    list.loaded(vec![origin_item]);
    list
}

/// More entries than the list shows, the first with names too long for
/// the file column and the second with a short one.
pub fn long_list() -> ReviewList {
    const LONG: &str =
        "2024-03-09_Stadtwerke_Musterhausen_Jahresabrechnung_Strom_und_Gas_Kundennummer_4711.pdf";
    let mut list = ReviewList::new();
    list.loaded(
        ids_of(200)
            .into_iter()
            .enumerate()
            .map(|(position, entry)| match position {
                0 => item(
                    entry,
                    LONG,
                    "Stadtwerke Musterhausen Energie und Wasser GmbH",
                ),
                1 => item(entry, "a.pdf", "A"),
                _ => item(entry, &format!("scan-{position}.pdf"), "Agency B"),
            })
            .collect(),
    );
    list
}

pub fn replan_item(row: &ReviewItem, after: ncrs_filing::replan_work::ReplanAfter) -> ReplanItem {
    ReplanItem {
        document_id: row.document_id,
        review: Some(row.entry),
        path: row.path.clone(),
        now: Some(row.reason),
        after,
    }
}

pub fn list_in(kind_language: KindLanguage) -> ReviewList {
    let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
    let mut list = ReviewList::new().with_kinds(Kinds::built_in(kind_language));
    list.loaded(vec![item(one, "scan-1.pdf", "Company A")]);
    list
}
