use crate::ai_model::{AiModelCard, InstallOutcome};
use crate::key_bindings::KeyBindings;
use crate::ocr_models::OcrModelsCard;
use crate::settings::SettingsState;
use crate::theme_fetch::ThemeFetch;

/// What only the settings use: the page and the cards on top of the panels.
/// Each card has a generation, so the answer of a download that was closed and
/// opened again is not taken for the answer of the new one.
#[derive(Debug, Default)]
pub struct Settings {
    pub page: Option<SettingsState>,
    /// Shown in the bar of the page; a value refused at its row shows there.
    pub failure: Option<String>,
    pub theme_fetch: Option<ThemeFetch>,
    pub theme_fetch_generation: u64,
    pub key_bindings: Option<KeyBindings>,
    pub ocr_models: Option<OcrModelsCard>,
    pub ocr_models_generation: u64,
    pub ai_model: Option<AiModelCard>,
    pub ai_model_generation: u64,
    /// The download goes on behind the overview, so the card is not drawn.
    pub ai_card_hidden: bool,
}

impl Settings {
    pub fn ai_model_download_runs(&self) -> bool {
        self.ai_model
            .as_ref()
            .is_some_and(AiModelCard::is_downloading)
    }

    pub fn ocr_models_download_runs(&self) -> bool {
        self.ocr_models
            .as_ref()
            .is_some_and(OcrModelsCard::is_downloading)
    }

    /// A model card that has nothing left to ask: it only reports, and has
    /// Close or Done. The AI card that could not turn the AI on has a second
    /// button.
    pub fn model_card_has_one_button(&self) -> bool {
        let ocr = self
            .ocr_models
            .as_ref()
            .is_some_and(|card| card.is_installed() || card.is_finished());
        let ai = self.ai_model.as_ref().is_some_and(|card| {
            card.is_installed()
                || (card.is_finished() && card.outcome() != Some(InstallOutcome::StillOff))
        });
        ocr || ai
    }

    pub fn ai_card_is_shown(&self) -> bool {
        self.ai_model.is_some() && !self.ai_card_hidden
    }
}
