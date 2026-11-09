use std::fmt;
use std::path::Path;

/// Whether the user allowed documents of a source to go to an endpoint.
pub trait ConsentLedger: Send + Sync + fmt::Debug {
    /// `endpoint_origin` is `scheme://host[:port]`; `source` is the inbox
    /// location the document came from.
    fn granted(&self, endpoint_origin: &str, source: &Path) -> bool;
}

/// Grants every call. An address off this computer gets into the settings
/// through the dialog whose "I understand" tick is the consent, or by the user
/// editing config.toml by hand; both are the user's own decision, so the saved
/// server is the one the user agreed to.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsentGivenInSettings;

impl ConsentLedger for ConsentGivenInSettings {
    fn granted(&self, _endpoint_origin: &str, _source: &Path) -> bool {
        true
    }
}

/// Exactly what a call would send, for the user to read before agreeing. The
/// key is not part of it: it travels in a header, never in the body.
#[derive(Clone, PartialEq, Eq)]
pub struct OutgoingPreview {
    pub endpoint: String,
    pub model: String,
    /// The JSON body, pretty-printed.
    pub body: String,
}

// The body holds the document text, so it stays out of logs and error output.
impl fmt::Debug for OutgoingPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutgoingPreview")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}
