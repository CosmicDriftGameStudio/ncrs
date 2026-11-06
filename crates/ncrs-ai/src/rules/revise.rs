//! The question that changes levels by words: the levels as they are, and the
//! sentence of the user.

use super::levels::Levels;

/// The model copies what it is shown, so the question holds a worked example; the answer is the changed levels, in the grammar of
/// the recognition.
pub fn revise_question(previous: &Levels, sentence: &str) -> String {
    format!(
        "Levels are numbered from 1: the folders in order, the file name last. The parts of a \
         level stand in the order of the name, from left to right.\n\
         Example. Levels: {{\"ordner\":[\"year\"],\"datei\":{{\"teile\":[{{\"fest\":\"Rechnung\"}},\"date\"],\"trenner\":\"_\"}}}}\n\
         The user says: put the date at the front on 2\n\
         Changed levels: {{\"ordner\":[\"year\"],\"datei\":{{\"teile\":[\"date\",{{\"fest\":\"Rechnung\"}}],\"trenner\":\"_\"}}}}\n\
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

    #[test]
    fn the_question_holds_the_levels_and_the_sentence() {
        let levels = Levels::from_pattern("{year}/Rechnung_{date}").unwrap();
        let question = revise_question(&levels, "Datum nach vorn");
        assert!(question.contains(&levels.json_fields()));
        assert!(question.contains("Datum nach vorn"));
    }
}
