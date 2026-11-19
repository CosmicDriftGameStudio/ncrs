use super::*;
use crate::messages::{QuickFixMessage, RuleFormMessage, RulesMessage};

fn rules_form_of(app: &App) -> &crate::rule_form::RuleForm {
    app.rules_for_test().unwrap().form().unwrap()
}

#[test]
fn create_rule_in_the_quick_fix_opens_the_rule_form_with_the_document_pattern() {
    let mut item = App::waiting_document_for_test().unwrap();
    item.fields.tags = std::collections::BTreeSet::from(["tax".to_owned()]);
    let mut app = App::with_quick_fix_menu(item);
    let _ = app.update(Message::QuickFix(QuickFixMessage::Hover(2)));

    let _ = app.update(Message::QuickFix(QuickFixMessage::Activate));

    let form = rules_form_of(&app);
    assert_eq!(form.target, crate::rule_form::RuleFormTarget::FromDocument);
    assert_eq!(form.conditions.rows[1].value, "company-a");
    assert_eq!(
        form.tags,
        std::collections::BTreeSet::from(["tax".to_owned()])
    );
    assert_eq!(form.filing.pattern(), "{sender}/{date}_{kind}");
    let _ = app.update(Message::Rules(RulesMessage::Form(RuleFormMessage::Cancel)));
    assert!(app.rules_for_test().is_some());
    assert!(app.rules_for_test().unwrap().form().is_none());
}

#[test]
fn set_fields_in_the_quick_fix_opens_the_chat() {
    let mut app = App::with_quick_fix_menu(App::waiting_document_for_test().unwrap());
    let _ = app.update(Message::QuickFix(QuickFixMessage::Hover(1)));

    let _ = app.update(Message::QuickFix(QuickFixMessage::Activate));

    assert!(app.hermes_chat_for_test().open);
}
