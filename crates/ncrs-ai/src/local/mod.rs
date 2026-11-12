//! A provider that runs the pinned model inside this process (llama.cpp). No
//! document leaves the machine, so there is no consent to ask for. The model
//! can only write what the reply grammar allows: fields, or one chat intent.
//!
//! `LocalModel` is the loaded model. `LazyLocalModel` is what the rest of the
//! app holds: it loads on the first job, on its own worker thread, and lets
//! go of the memory again after a quiet time.

use std::num::{NonZeroU32, NonZeroUsize};
use std::path::PathBuf;
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::list_llama_ggml_backend_devices;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::vocab::LlamaVocab;

use crate::contract::{parse_reply, Extraction, ExtractionRequest, Provider};
use crate::error::AiError;
use crate::grammar::reply_grammar;
use crate::model_file::{remove_previous_models, verify_model_file, ModelPin};
use crate::prompt::{user_message_prefix, user_message_suffix, SYSTEM_PROMPT};
use crate::status::Compute;

mod backends;
mod chat_session;
mod check;
mod lazy;
mod rule_session;
mod worker;

pub use check::{check_local_model, LocalCheck};
pub use lazy::LazyLocalModel;
pub use rule_session::RuleSession;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub use tests::models_dir;

/// What the reply may take: a handful of short fields.
const MAX_NEW_TOKENS: usize = 256;
/// Tokens handed to llama.cpp per decode call.
const MAX_BATCH_TOKENS: u32 = 512;

#[derive(Debug, Clone)]
pub struct LocalModelConfig {
    pub path: PathBuf,
    /// Threads the model reads documents with, unless a job names another count.
    pub threads: NonZeroU32,
    /// The prompt (instructions and document) and the reply share this.
    pub context_tokens: NonZeroU32,
}

/// The runtime exists once per process: a second `init` is an error, and the
/// stored failure keeps it from being retried. The one place that loads the
/// compute backends and starts the runtime.
fn backend() -> Result<&'static LlamaBackend, AiError> {
    static BACKEND: OnceLock<Result<LlamaBackend, &'static str>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            backends::load_modules()?;
            let mut backend =
                LlamaBackend::init().map_err(|_refused| "the model runtime could not start")?;
            backend.void_logs();
            Ok(backend)
        })
        .as_ref()
        .map_err(|reason| AiError::Model(reason))
}

#[derive(Debug)]
pub struct LocalModel {
    model: LlamaModel,
    name: String,
    threads: NonZeroU32,
    context_tokens: NonZeroU32,
    compute: Compute,
}

