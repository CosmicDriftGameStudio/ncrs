use crate::app::*;

/// An overlay over the panels or a page: a dialog, a card, a menu or the
/// prompt. The view draws the one on top and the keys go to it. The dialogs
/// and menus that belong to a page are the page's and come after these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modal {
    QuickFix,
    HeaderMenu,
    FavouriteMenu,
    ContextMenu,
    VolumeMenu,
    Delete,
    Quit,
    OpenFile,
    BatchRename,
    Permissions,
    About,
    OcrModels,
    AiModel,
    ConfirmAgain,
    StartOver,
    Compare,
    Connect,
    Grant,
    Conflict,
    TransferDialog,
    Progress,
    QuickOpen,
    KeyBindings,
    CommandPalette,
    ThemeFetch,
    Prompt,
}

impl PromptKeyState {
    /// The open overlays, the one on top first. Most are opened only while
    /// nothing else is up; this order decides for the ones that arrive on
    /// their own (a conflict, the progress of a job, the grant at the start).
    pub fn modal_stack(&self) -> Vec<Modal> {
        [
            (self.quick_fix, Modal::QuickFix),
            (self.header_menu, Modal::HeaderMenu),
            (self.favourite_menu, Modal::FavouriteMenu),
            (self.context_menu, Modal::ContextMenu),
            (self.volume_menu, Modal::VolumeMenu),
            (self.delete_dialog != DeleteKeys::Closed, Modal::Delete),
            (self.quit_dialog, Modal::Quit),
            (self.open_file_dialog, Modal::OpenFile),
            (self.batch_rename_dialog, Modal::BatchRename),
            (self.permissions_dialog, Modal::Permissions),
            (self.about, Modal::About),
            (self.ocr_models, Modal::OcrModels),
            (self.ai_model, Modal::AiModel),
            (self.confirm_again, Modal::ConfirmAgain),
            (self.start_over_dialog, Modal::StartOver),
            (self.compare_dialog, Modal::Compare),
            (self.connect_dialog, Modal::Connect),
            (self.grant_dialog, Modal::Grant),
            (self.conflict_open, Modal::Conflict),
            (self.transfer_dialog, Modal::TransferDialog),
            (self.transfer_progress, Modal::Progress),
            (self.quick_open, Modal::QuickOpen),
            (self.key_bindings, Modal::KeyBindings),
            (self.command_palette, Modal::CommandPalette),
            (self.theme_fetch, Modal::ThemeFetch),
            (self.open, Modal::Prompt),
        ]
        .into_iter()
        .filter_map(|(open, modal)| open.then_some(modal))
        .collect()
    }

    /// The overlay on top, which the view draws and the keys go to.
    pub fn top_modal(&self) -> Option<Modal> {
        self.modal_stack().first().copied()
    }
}
