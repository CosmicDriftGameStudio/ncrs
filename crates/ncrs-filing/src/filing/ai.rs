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
        compute: ncrs_ai::Compute,
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
pub(in crate::filing) fn wire_ai(setting: &AiSetting, load: Load, db_path: &Path) -> AiWiring {
    match setting {
        AiSetting::Off => AiWiring::Off,
        #[cfg(feature = "local-ai")]
        AiSetting::Local { threads } => AiWiring::Local(shared_local_model(db_path, load, *threads)),
        #[cfg(not(feature = "local-ai"))]
        AiSetting::Local { .. } => {
            let _ = (db_path, load);
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

/// The local model of this index. The path keys the instance, so the fields
/// and the chat share it and the model is loaded once. The load decides how
/// many threads and what priority the documents get, unless the config names
/// a thread count.
#[cfg(feature = "local-ai")]
pub fn shared_local_model(
    db_path: &Path,
    load: Load,
    configured_threads: Option<NonZeroU32>,
) -> Arc<ncrs_ai::LazyLocalModel> {
    let folder =
        ai_models_dir(db_path).unwrap_or_else(|| PathBuf::from(ncrs_ai::LOCAL_MODEL_FOLDER));
    let threads = extraction_threads(load, configured_threads);
    let model = ncrs_ai::LazyLocalModel::shared(
        folder.join(ncrs_ai::LOCAL_MODEL.name),
        ncrs_ai::LOCAL_MODEL,
        threads,
        AI_CONTEXT_TOKENS,
    );
    model.set_extraction(threads, load.thread_priority());
    model
}

/// What the load gives the model for a document; the chat does not use it.
#[cfg(feature = "local-ai")]
pub(in crate::filing) fn extraction_threads(
    load: Load,
    configured: Option<NonZeroU32>,
) -> NonZeroU32 {
    configured.unwrap_or_else(|| {
        load.ai_threads(thread::available_parallelism().unwrap_or(NonZeroUsize::MIN))
    })
}