impl LocalModel {
    /// Checks size and checksum of the file against `pin` before the path
    /// reaches llama.cpp, then loads it.
    pub fn load(config: LocalModelConfig, pin: &ModelPin) -> Result<Self, AiError> {
        verify_model_file(&config.path, pin)?;
        if let Some(dir) = config.path.parent() {
            remove_previous_models(dir);
        }
        let backend = backend()?;
        let (model, compute) = match gpu_attempt() {
            Some((params, device)) => LlamaModel::load_from_file(backend, &config.path, &params)
                .map(|model| (model, Compute::Gpu(device)))
                .or_else(|_gpu_refused| load_on_cpu(backend, &config.path))?,
            None => load_on_cpu(backend, &config.path)?,
        };
        Ok(Self {
            model,
            name: pin.name.to_owned(),
            threads: config.threads,
            context_tokens: config.context_tokens,
            compute,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.name
    }

    /// The bytes of the model's tensors, which is about what it holds in memory.
    pub fn memory_bytes(&self) -> u64 {
        self.model.size()
    }

    pub fn compute(&self) -> &Compute {
        &self.compute
    }

    fn new_context(
        &self,
        context_tokens: NonZeroU32,
        batch_tokens: u32,
        threads: NonZeroU32,
    ) -> Result<LlamaContext<'_>, AiError> {
        let thread_count = i32::try_from(threads.get()).unwrap_or(i32::MAX);
        let params = LlamaContextParams::default()
            .with_n_ctx(Some(context_tokens))
            .with_n_batch(batch_tokens)
            .with_n_threads(thread_count)
            .with_n_threads_batch(thread_count);
        self.model
            .new_context(backend()?, params)
            .map_err(|_refused| AiError::Model("llama.cpp could not set up a context"))
    }

    /// Reads a document with `threads` instead of the count of the config.
    pub fn extract_with_threads(
        &self,
        request: &ExtractionRequest,
        threads: NonZeroU32,
    ) -> Result<Extraction, AiError> {
        parse_reply(&self.generate(request, threads)?, request)
    }

    fn generate(
        &self,
        request: &ExtractionRequest,
        threads: NonZeroU32,
    ) -> Result<String, AiError> {
        let vocab = self.model.vocab();
        let context_tokens = self.context_tokens.get();
        let batch_tokens = context_tokens.min(MAX_BATCH_TOKENS);
        let batch_len = usize::try_from(batch_tokens).unwrap_or(1);
        let prompt_limit = usize::try_from(context_tokens)
            .unwrap_or(usize::MAX)
            .saturating_sub(MAX_NEW_TOKENS);
        let prompt = prompt_tokens(&vocab, request, prompt_limit)?;

        let mut context = self.new_context(self.context_tokens, batch_tokens, threads)?;
        let mut batch = LlamaBatch::new(batch_len, 1);
        decode_tokens(&mut context, &mut batch, &prompt, 0, batch_len)?;
        self.sample_reply(
            &mut context,
            &mut batch,
            &reply_grammar(&request.open_fields, request.judge_document),
            prompt.len(),
            MAX_NEW_TOKENS,
        )
    }

    /// Writes the reply token by token after the prompt that was decoded
    /// last. Grammar first, greedy second: the grammar rules out every token
    /// that would leave the reply shape, so greedy only ever sees legal ones.
    fn sample_reply(
        &self,
        context: &mut LlamaContext<'_>,
        batch: &mut LlamaBatch<'_>,
        grammar: &str,
        first_position: usize,
        max_new_tokens: usize,
    ) -> Result<String, AiError> {
        let vocab = self.model.vocab();
        let constraint = LlamaSampler::grammar(&self.model, grammar, "root")
            .map_err(|_refused| AiError::Model("llama.cpp refused the reply grammar"))?;
        let mut sampler = LlamaSampler::chain_simple([constraint, LlamaSampler::greedy()]);

        let mut reply = Vec::new();
        for _ in 0..max_new_tokens {
            let token = sampler.sample(context, batch.n_tokens() - 1);
            if vocab.is_eog(token) {
                break;
            }
            let position = i32::try_from(first_position + reply.len())
                .map_err(|_too_long| AiError::Model("the reply is too long"))?;
            reply.push(token);
            batch.clear();
            batch
                .add(token, position, &[0], true)
                .map_err(|_full| AiError::Model("the model could not continue"))?;
            context.decode(batch).map_err(decoding_failed)?;
        }
        Ok(String::from_utf8_lossy(&vocab.detokenize(&reply, false, false)).into_owned())
    }
}

fn decoding_failed(_err: impl std::error::Error) -> AiError {
    AiError::Model("the model could not read the document")
}

/// The parameters for the first load and the GPU they use, or none when
/// there is no hardware GPU. Every layer goes to the GPU backend; a software
/// rasteriser listed next to a real GPU is left out.
fn gpu_attempt() -> Option<(LlamaModelParams, String)> {
    let devices = list_llama_ggml_backend_devices();
    let hardware: Vec<_> = devices
        .iter()
        .filter(|device| backends::is_hardware_gpu(device.device_type, &device.description))
        .collect();
    let first = hardware.first()?;
    let with_software = devices
        .iter()
        .filter(|device| backends::is_gpu_type(device.device_type))
        .count()
        > hardware.len();
    let params = if with_software {
        let indices: Vec<usize> = hardware.iter().map(|device| device.index).collect();
        LlamaModelParams::default().with_devices(&indices).ok()?
    } else {
        LlamaModelParams::default()
    };
    Some((params, first.description.clone()))
}

fn load_on_cpu(
    backend: &LlamaBackend,
    path: &std::path::Path,
) -> Result<(LlamaModel, Compute), AiError> {
    let params = LlamaModelParams::default().with_n_gpu_layers(0);
    LlamaModel::load_from_file(backend, path, &params)
        .map(|model| (model, Compute::Cpu))
        .map_err(|_refused| AiError::Model("llama.cpp could not load the model file"))
}

/// Feeds `tokens` to the context starting at `first_position`, in batches.
/// Only the last token asks for logits, which is where sampling starts.
fn decode_tokens(
    context: &mut LlamaContext<'_>,
    batch: &mut LlamaBatch<'_>,
    tokens: &[LlamaToken],
    first_position: usize,
    batch_len: usize,
) -> Result<(), AiError> {
    let last = tokens.len().saturating_sub(1);
    for (chunk_number, chunk) in tokens.chunks(batch_len).enumerate() {
        batch.clear();
        for (offset, token) in chunk.iter().enumerate() {
            let index = chunk_number * batch_len + offset;
            let position = i32::try_from(first_position + index)
                .map_err(|_too_long| AiError::Model("the prompt is too long"))?;
            batch
                .add(*token, position, &[0], index == last)
                .map_err(|_full| AiError::Model("the prompt is too long"))?;
        }
        context.decode(batch).map_err(decoding_failed)?;
    }
    Ok(())
}

impl Provider for LocalModel {
    fn extract(&self, request: &ExtractionRequest) -> Result<Extraction, AiError> {
        self.extract_with_threads(request, self.threads)
    }
}

/// The chat is waited for by the person typing, so it gets half the cores no
/// matter how gentle the indexing is.
pub fn chat_threads() -> NonZeroU32 {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    u32::try_from(cores.div_euclid(2))
        .ok()
        .and_then(NonZeroU32::new)
        .unwrap_or(NonZeroU32::MIN)
}

/// The text of a document as tokens. Special tokens are not parsed, so
/// `<|im_end|>` inside a document stays plain text and cannot end its turn.
pub fn document_tokens(vocab: &LlamaVocab<'_>, text: &str) -> Vec<LlamaToken> {
    vocab.tokenize(text.as_bytes(), false, false)
}

/// The prompt in the ChatML of the pinned Qwen3.5, written here and not read
/// from the model file. The fixed parts are tokenized with special tokens, the
/// document alone without, and the document is cut so the whole fits `limit`.
pub fn prompt_tokens(
    vocab: &LlamaVocab<'_>,
    request: &ExtractionRequest,
    limit: usize,
) -> Result<Vec<LlamaToken>, AiError> {
    let head = format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{}",
        user_message_prefix(&request.open_fields, request.judge_document)
    );
    let tail = format!(
        "{}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
        user_message_suffix()
    );
    let mut tokens = vocab.tokenize(head.as_bytes(), vocab.should_add_bos(), true);
    let tail_tokens = vocab.tokenize(tail.as_bytes(), false, true);
    let room = limit
        .checked_sub(tokens.len() + tail_tokens.len())
        .ok_or(AiError::Model(
            "the context is too small for the instructions",
        ))?;
    tokens.extend(document_tokens(vocab, &request.text).into_iter().take(room));
    tokens.extend(tail_tokens);
    Ok(tokens)
}
