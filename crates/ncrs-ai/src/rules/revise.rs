//! The question that changes levels by words: the levels as they are, and the
//! sentence of the user.

use super::levels::Levels;

/// The model copies what it is shown, so each kind of change has a worked
/// example before the real one: the user is done, a fixed name is renamed,
/// parts join in one level, a part moves, and two changes in one sentence.
/// The last examples say `"fertig":false`: the model repeats the last one.
const EXAMPLES: &str = r#"Example. Levels: {"ordner":[{"teile":[{"fest":"Post"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":null}
The user says: that is all, leave the rest as it is
Changed levels: {"ordner":[{"teile":[{"fest":"Post"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":null,"fertig":true}
Example. Levels: {"ordner":[{"teile":[{"fest":"Post"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":null}
The user says: call level 1 Briefe
Changed levels: {"ordner":[{"teile":[{"fest":"Briefe"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":null,"fertig":false}
Example. Levels: {"ordner":[{"teile":["year"],"trenner":""}],"datei":null}
The user says: year and month together in level 1, with a dash
Changed levels: {"ordner":[{"teile":["year","month"],"trenner":"-"}],"datei":null,"fertig":false}
Example. Levels: {"ordner":[{"teile":["year"],"trenner":""}],"datei":{"teile":[{"fest":"Rechnung"},"date"],"trenner":"_"}}
The user says: put the date at the front on 2
Changed levels: {"ordner":[{"teile":["year"],"trenner":""}],"datei":{"teile":["date",{"fest":"Rechnung"}],"trenner":"_"},"fertig":false}
Example. Levels: {"ordner":[{"teile":[{"fest":"Post"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":{"teile":[{"fest":"Rechnung"},"date"],"trenner":"_"}}
The user says: call level 1 Briefe and put the date first in level 3
Changed levels: {"ordner":[{"teile":[{"fest":"Briefe"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":{"teile":["date",{"fest":"Rechnung"}],"trenner":"_"},"fertig":false}
Example. Levels: {"ordner":[{"teile":[{"fest":"Post"}],"trenner":""},{"teile":["year"],"trenner":""}],"datei":{"teile":[{"fest":"Rechnung"},"date"],"trenner":"_"}}
The user says: year and month together in level 2, with a dash, and the date first in level 3
Changed levels: {"ordner":[{"teile":[{"fest":"Post"}],"trenner":""},{"teile":["year","month"],"trenner":"-"}],"datei":{"teile":["date",{"fest":"Rechnung"}],"trenner":"_"},"fertig":false}
"#;

/// Asked above the reminder; the answer is the changed levels and whether the
/// user is done, in the grammar of the revision.
pub fn revise_question(previous: &Levels, sentence: &str) -> String {
    format!(
        "Levels are numbered from 1: the folders in order, the file name last. The parts of a \
         level stand in the order of the name, from left to right. Placeholders: year month day \
         date sender recipient number kind. Change only what the user says; fertig is false unless the user only says they are \
         done. Keep every level the user does not mention.\n\
         {EXAMPLES}\
         Now the real one. Levels: {{{}}}\n\
         The user says: {sentence}\n\
         Changed levels:",
        previous.json_fields()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::rules::parse::parse_revision;

    #[test]
    fn the_question_holds_the_levels_and_the_sentence() {
        let levels = Levels::from_pattern("{year}/Rechnung_{date}").unwrap();
        let question = revise_question(&levels, "Datum nach vorn");
        assert!(question.contains(&levels.json_fields()));
        assert!(question.contains("Datum nach vorn"));
    }

    #[test]
    fn every_example_answer_is_a_revision_of_its_levels() {
        let answers: Vec<&str> = EXAMPLES
            .lines()
            .filter_map(|line| line.strip_prefix("Changed levels: "))
            .collect();
        assert_eq!(answers.len(), 6);
        for answer in answers {
            parse_revision(answer).unwrap();
        }
    }
}
