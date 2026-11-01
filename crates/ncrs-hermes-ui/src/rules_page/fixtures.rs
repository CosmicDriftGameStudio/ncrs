use super::*;
use ncrs_index::{Index, Rule, RuleConditions, RuleId};
use std::collections::BTreeSet;

/// The `n`th id (from 1) an index hands out.
pub fn rule_id(n: usize) -> RuleId {
    let dir = tempfile::TempDir::new().unwrap();
    let mut index = Index::create_plain(&dir.path().join("index.db")).unwrap();
    let spec = spec(RuleConditions::all_of(vec![Condition::new(
        ConditionKind::Word,
        "rent",
    )]));
    let mut last = None;
    for _ in 0..n {
        last = Some(index.create_rule(&spec).unwrap());
    }
    last.unwrap()
}

pub fn spec(conditions: RuleConditions) -> RuleSpec {
    RuleSpec {
        name: None,
        conditions,
        extract_number: None,
        extract_date: None,
        extract_amount: None,
        kind: Some("Invoice".to_owned()),
        tags: BTreeSet::from(["tax".to_owned(), "office".to_owned()]),
        filing_pattern: "{year}/{sender}".to_owned(),
        confirmed: true,
        enabled: true,
    }
}

pub fn listed(id: i64, global_index: usize) -> ListedRule {
    ListedRule {
        rule: Rule {
            id: rule_id(usize::try_from(id).unwrap()),
            position: i64::try_from(global_index).unwrap() + 1,
            origin: RuleOrigin::Hand,
            spec: spec(RuleConditions::all_of(vec![Condition::new(
                ConditionKind::Word,
                "rent",
            )])),
        },
        global_index,
        hits: 0,
    }
}

/// Three listed rules with a suggestion between the first two (global
/// places 0, 2 and 3).
pub fn page() -> RulesPage {
    let mut page = RulesPage::new();
    page.loaded(RulesLoad {
        rules: vec![listed(1, 0), listed(2, 2), listed(3, 3)],
    });
    page
}
