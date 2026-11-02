use super::*;

#[test]
fn a_tag_name_is_lowercase_words_with_hyphens() {
    assert_eq!(tag_name("Firma A GmbH"), "firma-a-gmbh");
    assert_eq!(tag_name("Deutsche Telekom AG"), "deutsche-telekom-ag");
    assert_eq!(tag_name("Müller & Söhne"), "müller-söhne");
    assert_eq!(tag_name("  2026 "), "2026");
    assert_eq!(tag_name("--"), "");
}

#[test]
fn a_tag_name_keeps_umlauts() {
    assert_eq!(tag_name("Büro"), "büro");
}

fn tag_matches(tag: &str, wanted: &str) -> bool {
    KindCatalog::built_in().tag_matches(tag, wanted)
}

#[test]
fn two_spellings_of_a_known_kind_match() {
    assert!(tag_matches("invoice", "rechnung"));
    assert!(tag_matches("rechnung", "Invoice"));
    assert!(tag_matches("credit-note", "Gutschrift"));
    assert!(!tag_matches("invoice", "mahnung"));
}

#[test]
fn a_known_kind_makes_its_id_as_tag() {
    let fields = DocumentFields {
        kind: Some("Gutschrift".to_owned()),
        ..DocumentFields::default()
    };
    assert_eq!(
        fields.automatic_tags(&KindCatalog::built_in()),
        BTreeSet::from(["credit-note".to_owned()])
    );
}

#[test]
fn a_tag_matches_itself_and_tags_below_it() {
    assert!(tag_matches("firma-a-gmbh", "firma-a"));
    assert!(!tag_matches("firma-ab", "firma-a"));
    assert!(tag_matches("firma-a", "Firma A"));
    assert!(!tag_matches("firma-a", ""));
}

#[test]
fn automatic_tags_come_from_kind_sender_recipient_and_year() {
    let fields = DocumentFields {
        sender: Some("Firma A GmbH".into()),
        recipient: Some("Erika Beispiel".into()),
        kind: Some("Rechnung".into()),
        date: NaiveDate::from_ymd_opt(2026, 3, 4),
        tags: BTreeSet::from(["Steuer".to_string()]),
        ..DocumentFields::default()
    };
    assert_eq!(
        fields.automatic_tags(&KindCatalog::built_in()),
        BTreeSet::from(["firma-a-gmbh", "erika-beispiel", "invoice", "2026"].map(String::from))
    );
    assert!(fields.all_tags(&KindCatalog::built_in()).contains("steuer"));
}
