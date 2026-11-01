use super::fixtures::{default_pattern_list, ids, item, list_in, loaded_list, replan_item};
use super::*;
use ncrs_index::KindLanguage;

#[test]
fn replan_the_step_files_nothing_by_hand_and_an_old_answer_is_dropped() {
    use ncrs_filing::replan_work::ReplanAfter;

    let mut list = loaded_list();
    let first = list.rows()[0].clone();
    let serial = list.begin_replan(ReplanScope::OpenCases).unwrap();
    assert!(list.begin_replan(ReplanScope::OpenCases).is_none());
    assert!(list.begin_confirm().is_none(), "Enter does not file a row");
    assert!(list.confirm_replan().is_none(), "nothing is previewed yet");

    list.previewed(serial.wrapping_add(1), Vec::new());
    assert!(matches!(list.replan(), Some(ReplanStep::Looking(_))));

    let path = PathBuf::from("Company A/invoice.pdf");
    list.previewed(serial, vec![replan_item(&first, ReplanAfter::Filed(path))]);
    assert_eq!(list.replan_fileable(), 1);
    let (scope, items) = list.confirm_replan().unwrap();
    assert_eq!(scope, ReplanScope::OpenCases);
    assert_eq!(items.len(), 1);
    assert!(list.replan().is_none());
}

#[test]
fn replan_a_preview_without_a_document_to_file_cannot_be_confirmed() {
    use ncrs_filing::replan_work::ReplanAfter;

    let mut list = loaded_list();
    let first = list.rows()[0].clone();
    let serial = list.begin_replan(ReplanScope::OpenCases).unwrap();
    list.previewed(serial, vec![replan_item(&first, ReplanAfter::Unread)]);

    assert_eq!(list.replan_fileable(), 0);
    assert!(list.confirm_replan().is_none());
    assert!(list.replan().is_some(), "the step stays until it is left");
    assert!(list.cancel_replan());
    assert!(!list.cancel_replan());
}

#[test]
fn without_a_rule_the_form_starts_with_the_default_pattern_and_files_nothing() {
    let mut list = default_pattern_list();

    assert_eq!(list.form().pattern, "{year}/{sender}/");
    assert_eq!(
        list.pattern_source(&list.rows()[0]),
        Some(PatternSource::Default)
    );
    assert_eq!(
        list.preview(),
        Some(PathBuf::from("2024/Frost/2024-03-09_Frost_3318"))
    );
    assert!(list.filed().is_none());

    list.select(1);
    assert_eq!(list.form().pattern, "", "no date, no pattern");
    assert_eq!(list.pattern_source(&list.rows()[1]), None);
}

#[test]
fn a_rule_pattern_is_kept_over_the_default() {
    let list = loaded_list();

    assert_eq!(list.form().pattern, "{sender}/{date}_{kind}");
    assert_eq!(
        list.pattern_source(&list.rows()[0]),
        Some(PatternSource::RuleOrProfile)
    );
}

#[test]
fn a_configured_default_replaces_the_built_in_ones() {
    let [one, _] = <[_; 2]>::try_from(ids()).unwrap();
    let mut item = item(one, "scan-1.pdf", "Frost");
    item.proposed_pattern = None;
    let mut list = ReviewList::new().with_default_pattern(Some("{year}/{kind}".to_owned()));
    list.loaded(vec![item]);

    assert_eq!(list.form().pattern, "{year}/{kind}");
}

#[test]
fn the_default_pattern_follows_the_fields_until_the_pattern_is_typed() {
    let mut list = default_pattern_list();
    list.select(1);

    list.set_text(FormField::Date, "2024-03-09".to_owned());
    assert_eq!(list.form().pattern, "{year}/{sender}/");

    list.set_pattern("{kind}".to_owned());
    list.set_text(FormField::Number, "1".to_owned());
    assert_eq!(list.form().pattern, "{kind}");
}

#[test]
fn a_field_counts_as_edited_once_the_form_differs_from_what_was_loaded() {
    let mut list = loaded_list();
    assert!(!list.is_edited(Field::Sender));
    list.start_edit();
    list.set_text(FormField::Sender, "Company AX".to_owned());
    assert!(list.is_edited(Field::Sender));
    assert!(!list.is_edited(Field::Kind));
    list.set_text(FormField::Sender, "Company A".to_owned());
    assert!(!list.is_edited(Field::Sender));
}

#[test]
fn the_preview_names_the_field_a_pattern_misses() {
    let mut list = loaded_list();
    list.set_text(FormField::Sender, String::new());

    assert_eq!(list.preview(), None);
    assert_eq!(list.missing_field(), Some(Field::Sender));
}

