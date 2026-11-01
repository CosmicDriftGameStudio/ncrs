//! The message each context menu entry sends: exactly the one its key sends.

use ncrs_files::context_menu::{ContextAction, HermesAction};

use crate::fs::OpenKind;
use crate::hermes_column::FolderRole;
use crate::messages::PanelMessage;
use crate::messages::{
    ClipboardKind, HermesFileMessage, HitListMessage, Message, QuickFixMessage, TransferKind,
};

pub trait ContextActionMessage {
    fn message(self) -> Message;
}

impl ContextActionMessage for ContextAction {
    fn message(self) -> Message {
        match self {
            ContextAction::Open => Message::Panel(PanelMessage::OpenSelected),
            ContextAction::View => Message::OpenExternal(OpenKind::View),
            ContextAction::Edit => Message::OpenExternal(OpenKind::Edit),
            ContextAction::Copy => Message::Transfer(TransferKind::Copy),
            ContextAction::Move => Message::Transfer(TransferKind::Move),
            ContextAction::NewFolder => Message::CreateDirPrompt,
            ContextAction::CopyPath => Message::CopyToClipboard(ClipboardKind::Path),
            ContextAction::CopyName => Message::CopyToClipboard(ClipboardKind::Name),
            ContextAction::ComputeFolderSizes => Message::ComputeFolderSizes,
            ContextAction::Trash => Message::Delete { permanent: false },
            ContextAction::DeletePermanently => Message::Delete { permanent: true },
            ContextAction::ShowOnOtherSide => Message::HitList(HitListMessage::ShowOnOtherSide),
            ContextAction::RemoveFromList => Message::HitList(HitListMessage::RemoveFromList),
        }
    }
}

impl ContextActionMessage for HermesAction {
    fn message(self) -> Message {
        match self {
            HermesAction::ShowSuggestion => Message::QuickFix(QuickFixMessage::Open),
            HermesAction::HandToInbox => Message::HermesFile(HermesFileMessage::HandToInbox),
            HermesAction::ProcessAgain => Message::HermesFile(HermesFileMessage::ProcessAgain),
            HermesAction::ReprocessFolder => {
                Message::HermesFile(HermesFileMessage::ReprocessFolder)
            }
            HermesAction::SetInbox => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Inbox))
            }
            HermesAction::SetFilingLocation => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::FilingLocation))
            }
            HermesAction::SetArchive => {
                Message::HermesFile(HermesFileMessage::SetRole(FolderRole::Archive))
            }
            HermesAction::Exclude | HermesAction::Include => {
                Message::HermesFile(HermesFileMessage::ToggleExclusion)
            }
            HermesAction::Inspect => Message::HermesFile(HermesFileMessage::ShowInInspection),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::KeymapMessages as _;
    use ncrs_files::context_menu::GROUPS;

    #[test]
    fn each_entry_sends_the_message_of_its_key() {
        assert_eq!(
            ContextAction::Open.message(),
            Message::Panel(PanelMessage::OpenSelected)
        );
        assert_eq!(
            ContextAction::Trash.message(),
            Message::Delete { permanent: false }
        );
        assert_eq!(
            ContextAction::DeletePermanently.message(),
            Message::Delete { permanent: true }
        );
        for action in GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .filter(|action| **action != ContextAction::ComputeFolderSizes)
        {
            assert!(
                crate::keymap::Keymap::built_in()
                    .shortcut_label(&action.message())
                    .is_some(),
                "{action:?} has no key to show"
            );
        }
    }
}
