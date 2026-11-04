#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::eval::{rows, search_of_json, Expect, HOLDOUT_ROWS, MAIN_ROWS};
use super::*;
use crate::error::AiError;

fn turn(message: &str) -> ChatTurn {
    ChatTurn {
        message: message.to_owned(),
        question: None,
        previous_search: None,
        previous_request: None,
        previous_message: None,
    }
}

fn question(text: &str, options: &[&str]) -> Option<ChatQuestion> {
    Some(ChatQuestion {
        text: text.to_owned(),
        options: options.iter().map(|option| (*option).to_owned()).collect(),
    })
}

fn parse(raw: &str) -> Result<ChatIntent, AiError> {
    parse_chat_reply(raw, &turn("x"))
}

#[test]
fn the_user_message_without_context() {
    assert_eq!(
        chat_user_message(&turn("räum meine downloads auf")).unwrap(),
        "Hermes' last question: none\nPrevious search: none\nPrevious request: none\nPrevious message: none\nMessage: räum meine downloads auf"
    );
}

#[test]
fn the_user_message_with_a_question_and_options() {
    let asked = ChatTurn {
        question: question("Soll ich das so umsetzen?", &["ja", "nein"]),
        ..turn("ja bitte")
    };

    assert_eq!(
        chat_user_message(&asked).unwrap(),
        "Hermes' last question: Soll ich das so umsetzen? Options: ja | nein\n\
         Previous search: none\nPrevious request: none\nPrevious message: none\nMessage: ja bitte"
    );
}

#[test]
fn the_user_message_carries_the_previous_search_as_compact_json() {
    let asked = ChatTurn {
        previous_search: Some(SearchQuery {
            sender: Some("Finanzamt Chemnitz-Süd".to_owned()),
            year_from: Some(2016),
            ..SearchQuery::default()
        }),
        ..turn("nur bescheide")
    };

    assert_eq!(
        chat_user_message(&asked).unwrap(),
        "Hermes' last question: none\n\
         Previous search: {\"sender\":\"Finanzamt Chemnitz-Süd\",\"year_from\":2016}\n\
         Previous request: none\nPrevious message: none\nMessage: nur bescheide"
    );
}

#[test]
fn amounts_are_written_whole_or_with_the_cents_they_need() {
    let query = SearchQuery {
        amount_min_cents: Some(10_000),
        amount_max_cents: Some(1_250),
        text: Some("a \"b\" \\ c".to_owned()),
        ..SearchQuery::default()
    };

    assert_eq!(
        search_json(&query),
        "{\"amount_min\":100,\"amount_max\":12.5,\"text\":\"a \\\"b\\\" \\\\ c\"}"
    );
    let cents = SearchQuery {
        amount_min_cents: Some(1_234),
        ..SearchQuery::default()
    };
    assert_eq!(search_json(&cents), "{\"amount_min\":12.34}");
}

#[test]
fn an_empty_message_is_refused_and_control_characters_are_removed() {
    chat_user_message(&turn("  \n\t ")).unwrap_err();
    assert!(chat_user_message(&turn("a\u{7}b"))
        .unwrap()
        .ends_with("Message: ab"));
}

#[test]
fn the_grammar_without_options_has_no_answer() {
    let grammar = chat_grammar(&turn("x"));

    assert!(grammar
        .starts_with("root ::= tidy | change | undo | search | ask | explain | bulk | smalltalk | leave | other\n"));
    assert!(!grammar.contains("answer"));
    assert!(grammar.contains("other ::="));
}

#[test]
fn the_grammar_with_options_has_an_answer_of_exactly_those() {
    let asked = ChatTurn {
        question: question("?", &["ja", "nein"]),
        ..turn("x")
    };

    let grammar = chat_grammar(&asked);

    assert!(grammar.starts_with(
        "root ::= tidy | change | undo | search | ask | explain | bulk | smalltalk | leave | other | answer\n\
         choice ::= \"\\\"ja\\\"\" | \"\\\"nein\\\"\"\n\
         answer ::= \"{\\\"intent\\\":\\\"answer\\\",\\\"choice\\\":\" choice \"}\"\n"
    ));
}