#[test]
fn a_wanted_entry_gets_the_cursor_and_the_form_once_the_list_has_loaded() {
    let rows = loaded_list().rows().to_vec();
    let mut reopened = ReviewList::new();
    reopened.edit_on_load(rows[1].path.clone(), FormField::Kind);
    reopened.loaded(rows);
    assert_eq!(reopened.cursor(), 1);
    assert!(reopened.is_editing());
    assert_eq!(reopened.take_focus_request(), Some(FormField::Kind));
    assert_eq!(reopened.take_focus_request(), None);
}

#[test]
fn the_list_opens_busy_and_empty() {
    let list = ReviewList::new();

    assert!(list.is_busy());
    assert!(list.rows().is_empty());
    assert_eq!(list.open_count(), None);
}

#[test]
fn loading_fills_the_form_from_the_first_entry() {
    let list = loaded_list();

    assert!(!list.is_busy());
    assert_eq!(list.open_count(), Some(2));
    assert_eq!(list.form().sender, "Company A");
    assert_eq!(list.form().date, "2024-03-09");
    assert_eq!(list.form().pattern, "{sender}/{date}_{kind}");
    assert_eq!(
        list.preview(),
        Some(PathBuf::from("Company A/2024-03-09_Invoice"))
    );
}

#[test]
fn moving_the_cursor_loads_the_form_of_the_other_entry() {
    let mut list = loaded_list();
    list.set_text(FormField::Sender, "Changed".to_owned());

    list.move_cursor(1);

    assert_eq!(list.cursor(), 1);
    assert_eq!(list.form().sender, "Agency B");
    list.move_cursor(-1);
    assert_eq!(list.form().sender, "Company A");
    list.move_cursor(-1);
    assert_eq!(list.cursor(), 0);
}

#[test]
fn a_value_no_field_can_hold_is_a_problem_and_nothing_is_filed() {
    let mut list = loaded_list();

    list.set_text(FormField::Date, "9.3.2024".to_owned());
    assert_eq!(list.begin_confirm(), None);
    assert_eq!(list.problem(), Some(&Problem::InvalidDate));

    list.set_text(FormField::Date, "2024-03-09".to_owned());
    list.set_text(FormField::Amount, "lots".to_owned());
    assert_eq!(list.begin_confirm(), None);
    assert_eq!(list.problem(), Some(&Problem::InvalidAmount));
    assert!(!list.is_busy());
}

#[test]
fn a_pattern_that_needs_an_empty_field_does_not_fit() {
    let mut list = loaded_list();
    list.set_text(FormField::Sender, " ".to_owned());

    assert_eq!(list.begin_confirm(), None);
    assert_eq!(list.problem(), Some(&Problem::PatternDoesNotFit));

    list.set_text(FormField::Sender, "Company A".to_owned());
    list.set_pattern("{nonsense}".to_owned());
    assert_eq!(list.begin_confirm(), None);
    assert_eq!(list.preview(), None);
}

#[test]
fn confirming_hands_over_the_edited_fields_and_marks_the_list_busy() {
    let mut list = loaded_list();
    list.set_text(FormField::Sender, " Company C ".to_owned());
    list.set_text(FormField::TagInput, "Tax".to_owned());
    list.add_tag();
    list.set_text(FormField::TagInput, "Travel".to_owned());
    list.add_tag();

    let request = list.begin_confirm().unwrap();

    assert_eq!(request.fields.sender.as_deref(), Some("Company C"));
    assert_eq!(request.fields.tags.len(), 2);
    assert_eq!(request.pattern, "{sender}/{date}_{kind}");
    assert!(list.is_busy());
    assert_eq!(list.begin_confirm(), None);
}

#[test]
fn a_tag_joins_the_own_tags_normalised_and_an_empty_or_known_one_does_not() {
    let mut list = loaded_list();

    list.set_text(FormField::TagInput, " Home Office ".to_owned());
    list.add_tag();
    list.set_text(FormField::TagInput, "home-office".to_owned());
    list.add_tag();
    list.set_text(FormField::TagInput, "  ".to_owned());
    list.add_tag();

    assert_eq!(list.form().tag_input, "");
    assert_eq!(
        list.form()
            .tags
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["home-office"]
    );
    list.remove_tag("home-office");
    assert!(list.form().tags.is_empty());
}

