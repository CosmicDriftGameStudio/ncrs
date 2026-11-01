//! Starting the pipeline and the AI behind it.

use super::*;

impl FilingModule {
    /// Starts the pipeline on the reachable locations. The only way to the
    /// index: it is handed over in exactly one case, the module is `Ready` and
    /// the index is there.
    ///
    /// The second value is the folder the PDF library is missing from, when it
    /// is: PDFs wait until it is there and the next start finds it.
    pub fn start_pipeline(&self) -> Result<(Runner, Option<PathBuf>), HeldReason> {
        let handle = self.intake_index()?;
        let locations: Vec<Location> = self.reachable_locations().cloned().collect();
        let logical_cores = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        let (chain, pdf_library_missing) = shipped_text_chain(
            ocr_models_dir(&self.db_path).unwrap_or_else(|| PathBuf::from(OCR_MODELS_FOLDER)),
        );
        let runner = Runner::start(
            Settings::new(self.load, logical_cores),
            intake_steps(
                Arc::new(handle),
                Arc::new(chain),
                self.ai_provider(),
                self.own.clone(),
            ),
            locations,
            Arc::new(SystemClock),
        );
        Ok((runner, pdf_library_missing))
    }

    fn ai_provider(&self) -> Option<Arc<dyn ncrs_ai::Provider>> {
        match &self.ai {
            AiWiring::Off => None,
            #[cfg(feature = "local-ai")]
            AiWiring::Local(model) => Some(Arc::clone(model) as Arc<dyn ncrs_ai::Provider>),
            AiWiring::Http { provider, .. } => {
                Some(Arc::clone(provider) as Arc<dyn ncrs_ai::Provider>)
            }
        }
    }

    /// For the status page. Reads no disk and waits for nothing.
    pub fn ai_status(&self) -> AiStatus {
        match &self.ai {
            AiWiring::Off => AiStatus::Off,
            #[cfg(feature = "local-ai")]
            AiWiring::Local(model) => match model.status() {
                ncrs_ai::LocalModelStatus::NotDownloaded => AiStatus::NotDownloaded,
                ncrs_ai::LocalModelStatus::NotLoadedYet => AiStatus::NotLoadedYet,
                ncrs_ai::LocalModelStatus::Loaded { name, memory_bytes } => {
                    AiStatus::Loaded { name, memory_bytes }
                }
                ncrs_ai::LocalModelStatus::Failed(reason) => AiStatus::Failed(reason),
            },
            AiWiring::Http { origin, model, .. } => AiStatus::Http {
                origin: origin.clone(),
                model: model.clone(),
            },
        }
    }

    /// The setting asks for the local model and its file is not there.
    pub fn ai_model_is_missing(&self) -> bool {
        self.ai_status() == AiStatus::NotDownloaded
    }

    /// After a download: the model file is looked for again.
    pub fn refresh_ai_model(&self) {
        #[cfg(feature = "local-ai")]
        if let AiWiring::Local(model) = &self.ai {
            model.refresh();
        }
    }
}
