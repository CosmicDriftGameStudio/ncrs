//! The file name pattern a folder's names follow, asked of the chat model.

use super::intent::{ChatIntent, ChatModel, NameSpec};
use super::turn::{ChatQuestion, ChatTurn};
use crate::error::AiError;

/// The most names of a folder the model is shown.
pub const PROPOSAL_NAMES: usize = 30;

/// Asks `model` which name blocks the `names` of one folder follow. `None`
/// when it names none. The names go as the options of the question because a
/// message is cut after 200 characters.
pub fn propose_name_spec(
    model: &dyn ChatModel,
    names: &[String],
) -> Result<Option<NameSpec>, AiError> {
    let turn = ChatTurn {
        message: "Name new files like the files of this folder: change_structure with name"
            .to_owned(),
        question: Some(ChatQuestion {
            text: "Which blocks do the file names of this folder follow?".to_owned(),
            options: names.iter().take(PROPOSAL_NAMES).cloned().collect(),
        }),
        previous_search: None,
        previous_request: None,
        previous_message: None,
    };
    Ok(match model.understand(&turn)? {
        ChatIntent::ChangeStructure { name, .. } => name,
        _ => None,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::chat::NameBlock;
    use crate::status::LocalModelStatus;
    use crate::testing::ScriptedChatModel;

    fn names(count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("2026-01-0{index}_Bank.pdf"))
            .collect()
    }

    #[test]
    fn a_change_with_a_name_is_the_proposal() {
        let model = ScriptedChatModel::new(
            LocalModelStatus::NotDownloaded,
            [Ok(
                r#"{"intent":"change_structure","area":"*","name":["year","month","day","sender"],"separator":"_"}"#,
            )],
        );
        let spec = propose_name_spec(&model, &names(3)).unwrap().unwrap();
        assert_eq!(
            spec.blocks,
            vec![
                NameBlock::Year,
                NameBlock::Month,
                NameBlock::Day,
                NameBlock::Sender
            ]
        );
        assert_eq!(spec.separator, '_');
    }

    #[test]
    fn another_intent_is_no_proposal() {
        let model = ScriptedChatModel::new(
            LocalModelStatus::NotDownloaded,
            [Ok(r#"{"intent":"other"}"#)],
        );
        assert_eq!(propose_name_spec(&model, &names(2)).unwrap(), None);
    }

    #[test]
    fn at_most_thirty_names_are_shown() {
        let model = ScriptedChatModel::new(
            LocalModelStatus::NotDownloaded,
            [Ok(r#"{"intent":"other"}"#)],
        );
        propose_name_spec(&model, &names(45)).unwrap();
        let shown = model.turns();
        assert_eq!(shown[0].question.as_ref().unwrap().options.len(), 30);
    }
}