#[test]
fn the_kind_field_shows_the_word_of_the_language_and_saving_stores_the_id() {
    let german = list_in(KindLanguage::German);
    let english = list_in(KindLanguage::English);

    assert_eq!(german.form().kind, "Rechnung");
    assert_eq!(english.form().kind, "Invoice");
    assert!(!german.is_edited(Field::Kind));

    let mut edited = list_in(KindLanguage::German);
    edited.set_text(FormField::Kind, "Gutschrift".to_owned());
    assert_eq!(
        edited.typed_fields().unwrap().kind.as_deref(),
        Some("credit_note")
    );
    edited.set_text(FormField::Kind, "Steuerbescheid 2024".to_owned());
    assert_eq!(
        edited.typed_fields().unwrap().kind.as_deref(),
        Some("Steuerbescheid 2024")
    );
}

#[test]
fn the_path_preview_words_the_kind_in_the_language() {
    let german = list_in(KindLanguage::German);
    let english = list_in(KindLanguage::English);

    assert_eq!(
        german.preview(),
        Some(PathBuf::from("Company A").join("2024-03-09_Rechnung"))
    );
    assert_eq!(
        english.preview(),
        Some(PathBuf::from("Company A").join("2024-03-09_Invoice"))
    );
}

#[test]
fn the_automatic_tags_follow_the_typed_fields_and_the_recipient_is_filed() {
    let mut list = loaded_list();
    list.set_text(FormField::Sender, "Company C".to_owned());
    list.set_text(FormField::Recipient, " Erika Beispiel ".to_owned());
    list.set_text(FormField::Date, "not a date".to_owned());

    let automatic = list.automatic_tags();
    assert!(automatic.contains("company-c"));
    assert!(automatic.contains("erika-beispiel"));
    assert!(!list.form().tags.contains("company-c"));

    list.set_text(FormField::Date, "2024-03-09".to_owned());
    assert!(list.automatic_tags().contains("2024"));
    let request = list.begin_confirm().unwrap();
    assert_eq!(request.fields.recipient.as_deref(), Some("Erika Beispiel"));
    assert!(request.fields.tags.is_empty());
}

#[test]
fn a_filed_document_offers_a_rule_for_its_sender() {
    let mut list = loaded_list();
    for tag in ["tax", "travel"] {
        list.set_text(FormField::TagInput, tag.to_owned());
        list.add_tag();
    }
    list.begin_confirm().unwrap();
    list.confirmed(ConfirmOutcome::Filed {
        relative_path: PathBuf::from("Company A/2024-03-09_invoice.pdf"),
    });
    list.loaded(vec![list.rows()[1].clone()]);

    let filed = list.filed().unwrap();
    assert_eq!(
        filed.path,
        PathBuf::from("Company A/2024-03-09_invoice.pdf")
    );
    assert!(filed.offers_rule());
    let offer = list.rule_offer().unwrap();
    assert_eq!(offer.sender, "Company A");
    assert_eq!(offer.kind, "invoice");
    assert_eq!(
        offer.tags,
        BTreeSet::from(["tax".to_owned(), "travel".to_owned()])
    );
    assert_eq!(offer.pattern, "{sender}/{date}_{kind}");
}

#[test]
fn no_rule_is_offered_for_a_document_without_a_sender() {
    let mut list = loaded_list();
    list.set_text(FormField::Sender, String::new());
    list.set_pattern("{kind}/{date}".to_owned());

    list.begin_confirm().unwrap();
    list.confirmed(ConfirmOutcome::Filed {
        relative_path: PathBuf::from("invoice/2024-03-09.pdf"),
    });

    assert!(!list.filed().unwrap().offers_rule());
}

#[test]
fn a_taken_place_is_reported_and_the_entry_stays() {
    let mut list = loaded_list();
    list.begin_confirm().unwrap();

    list.confirmed(ConfirmOutcome::TargetExists);
    list.loaded(list.rows().to_vec());

    assert_eq!(list.rows().len(), 2);
    assert_eq!(list.filed(), None);
}

#[test]
fn a_failure_ends_busy_and_is_shown() {
    let mut list = loaded_list();
    list.begin_confirm().unwrap();

    list.failed("index not open".to_owned());

    assert!(!list.is_busy());
    assert_eq!(
        list.problem(),
        Some(&Problem::Failed("index not open".to_owned()))
    );
}

#[test]
fn an_empty_list_has_nothing_to_edit_or_file() {
    let mut list = ReviewList::new();
    list.loaded(Vec::new());

    list.start_edit();

    assert!(!list.is_editing());
    assert_eq!(list.begin_confirm(), None);
    assert_eq!(list.preview(), None);
    assert_eq!(list.open_count(), Some(0));
}