#[test]
fn options_the_grammar_cannot_spell_are_left_out() {
    let asked = ChatTurn {
        question: question("?", &["ja", "say \"x\"", "back\\slash", "tab\there", ""]),
        ..turn("x")
    };
    let only_bad = ChatTurn {
        question: question("?", &["say \"x\""]),
        ..turn("x")
    };

    assert!(chat_grammar(&asked).contains("choice ::= \"\\\"ja\\\"\"\nanswer"));
    assert!(!chat_grammar(&only_bad).contains("answer"));
}

#[test]
fn every_intent_is_read() {
    assert_eq!(
        parse(
            r#"{"intent":"tidy_folder","source":"Downloads","target":"Archiv","schedule":"always","transfer":"move"}"#
        ),
        Ok(ChatIntent::TidyFolder {
            source: "Downloads".to_owned(),
            target: Some("Archiv".to_owned()),
            schedule: Some(Schedule::Always),
            transfer: Some(Transfer::Move),
        })
    );
    assert_eq!(
        parse(r#"{"intent":"change_structure","area":"Konto","scope":"2020","order":"by_month"}"#),
        Ok(ChatIntent::ChangeStructure {
            area: "Konto".to_owned(),
            scope: Some("2020".to_owned()),
            order: Some(Order::ByMonth),
            name: None,
        })
    );
    assert_eq!(parse(r#"{"intent":"undo"}"#), Ok(ChatIntent::Undo));
    assert_eq!(
        parse_chat_reply(
            r#"{"intent":"search","sender":"DKB","year_from":2020,"amount_min":12.5}"#,
            &turn("dkb ab 2020 über 12,50 euro")
        ),
        Ok(ChatIntent::Search(SearchQuery {
            sender: Some("DKB".to_owned()),
            year_from: Some(2020),
            amount_min_cents: Some(1_250),
            ..SearchQuery::default()
        }))
    );
    assert_eq!(
        parse(r#"{"intent":"ask_content","question_kind":"sum","kind":"Rechnung"}"#),
        Ok(ChatIntent::AskContent {
            question_kind: QuestionKind::Sum,
            sender: None,
            kind: Some("Rechnung".to_owned()),
        })
    );
    assert_eq!(
        parse(r#"{"intent":"explain","file":"a.pdf"}"#),
        Ok(ChatIntent::Explain {
            file: "a.pdf".to_owned()
        })
    );
    assert_eq!(
        parse(r#"{"intent":"bulk_move","filter_year":2019,"target":"Alt"}"#),
        Ok(ChatIntent::BulkMove {
            filter_kind: None,
            filter_sender: None,
            filter_year: Some(2019),
            target: "Alt".to_owned(),
        })
    );
    assert_eq!(parse(r#"{"intent":"other","x":1}"#), Ok(ChatIntent::Other));
    assert_eq!(
        parse(r#"{"intent":"smalltalk"}"#),
        Ok(ChatIntent::SmallTalk)
    );
    assert_eq!(
        parse(r#"{"intent":"leave_as_is"}"#),
        Ok(ChatIntent::LeaveAsIs)
    );
}

#[test]
fn a_change_may_name_the_files_instead_of_the_order() {
    assert_eq!(
        parse(
            r#"{"intent":"change_structure","area":"Bank","name":["year","month","day"],"separator":"-"}"#
        ),
        Ok(ChatIntent::ChangeStructure {
            area: "Bank".to_owned(),
            scope: None,
            order: None,
            name: Some(NameSpec {
                blocks: vec![NameBlock::Year, NameBlock::Month, NameBlock::Day],
                separator: '-',
            }),
        })
    );
    let Ok(ChatIntent::ChangeStructure {
        name: Some(spec), ..
    }) = parse(
        r#"{"intent":"change_structure","area":"*","order":"by_year","name":["sender","bogus"],"separator":"x"}"#,
    )
    else {
        panic!("not a change with a name");
    };
    assert_eq!(spec.blocks, vec![NameBlock::Sender]);
    assert_eq!(spec.separator, '_');
}

#[test]
fn a_change_with_neither_order_nor_name_is_refused() {
    assert!(parse(r#"{"intent":"change_structure","area":"Bank"}"#).is_err());
    assert!(parse(r#"{"intent":"change_structure","area":"Bank","name":[]}"#).is_err());
}

#[test]
fn the_area_star_means_every_area() {
    assert_eq!(
        parse(r#"{"intent":"change_structure","area":"*","order":"by_year"}"#),
        Ok(ChatIntent::ChangeStructure {
            area: "*".to_owned(),
            scope: None,
            order: Some(Order::ByYear),
            name: None,
        })
    );
}

#[test]
fn the_grammar_allows_a_name_and_the_star_area() {
    let grammar = chat_grammar(&turn("x"));
    assert!(grammar.contains("nblock"));
    assert!(grammar.contains("separator"));
}

#[test]
fn a_request_written_as_json_reads_back_to_the_same_intent() {
    for raw in [
        r#"{"intent":"tidy_folder","source":"Downloads","target":"Archiv","schedule":"always","transfer":"copy"}"#,
        r#"{"intent":"tidy_folder","source":"Mails"}"#,
        r#"{"intent":"change_structure","area":"Bank","scope":"2020","order":"by_month"}"#,
        r#"{"intent":"change_structure","area":"*","order":"flat","name":["year","sender"],"separator":" "}"#,
        r#"{"intent":"change_structure","area":"Bank","name":["day"],"separator":"."}"#,
    ] {
        let intent = parse(raw).unwrap();
        let json = intent_json(&intent).unwrap();
        assert_eq!(json, raw);
        assert_eq!(parse(&json), Ok(intent));
    }
    assert_eq!(intent_json(&ChatIntent::Undo), None);
    assert_eq!(intent_json(&ChatIntent::SmallTalk), None);
}

#[test]
fn the_user_message_carries_the_previous_request_and_message() {
    let asked = ChatTurn {
        previous_request: Some(r#"{"intent":"tidy_folder","source":"A"}"#.to_owned()),
        previous_message: Some("mach mal".to_owned()),
        ..turn("nur bank")
    };
    assert_eq!(
        chat_user_message(&asked).unwrap(),
        "Hermes' last question: none\nPrevious search: none\n\
         Previous request: {\"intent\":\"tidy_folder\",\"source\":\"A\"}\n\
         Previous message: mach mal\nMessage: nur bank"
    );
    assert!(chat_user_message(&turn("x"))
        .unwrap()
        .contains("Previous request: none\nPrevious message: none\n"));
}

#[test]
fn the_previous_request_of_every_row_is_a_request_the_parser_reads() {
    let mut checked = 0;
    for row in rows(MAIN_ROWS).iter().chain(&rows(HOLDOUT_ROWS)) {
        if let Some(json) = &row.previous_request {
            let intent = parse(json).unwrap();
            assert_eq!(&intent_json(&intent).unwrap(), json, "row {}", row.id);
            checked += 1;
        }
    }
    assert!(checked >= 8);
}

#[test]
fn an_answer_is_the_index_of_the_option_it_names() {
    let asked = ChatTurn {
        question: question("?", &["ja", "nein", "später"]),
        ..turn("x")
    };

    assert_eq!(
        parse_chat_reply(r#"{"intent":"answer","choice":"später"}"#, &asked),
        Ok(ChatIntent::Answer { choice: 2 })
    );
    parse_chat_reply(r#"{"intent":"answer","choice":"vielleicht"}"#, &asked).unwrap_err();
    parse_chat_reply(r#"{"intent":"answer","choice":"JA"}"#, &asked).unwrap_err();
    parse(r#"{"intent":"answer","choice":"ja"}"#).unwrap_err();
}

#[test]
fn amounts_of_zero_or_less_and_bad_years_are_no_values() {
    let Ok(ChatIntent::Search(query)) = parse(
        r#"{"intent":"search","year_from":999,"year_to":3000,"amount_min":0,"amount_max":-5}"#,
    ) else {
        panic!("a search");
    };

    assert_eq!(query, SearchQuery::default());
    let Ok(ChatIntent::Search(rounded)) = parse_chat_reply(
        r#"{"intent":"search","amount_min":0.005}"#,
        &turn("ab 0,01 euro"),
    ) else {
        panic!("a search");
    };
    assert_eq!(rounded.amount_min_cents, Some(1));
}

#[test]
fn an_amount_the_typed_sentence_does_not_contain_is_dropped() {
    let raw = r#"{"intent":"search","amount_min":500,"amount_max":1000}"#;
    let amounts = |typed: &str| {
        let Ok(ChatIntent::Search(query)) = parse_chat_reply(raw, &turn(typed)) else {
            panic!("a search");
        };
        (query.amount_min_cents, query.amount_max_cents)
    };

    assert_eq!(amounts("alle rechnungen"), (None, None));
    assert_eq!(amounts("rechnungen über 500 euro"), (Some(50_000), None));
    assert_eq!(
        amounts("zwischen 500 und 1.000 euro"),
        (Some(50_000), Some(100_000))
    );
}

#[test]
fn strings_are_cleaned_and_a_long_one_is_no_value() {
    let long = "x".repeat(81);
    let raw = format!(r#"{{"intent":"search","sender":"A\u0007B","kind":"{long}"}}"#);

    let Ok(ChatIntent::Search(query)) = parse(&raw) else {
        panic!("a search");
    };

    assert_eq!(query.sender.as_deref(), Some("AB"));
    assert_eq!(query.kind, None);
}

#[test]
fn a_missing_required_field_or_an_unknown_intent_is_an_error() {
    for raw in [
        r#"{"intent":"tidy_folder"}"#,
        r#"{"intent":"change_structure","area":"A"}"#,
        r#"{"intent":"change_structure","order":"flat"}"#,
        r#"{"intent":"change_structure","area":"A","order":"sideways"}"#,
        r#"{"intent":"explain"}"#,
        r#"{"intent":"bulk_move"}"#,
        r#"{"intent":"ask_content"}"#,
        r#"{"intent":"delete_everything"}"#,
        r#"{"sender":"A"}"#,
        "[1]",
        "nonsense",
    ] {
        assert!(parse(raw).is_err(), "{raw}");
    }
}

#[test]
fn the_previous_search_of_every_row_is_written_back_unchanged() {
    let mut checked = 0;
    for row in rows(MAIN_ROWS).iter().chain(&rows(HOLDOUT_ROWS)) {
        if let Some(json) = &row.previous_search {
            assert_eq!(&search_json(&search_of_json(json)), json, "row {}", row.id);
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn the_rows_are_all_there_and_make_turns() {
    let all: Vec<_> = rows(MAIN_ROWS)
        .into_iter()
        .chain(rows(HOLDOUT_ROWS))
        .collect();

    assert_eq!(all.len(), 191);
    for row in &all {
        let turn = row.turn();
        assert_eq!(turn.message, row.text);
        assert_eq!(
            turn.question.map(|q| q.options),
            row.question.as_ref().map(|_| row.options.clone())
        );
        assert!(!row.intent.is_empty(), "row {}", row.id);
        for (key, expect) in &row.expected {
            let usable = match expect {
                Expect::Text(any_of) => !any_of.is_empty(),
                Expect::Number(number) => number.is_finite(),
            };
            assert!(usable, "row {} {key}", row.id);
        }
    }
}

#[cfg(feature = "testing")]
#[test]
fn a_scripted_model_replies_through_the_real_parser_and_runs_dry() {
    use crate::status::LocalModelStatus;
    use crate::testing::ScriptedChatModel;

    let model = ScriptedChatModel::new(
        LocalModelStatus::NotLoadedYet,
        [Ok(r#"{"intent":"undo"}"#), Err(AiError::Model("down"))],
    );

    assert_eq!(model.understand(&turn("a")), Ok(ChatIntent::Undo));
    assert_eq!(model.understand(&turn("b")), Err(AiError::Model("down")));
    assert_eq!(
        model.understand(&turn("c")),
        Err(AiError::Model("no scripted reply"))
    );
    assert_eq!(model.turns().len(), 3);
    model.refresh();
    assert_eq!(model.refresh_count(), 1);
}
