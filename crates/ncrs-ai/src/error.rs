use std::error::Error;
use std::fmt;

use crate::consent::OutgoingPreview;

/// Why an endpoint is not accepted. Never names the endpoint: it may carry
/// a secret in its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointProblem {
    NotAnAddress,
    SchemeNotHttp,
    NoHost,
    CarriesCredentials,
    NotLocalAndNotHttps,
}

impl fmt::Display for EndpointProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnAddress => "the endpoint is not an address",
            Self::SchemeNotHttp => "the endpoint must start with http:// or https://",
            Self::NoHost => "the endpoint has no host",
            Self::CarriesCredentials => "the endpoint must not carry a user name or password",
            Self::NotLocalAndNotHttps => "an endpoint outside this machine needs https",
        })
    }
}

/// Display never holds the key or the text of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// Nothing was sent. The preview is what a call would send.
    ConsentRequired(OutgoingPreview),
    BadEndpoint(EndpointProblem),
    Transport(&'static str),
    Status(u16),
    BadReply(&'static str),
    /// The built-in model could not be loaded or run.
    Model(&'static str),
    /// What was to be sent does not fit into one request.
    TooLong,
}

/// Who a failed call is about, which decides what is worth trying again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureScope {
    /// Every call will fail the same way until the setting changes.
    Provider,
    /// This document or text fails; others may not.
    Document,
    /// Likely to work later.
    Transient,
}

/// What a person is told about a failed model call: one case per thing they
/// can do about it. The technical reason stays out of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelProblem {
    TooLong,
    BadReply,
    /// The server did not answer, or did not accept the request.
    Unreachable,
    ConsentMissing,
    /// The built-in model could not be loaded or run; the reason is fixed text.
    NotLoaded(&'static str),
    /// A task of the app itself failed, not the model.
    Internal,
}

impl From<&AiError> for ModelProblem {
    fn from(error: &AiError) -> Self {
        match error {
            AiError::TooLong => Self::TooLong,
            AiError::BadReply(_) => Self::BadReply,
            AiError::ConsentRequired(_) => Self::ConsentMissing,
            AiError::BadEndpoint(_) | AiError::Transport(_) | AiError::Status(_) => {
                Self::Unreachable
            }
            AiError::Model(reason) => Self::NotLoaded(reason),
        }
    }
}

impl AiError {
    /// The scope from the error alone. A `Document` failure of a provider that
    /// never answered anything is really a `Provider` failure: a server that
    /// does not know the model or the response format fails every document
    /// alike. `Provider::failure_scope` accounts for that.
    pub fn scope(&self) -> FailureScope {
        match self {
            Self::ConsentRequired(_) | Self::BadEndpoint(_) | Self::Model(_) => {
                FailureScope::Provider
            }
            Self::Transport(_) => FailureScope::Transient,
            Self::Status(401 | 403 | 404) => FailureScope::Provider,
            Self::Status(408 | 429 | 500..=599) => FailureScope::Transient,
            Self::Status(_) | Self::BadReply(_) | Self::TooLong => FailureScope::Document,
        }
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConsentRequired(preview) => write!(
                f,
                "sending the document to {} needs the user's consent",
                preview.endpoint
            ),
            Self::BadEndpoint(problem) => write!(f, "{problem}"),
            Self::Transport(what) => write!(f, "the AI provider could not be reached: {what}"),
            Self::Status(status) => write!(f, "the AI provider answered with status {status}"),
            Self::BadReply(what) => write!(f, "the AI provider's reply is not usable: {what}"),
            Self::Model(what) => write!(f, "the local model could not be used: {what}"),
            Self::TooLong => f.write_str("the text is too long to be sent in one request"),
        }
    }
}

impl Error for AiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_follows_what_can_change_the_outcome() {
        assert_eq!(AiError::Status(401).scope(), FailureScope::Provider);
        assert_eq!(AiError::Status(429).scope(), FailureScope::Transient);
        assert_eq!(
            AiError::Transport("no connection").scope(),
            FailureScope::Transient
        );
        assert_eq!(AiError::TooLong.scope(), FailureScope::Document);
        assert_eq!(AiError::Status(400).scope(), FailureScope::Document);
    }
}
