#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use crate::{intent_of, normalize_word, reply_of, same_word, FolderRef, Intent, Reply, TimeUnit};

fn search(words: &str) -> Intent {
    Intent::Search {
        words: words.to_owned(),
    }
}

#[test]
fn the_seven_example_sentences_in_german() {
    assert_eq!(
        intent_of("räum meine Downloads auf"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Downloads)
        }
    );
    assert_eq!(
        intent_of("so soll mein Archiv aussehen"),
        Intent::LearnStructure { folder: None }
    );
    assert_eq!(
        intent_of("lies mal /Volumes/Archiv"),
        Intent::LearnStructure {
            folder: Some(FolderRef::Path(PathBuf::from("/Volumes/Archiv")))
        }
    );
    assert_eq!(
        intent_of("such die Rechnung von Kalle Klamrot"),
        search("die Rechnung von Kalle Klamrot")
    );
    assert_eq!(
        intent_of("warum liegt scan0042.pdf bei der HUK?"),
        Intent::Explain {
            words: "liegt scan0042.pdf bei der HUK".to_owned()
        }
    );
    assert_eq!(intent_of("mach das rückgängig"), Intent::Undo);
    assert_eq!(
        intent_of("verschieb alle Steuersachen von 2021 nach Steuer/2021"),
        Intent::BatchMove {
            words: "alle Steuersachen von 2021 nach Steuer/2021".to_owned()
        }
    );
    assert_eq!(
        intent_of("Kontoauszüge lieber nach Monat"),
        Intent::ChangeOrder {
            kind_word: "Kontoauszüge".to_owned(),
            unit: TimeUnit::Month
        }
    );
}

#[test]
fn the_seven_example_sentences_in_english() {
    assert_eq!(
        intent_of("tidy up my downloads"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Downloads)
        }
    );
    assert_eq!(
        intent_of("learn my structure"),
        Intent::LearnStructure { folder: None }
    );
    assert_eq!(
        intent_of("read /Volumes/Archive"),
        Intent::LearnStructure {
            folder: Some(FolderRef::Path(PathBuf::from("/Volumes/Archive")))
        }
    );
    assert_eq!(
        intent_of("find the invoice from Kalle Klamrot"),
        search("the invoice from Kalle Klamrot")
    );
    assert_eq!(
        intent_of("why is scan0042.pdf in HUK?"),
        Intent::Explain {
            words: "is scan0042.pdf in HUK".to_owned()
        }
    );
    assert_eq!(intent_of("undo that"), Intent::Undo);
    assert_eq!(
        intent_of("move all tax things from 2021 to Tax/2021"),
        Intent::BatchMove {
            words: "all tax things from 2021 to Tax/2021".to_owned()
        }
    );
    assert_eq!(
        intent_of("bank statements by month"),
        Intent::ChangeOrder {
            kind_word: "bank statements".to_owned(),
            unit: TimeUnit::Month
        }
    );
}

#[test]
fn more_ways_to_say_the_same() {
    for text in [
        "rückgängig",
        "Zurück",
        "undo",
        "put it back",
        "Mach das bitte rückgängig!",
    ] {
        assert_eq!(intent_of(text), Intent::Undo, "{text}");
    }
    assert_eq!(
        intent_of("wo ist die HUK Police?"),
        search("die HUK Police")
    );
    assert_eq!(intent_of("where is my tax return"), search("my tax return"));
    assert_eq!(intent_of("find Rechnung"), search("Rechnung"));
    assert_eq!(
        intent_of("Bitte sortier meinen Schreibtisch"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Desktop)
        }
    );
    assert_eq!(
        intent_of("clean up ~/Documents/Inbox"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Path(PathBuf::from("~/Documents/Inbox")))
        }
    );
    assert_eq!(
        intent_of("Dokumente aufräumen"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Documents)
        }
    );
    assert_eq!(
        intent_of("Kontoauszüge nach Jahr"),
        Intent::ChangeOrder {
            kind_word: "Kontoauszüge".to_owned(),
            unit: TimeUnit::Year
        }
    );
    assert_eq!(
        intent_of("kopier alle Fotos nach Bilder"),
        Intent::BatchMove {
            words: "alle Fotos nach Bilder".to_owned()
        }
    );
}

#[test]
fn a_capitalised_word_names_a_folder_only_without_a_path_or_home_folder() {
    assert_eq!(
        intent_of("räum den Keller auf"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Named("Keller".to_owned()))
        }
    );
    assert_eq!(
        intent_of("räum den Ordner auf"),
        Intent::TidyFolder { folder: None }
    );
    assert_eq!(
        intent_of("räum Keller /tmp/x auf"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Path(PathBuf::from("/tmp/x")))
        }
    );
    assert_eq!(
        intent_of("räum Keller Downloads auf"),
        Intent::TidyFolder {
            folder: Some(FolderRef::Downloads)
        }
    );
}

#[test]
fn shell_commands_are_recognised_and_plain_words_are_not() {
    for text in [
        "ls",
        "rm -rf x",
        "cd /tmp",
        "open ~/Downloads",
        "cat file.txt | grep a",
        "echo hi > out.txt",
        "ls; pwd",
        "make && make install",
        "echo $(date)",
        "echo `date`",
    ] {
        assert_eq!(intent_of(text), Intent::ShellCommand, "{text}");
    }
    assert_eq!(intent_of("find Rechnung"), search("Rechnung"));
    assert_eq!(intent_of("open the door"), Intent::Unknown);
}

#[test]
fn everything_else_is_unknown() {
    for text in [
        "",
        "   ",
        "Hallo",
        "wie spät ist es",
        "nach Monat",
        "what is the weather",
    ] {
        assert_eq!(intent_of(text), Intent::Unknown, "{text:?}");
    }
}

#[test]
fn replies_are_yes_no_or_cancel() {
    for text in [
        "ja",
        "Jo",
        "genau",
        "Passt",
        "ok",
        "Okay!",
        "yes",
        "yep",
        "sure",
        "ja, bitte",
    ] {
        assert_eq!(reply_of(text), Some(Reply::Yes), "{text}");
    }
    for text in ["nein", "Nee", "no", "nope", "nein danke"] {
        assert_eq!(reply_of(text), Some(Reply::No), "{text}");
    }
    for text in ["abbrechen", "Stopp", "cancel", "stop!"] {
        assert_eq!(reply_of(text), Some(Reply::Cancel), "{text}");
    }
    for text in [
        "",
        "vielleicht",
        "ja aber nicht alle",
        "räum meine Downloads auf",
    ] {
        assert_eq!(reply_of(text), None, "{text:?}");
    }
}

#[test]
fn words_are_folded_and_plurals_match() {
    assert_eq!(normalize_word("„Größe?“"), "grosse");
    assert_eq!(normalize_word("Kontoauszüge"), "kontoauszuge");
    assert!(same_word("Kontoauszüge", "Kontoauszug"));
    assert!(same_word("Kontoauszug", "Kontoauszüge"));
    assert!(same_word("Rechnungen", "Rechnung"));
    assert!(same_word("invoices", "invoice"));
    assert!(same_word("Rechnung", "rechnung"));
    assert!(!same_word("Rechnung", "Rechner"));
    assert!(!same_word("Vertrag", "Vertrags-Kunde"));
    assert!(!same_word("", ""));
    assert!(!same_word("es", "e"));
}
