//! What the local model reports about itself. Not behind the `llama` feature:
//! the status page and the chat show it in builds without the runtime too.

/// What the model computes on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compute {
    /// The description of the graphics device the layers run on.
    Gpu(String),
    Cpu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalModelStatus {
    NotDownloaded,
    /// Asleep (not yet needed, or unloaded after a quiet time) or loading.
    NotLoadedYet,
    Loaded {
        name: String,
        memory_bytes: u64,
        compute: Compute,
    },
    Failed(&'static str),
}
