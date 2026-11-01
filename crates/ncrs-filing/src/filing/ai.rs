//! What the filing module wires up for the AI of the config.

use super::*;

/// How much text of a document the local model takes in; longer ones are cut.
#[cfg(feature = "local-ai")]
pub(in crate::filing) const AI_CONTEXT_TOKENS: NonZeroU32 = match NonZeroU32::new(4096) {
    Some(tokens) => tokens,
    None => NonZeroU32::MIN,
};

#[derive(Clone)]
pub(in crate::filing) enum AiWiring {
    Off,
    #[cfg(feature = "local-ai")]
    Local(Arc<ncrs_ai::LazyLocalModel>),
    Http {
        provider: Arc<ncrs_ai::OpenAiCompatible>,
        origin: String,
        model: String,
    },
}

/// What the status page says about the AI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AiStatus {
    #[default]
    Off,
    NotDownloaded,
    // Only the local model reports these; the build without it never does.
    #[cfg_attr(not(feature = "local-ai"), allow(dead_code))]
    NotLoadedYet,
    #[cfg_attr(not(feature = "local-ai"), allow(dead_code))]
    Loaded {
        name: String,
        memory_bytes: u64,
    },
    #[cfg_attr(not(feature = "local-ai"), allow(dead_code))]
    Failed(&'static str),
    Http {
        origin: String,
        model: String,
    },
}

/// Builds nothing that reads a model or reaches a server: the local model is
/// loaded when the first document needs it, a remote endpoint is only parsed.
pub(in crate::filing) fn wire_ai(setting: &AiSetting, db_path: &Path) -> AiWiring {
    match setting {
        AiSetting::Off => AiWiring::Off,
        #[cfg(feature = "local-ai")]
        AiSetting::Local { threads } => {
            let folder = ai_models_dir(db_path)
                .unwrap_or_else(|| PathBuf::from(ncrs_ai::LOCAL_MODEL_FOLDER));
            AiWiring::Local(Arc::new(ncrs_ai::LazyLocalModel::new(
                folder.join(ncrs_ai::LOCAL_MODEL.name),
                ncrs_ai::LOCAL_MODEL,
                local_threads(*threads),
                AI_CONTEXT_TOKENS,
            )))
        }
        #[cfg(not(feature = "local-ai"))]
        AiSetting::Local { .. } => {
            let _ = db_path;
            AiWiring::Off
        }
        AiSetting::Http { endpoint, model } => {
            let config = ncrs_ai::OpenAiCompatibleConfig {
                endpoint: endpoint.clone(),
                model: model.clone(),
                api_key: None,
            };
            // No way to ask for consent yet: a far endpoint is refused at the
            // first document, a local one needs none.
            let ledger = Arc::new(ncrs_ai::NoConsentGiven);
            match (
                ncrs_ai::OpenAiCompatible::new(config, ledger),
                ncrs_ai::endpoint_origin(endpoint),
            ) {
                (Ok(provider), Ok(origin)) => AiWiring::Http {
                    provider: Arc::new(provider),
                    origin,
                    model: model.clone(),
                },
                (Err(_), _) | (_, Err(_)) => AiWiring::Off,
            }
        }
    }
}

/// The configured count, or half the logical cores: the model should not take
/// the whole machine from the person using it.
#[cfg(feature = "local-ai")]
pub(in crate::filing) fn local_threads(configured: Option<NonZeroU32>) -> NonZeroU32 {
    configured.unwrap_or_else(|| {
        let cores = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        u32::try_from(cores.div_euclid(2))
            .ok()
            .and_then(NonZeroU32::new)
            .unwrap_or(NonZeroU32::MIN)
    })
}
