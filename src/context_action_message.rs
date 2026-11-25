//! The message each context menu entry sends: exactly the one its key sends.

use ncrs_files::context_menu::{ContextAction, HermesAction};

use crate::fs::OpenKind;
use crate::messages::PanelMessage;
use crate::messages::{
    ClipboardKind, CompareMessage, HermesFileMessage, HitListMessage, Message, TransferKind,
};

pub trait ContextActionMessage {
    fn message(self) -> Message;
}

impl ContextActionMessage for ContextAction {
    fn message(self) -> Message {
        match self {
            ContextAction::Open => Message::Panel(PanelMessage::OpenSelected),
            ContextAction::ShowPackageContents => Message::Panel(PanelMessage::ShowPackageContents),
            ContextAction::View => Message::OpenExternal(OpenKind::View),
            ContextAction::Edit => Message::OpenExternal(OpenKind::Edit),
            ContextAction::OpenTerminal => Message::OpenTerminal,
            ContextAction::Copy => Message::Transfer(TransferKind::Copy),
            ContextAction::Move => Message::Transfer(TransferKind::Move),
            ContextAction::Permissions => Message::PermissionsPrompt,
            ContextAction::Rename => Message::Panel(PanelMessage::RenameStart),
            ContextAction::NewFolder => Message::CreateDirPrompt,
            ContextAction::CopyPath => Message::CopyToClipboard(ClipboardKind::Path),
            ContextAction::CopyName => Message::CopyToClipboard(ClipboardKind::Name),
            ContextAction::ComputeFolderSizes => Message::ComputeFolderSizes,
            ContextAction::CompareFolders => Message::Compare(CompareMessage::Open),
            ContextAction::GitView => Message::OpenGitView,
            ContextAction::AddFavourite => Message::AddFavourite,
            ContextAction::RemoveFavourite => Message::RemoveFavourite,
            ContextAction::Trash => Message::Delete { permanent: false },
            ContextAction::DeletePermanently => Message::Delete { permanent: true },
            ContextAction::ShowOnOtherSide => Message::HitList(HitListMessage::ShowOnOtherSide),
            ContextAction::RemoveFromList => Message::HitList(HitListMessage::RemoveFromList),
            ContextAction::NoteOnFile => Message::NoteOnFile,
            ContextAction::ExtractHere => Message::ExtractHere,
            ContextAction::Pack => Message::PackPrompt,
        }
    }
}

impl ContextActionMessage for HermesAction {
    fn message(self) -> Message {
        match self {
            HermesAction::ProcessAgain => Message::HermesFile(HermesFileMessage::ProcessAgain),
            HermesAction::FileOnce => Message::HermesFile(HermesFileMessage::FileOnce),
            HermesAction::WatchSource | HermesAction::ChangeSource => {
                Message::HermesFile(HermesFileMessage::WatchSource)
            }
            HermesAction::AddTarget | HermesAction::ChangeTarget => {
                Message::HermesFile(HermesFileMessage::AddTarget)
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
            .filter(|action| {
                !matches!(
                    action,
                    ContextAction::ComputeFolderSizes
                        | ContextAction::CompareFolders
                        | ContextAction::GitView
                        | ContextAction::RemoveFavourite
                        | ContextAction::ShowPackageContents
                        | ContextAction::ExtractHere
                )
            })
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
