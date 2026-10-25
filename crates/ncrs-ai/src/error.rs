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
        }
    }
}

impl Error for AiError {}
