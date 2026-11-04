//! What `ncrs --check-ai` runs: the model as a packaged app would use it,
//! to see on a clean machine that the backends load and the model answers.

use std::num::NonZeroU32;
use std::path::Path;
use std::time::{Duration, Instant};

use llama_cpp_2::list_llama_ggml_backend_devices;

use super::{backend, backends, LazyLocalModel};
use crate::chat::{ChatModel as _, ChatTurn};
use crate::error::AiError;
use crate::model_file::LOCAL_MODEL;
use crate::status::{Compute, LocalModelStatus};

const THREADS: NonZeroU32 = NonZeroU32::new(4).unwrap();
const CONTEXT_TOKENS: NonZeroU32 = NonZeroU32::new(4096).unwrap();

/// The first one is asked once before the timed round: it loads the model and
/// warms up the compute graphs.
const SENTENCES: [&str; 3] = [
    "mach das rückgängig",
    "räum meine downloads auf",
    "sortier den scans ordner in dokumente, originale behalten",
];

#[derive(Debug)]
pub struct LocalCheck {
    /// One `<type> <description>` per device the runtime found.
    pub devices: Vec<String>,
    pub compute: Compute,
    pub sentence_times: Vec<Duration>,
}

pub fn check_local_model(models_dir: &Path) -> Result<LocalCheck, AiError> {
    backend()?;
    let devices = list_llama_ggml_backend_devices()
        .into_iter()
        .map(|device| {
            format!(
                "{} {}",
                backends::device_type_name(device.device_type),
                device.description
            )
        })
        .collect();

    let model = LazyLocalModel::new(
        models_dir.join(LOCAL_MODEL.name),
        LOCAL_MODEL,
        THREADS,
        CONTEXT_TOKENS,
    );
    let turn = |sentence: &str| ChatTurn {
        message: sentence.to_owned(),
        question: None,
        previous_search: None,
        previous_request: None,
        previous_message: None,
    };
    model.understand(&turn(SENTENCES[0]))?;
    let compute = match model.status() {
        LocalModelStatus::Loaded { compute, .. } => compute,
        _ => {
            return Err(AiError::Model(
                "the model is not loaded after its first answer",
            ))
        }
    };

    let mut sentence_times = Vec::new();
    for sentence in SENTENCES {
        let started = Instant::now();
        model.understand(&turn(sentence))?;
        sentence_times.push(started.elapsed());
    }
    Ok(LocalCheck {
        devices,
        compute,
        sentence_times,
    })
}
