//! A provider that runs the pinned model inside this process (llama.cpp). No
//! document leaves the machine, so there is no consent to ask for. The model
//! can only write what the reply grammar allows: fields, or one chat intent.
//!
//! `LocalModel` is the loaded model. `LazyLocalModel` is what the rest of the
//! app holds: it loads on the first job, on its own worker thread, and lets
//! go of the memory again after a quiet time.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::vocab::LlamaVocab;
use llama_cpp_2::{list_llama_ggml_backend_devices, LlamaBackendDeviceType};

use crate::contract::{parse_reply, Extraction, ExtractionRequest, Provider};
use crate::error::AiError;
use crate::grammar::reply_grammar;
use crate::model_file::{verify_model_file, ModelPin};
use crate::prompt::{user_message_prefix, user_message_suffix, SYSTEM_PROMPT};
use crate::status::Compute;

mod chat_session;
mod lazy;
mod worker;

pub use lazy::LazyLocalModel;

#[cfg(test)]
mod chat_eval;
#[cfg(test)]
mod tests;

/// What the reply may take: a handful of short fields.
const MAX_NEW_TOKENS: usize = 256;
/// Tokens handed to llama.cpp per decode call.
const MAX_BATCH_TOKENS: u32 = 512;

#[derive(Debug, Clone)]
pub struct LocalModelConfig {
    pub path: PathBuf,
    /// Threads the model computes with, independent of the pipeline's workers.
    pub threads: NonZeroU32,
    /// The prompt (instructions and document) and the reply share this.
    pub context_tokens: NonZeroU32,
}

/// The runtime exists once per process: a second `init` is an error, and the
/// stored `None` keeps that failure from being retried. The one place that
/// starts a compute backend, so a GPU backend is added here.
fn backend() -> Result<&'static LlamaBackend, AiError> {
    static BACKEND: OnceLock<Option<LlamaBackend>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            LlamaBackend::init().ok().map(|mut backend| {
                backend.void_logs();
                backend
            })
        })
        .as_ref()
        .ok_or(AiError::Model("the model runtime could not start"))
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
        let backend = backend()?;
        let refused = |_err| AiError::Model("llama.cpp could not load the model file");
        // llama.cpp's default puts every layer on the GPU backend if the build
        // has one (Metal on Apple silicon); if that fails, the CPU still works.
        let (model, compute) =
            match LlamaModel::load_from_file(backend, &config.path, &LlamaModelParams::default()) {
                Ok(model) => (model, gpu_in_use()),
                Err(_gpu_refused) => {
                    let params = LlamaModelParams::default().with_n_gpu_layers(0);
                    let model = LlamaModel::load_from_file(backend, &config.path, &params)
                        .map_err(refused)?;
                    (model, Compute::Cpu)
                }
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
    ) -> Result<LlamaContext<'_>, AiError> {
        let threads = i32::try_from(self.threads.get()).unwrap_or(i32::MAX);
        let params = LlamaContextParams::default()
            .with_n_ctx(Some(context_tokens))
            .with_n_batch(batch_tokens)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);
        self.model
            .new_context(backend()?, params)
            .map_err(|_refused| AiError::Model("llama.cpp could not set up a context"))
    }

    fn generate(&self, request: &ExtractionRequest) -> Result<String, AiError> {
        let vocab = self.model.vocab();
        let context_tokens = self.context_tokens.get();
        let batch_tokens = context_tokens.min(MAX_BATCH_TOKENS);
        let batch_len = usize::try_from(batch_tokens).unwrap_or(1);
        let prompt_limit = usize::try_from(context_tokens)
            .unwrap_or(usize::MAX)
            .saturating_sub(MAX_NEW_TOKENS);
        let prompt = prompt_tokens(&vocab, request, prompt_limit)?;

        let mut context = self.new_context(self.context_tokens, batch_tokens)?;
        let mut batch = LlamaBatch::new(batch_len, 1);
        decode_tokens(&mut context, &mut batch, &prompt, 0, batch_len)?;
        self.sample_reply(
            &mut context,
            &mut batch,
            &reply_grammar(&request.open_fields),
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
        let grammar = LlamaSampler::grammar(&self.model, grammar, "root")
            .map_err(|_refused| AiError::Model("llama.cpp refused the reply grammar"))?;
        let mut sampler = LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()]);

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

/// The first graphics device of the runtime: where the layers run when the
/// model was loaded with the default parameters.
fn gpu_in_use() -> Compute {
    list_llama_ggml_backend_devices()
        .into_iter()
        .find(|device| {
            matches!(
                device.device_type,
                LlamaBackendDeviceType::Gpu | LlamaBackendDeviceType::IntegratedGpu
            )
        })
        .map_or(Compute::Cpu, |device| Compute::Gpu(device.description))
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
        parse_reply(&self.generate(request)?, request)
    }
}

/// The text of a document as tokens. Special tokens are not parsed, so
/// `<|im_end|>` inside a document stays plain text and cannot end its turn.
pub fn document_tokens(vocab: &LlamaVocab<'_>, text: &str) -> Vec<LlamaToken> {
    vocab.tokenize(text.as_bytes(), false, false)
}

/// The prompt in the ChatML of the pinned Qwen3, written here and not read
/// from the model file. The fixed parts are tokenized with special tokens, the
/// document alone without, and the document is cut so the whole fits `limit`.
pub fn prompt_tokens(
    vocab: &LlamaVocab<'_>,
    request: &ExtractionRequest,
    limit: usize,
) -> Result<Vec<LlamaToken>, AiError> {
    let head = format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n<|im_start|>user\n{}",
        user_message_prefix(&request.open_fields)
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
