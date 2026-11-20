use super::*;

fn asked_with(labels: &[&str]) -> Asked {
    Asked {
        question: Some("Which one?".to_owned()),
        chips: labels
            .iter()
            .map(|label| Chip {
                label: (*label).to_owned(),
                reply: ChipReply::Later,
                primary: false,
            })
            .collect(),
    }
}

#[test]
fn a_typed_label_is_the_button_regardless_of_case() {
    let asked = asked_with(&["Yes", "No"]);
    assert_eq!(chip_reply_for(&asked, " yes "), Some(ChipReply::Later));
}

#[test]
fn other_text_and_twin_labels_go_to_the_model() {
    assert_eq!(chip_reply_for(&asked_with(&["Yes"]), "yes please"), None);
    assert_eq!(chip_reply_for(&asked_with(&["Same", "same"]), "same"), None);
}
