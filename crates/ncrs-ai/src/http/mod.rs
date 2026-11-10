//! A provider that speaks the chat completions API many servers offer (a model
//! on this machine, or a hosted one). It does nothing until it is asked for a
//! document, and a document only leaves this machine with consent.

use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use jsonc_parser::{parse_to_value, JsonValue, ParseOptions};
use ureq::http::Uri;

use crate::api_key::KeyLookup;
use crate::consent::{ConsentLedger, OutgoingPreview};
use crate::contract::{output_schema, parse_reply, Extraction, ExtractionRequest, Provider};
use crate::error::{AiError, EndpointProblem};
use crate::json::Json;
use crate::prompt::{user_message, SYSTEM_PROMPT};
use crate::usage_log::{append_usage, parse_usage, UsageRecord};
use chrono::Utc;

mod chat;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A model can take its time on a long document.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// A reply holds a handful of short fields; anything beyond this is not one.
pub const MAX_REPLY_BYTES: u64 = 256 * 1024;

/// An API key. Debug and Display print `***`, and nothing else in this crate
/// has a way to show it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(key: String) -> Self {
        Self(key)
    }

    /// For the field that shows the key as dots and for the header it travels in.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    /// The API base, for example `https://host/v1`; `/chat/completions` is
    /// added.
    pub endpoint: String,
    pub model: String,
    /// A key to send as it is, over the lookup: the one typed into a test.
    pub api_key: Option<Secret>,
    /// Asked for the key of every request when `api_key` is none.
    pub key_lookup: Option<Arc<dyn KeyLookup>>,
    /// The file one line per answered request is added to; none logs nothing.
    pub usage_log: Option<PathBuf>,
}

/// The endpoint, parsed once.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    chat_url: String,
    models_url: String,
    /// `scheme://host[:port]`, what consent is given for.
    origin: String,
    /// The host alone, lower case and without brackets.
    host: String,
    /// `host[:port]`, lower case: what an API key is kept under.
    authority: String,
    /// This machine: no document leaves it, so no consent is needed.
    local: bool,
}

impl Endpoint {
    fn parse(text: &str) -> Result<Self, EndpointProblem> {
        let uri: Uri = text
            .trim()
            .parse()
            .map_err(|_unparsable| EndpointProblem::NotAnAddress)?;
        let secure = match uri.scheme_str() {
            Some(scheme) if scheme.eq_ignore_ascii_case("https") => true,
            Some(scheme) if scheme.eq_ignore_ascii_case("http") => false,
            _ => return Err(EndpointProblem::SchemeNotHttp),
        };
        let authority = uri.authority().ok_or(EndpointProblem::NoHost)?;
        if authority.as_str().contains('@') {
            return Err(EndpointProblem::CarriesCredentials);
        }
        let host = authority
            .host()
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        if host.is_empty() {
            return Err(EndpointProblem::NoHost);
        }
        let local = is_loopback(&host);
        if !local && !secure {
            return Err(EndpointProblem::NotLocalAndNotHttps);
        }
        let scheme = if secure { "https" } else { "http" };
        let authority_text = authority.as_str().to_ascii_lowercase();
        let origin = format!("{scheme}://{authority_text}");
        let base = format!("{origin}{}", uri.path().trim_end_matches('/'));
        Ok(Self {
            chat_url: format!("{base}/chat/completions"),
            models_url: format!("{base}/models"),
            origin,
            host,
            authority: authority_text,
            local,
        })
    }
}

/// Whether `text` is an endpoint this provider accepts, for a setting to be
/// checked before a provider is built from it.
pub fn check_endpoint(text: &str) -> Result<(), EndpointProblem> {
    Endpoint::parse(text).map(|_accepted| ())
}

/// Whether an accepted endpoint is on this machine (`localhost` or a loopback
/// address), the only place text is sent to for now.
pub fn endpoint_is_local(text: &str) -> Result<bool, EndpointProblem> {
    Endpoint::parse(text).map(|endpoint| endpoint.local)
}

/// `scheme://host[:port]` of an accepted endpoint, what a person is shown.
pub fn endpoint_origin(text: &str) -> Result<String, EndpointProblem> {
    Endpoint::parse(text).map(|endpoint| endpoint.origin)
}

/// The host alone of an accepted endpoint, for the sentence that says where
/// documents go.
pub fn endpoint_host(text: &str) -> Result<String, EndpointProblem> {
    Endpoint::parse(text).map(|endpoint| endpoint.host)
}

/// What an API key for this endpoint is kept under: `host[:port]`.
pub fn endpoint_key_name(text: &str) -> Result<String, EndpointProblem> {
    Endpoint::parse(text).map(|endpoint| endpoint.authority)
}

/// OpenRouter reports the price of a request when asked; other servers may
/// refuse a field they do not know, so only this host is asked.
fn is_openrouter(host: &str) -> bool {
    host == "openrouter.ai" || host.ends_with(".openrouter.ai")
}

/// Anthropic's compatible endpoint answers every `response_format` but a strict
/// JSON schema with a 400, so the format is left out for this host.
fn is_anthropic(host: &str) -> bool {
    host == "api.anthropic.com" || host.ends_with(".anthropic.com")
}

/// How the last request of a provider went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyFailure {
    /// The server answered with this HTTP status.
    Status(u16),
    /// No connection, no answer in time, or a reply that could not be read.
    NoAnswer,
}

/// The part of a reply content from the first `{` to the last `}`: a model may
/// wrap its JSON in a code fence or a sentence. Unchanged without braces.
fn outermost_object(content: &str) -> &str {
    match (content.find('{'), content.rfind('}')) {
        (Some(first), Some(last)) if first < last => content.get(first..=last).unwrap_or(content),
        _ => content,
    }
}

/// `localhost` or a loopback address (127.0.0.0/8, ::1), judged on the host of
/// the parsed address, not on the text around it.
fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[derive(Debug)]
pub struct OpenAiCompatible {
    endpoint: Endpoint,
    model: String,
    api_key: Option<Secret>,
    key_lookup: Option<Arc<dyn KeyLookup>>,
    usage_log: Option<PathBuf>,
    ledger: Arc<dyn ConsentLedger>,
    last_failure: Mutex<Option<ReplyFailure>>,
}

impl OpenAiCompatible {
    pub fn new(
        config: OpenAiCompatibleConfig,
        ledger: Arc<dyn ConsentLedger>,
    ) -> Result<Self, AiError> {
        let endpoint = Endpoint::parse(&config.endpoint).map_err(AiError::BadEndpoint)?;
        Ok(Self {
            endpoint,
            model: config.model,
            api_key: config.api_key,
            key_lookup: config.key_lookup,
            usage_log: config.usage_log,
            ledger,
            last_failure: Mutex::new(None),
        })
    }

    /// What went wrong with the last request; none when it was answered.
    pub fn last_failure(&self) -> Option<ReplyFailure> {
        *self
            .last_failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn remember(&self, outcome: &Result<String, AiError>) {
        let failure = match outcome {
            Ok(_) => None,
            Err(AiError::Status(status)) => Some(ReplyFailure::Status(*status)),
            Err(_) => Some(ReplyFailure::NoAnswer),
        };
        *self
            .last_failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = failure;
    }

    /// What a call for this request would send, for a person to read first.
    pub fn preview(&self, request: &ExtractionRequest) -> OutgoingPreview {
        OutgoingPreview {
            endpoint: self.endpoint.origin.clone(),
            model: self.model.clone(),
            body: self.request_body(request).pretty(),
        }
    }

    fn request_body(&self, request: &ExtractionRequest) -> Json {
        let user = user_message(request);
        let mut body = Json::object([
            ("model", Json::text(&self.model)),
            (
                "messages",
                Json::List(vec![
                    Json::object([
                        ("role", Json::text("system")),
                        ("content", Json::text(SYSTEM_PROMPT)),
                    ]),
                    Json::object([("role", Json::text("user")), ("content", Json::Text(user))]),
                ]),
            ),
        ]);
        self.add_response_format(&mut body, || {
            Json::object([
                ("type", Json::text("json_schema")),
                (
                    "json_schema",
                    // Not strict: strict mode wants every property required,
                    // and a field the model cannot find is left out.
                    Json::object([
                        ("name", Json::text("document_fields")),
                        ("strict", Json::Bool(false)),
                        (
                            "schema",
                            output_schema(&request.open_fields, &request.kinds),
                        ),
                    ]),
                ),
            ])
        });
        self.include_usage(&mut body);
        body
    }

    fn add_response_format(&self, body: &mut Json, format: impl FnOnce() -> Json) {
        if is_anthropic(&self.endpoint.host) {
            return;
        }
        if let Json::Object(entries) = body {
            entries.push(("response_format".to_owned(), format()));
        }
    }

    fn include_usage(&self, body: &mut Json) {
        if is_openrouter(&self.endpoint.host) {
            if let Json::Object(entries) = body {
                entries.push((
                    "usage".to_owned(),
                    Json::object([("include", Json::Bool(true))]),
                ));
            }
        }
    }

    fn log_usage(&self, reply: &str) {
        let Some(path) = &self.usage_log else {
            return;
        };
        let record = UsageRecord {
            time: Utc::now(),
            model: self.model.clone(),
            usage: parse_usage(reply),
        };
        // The answer is already here: a log that cannot be written must not
        // take it away.
        let _ignored = append_usage(path, &record);
    }

    fn post(&self, body: &str) -> Result<String, AiError> {
        let agent = agent();
        let call = agent
            .post(&self.endpoint.chat_url)
            .header("Content-Type", "application/json");
        let key = key_of(self.api_key.as_ref(), self.key_lookup.as_deref());
        read_reply(with_key(call, key.as_ref()).send(body))
    }
}

/// The explicit key, else what the lookup finds right now.
fn key_of(explicit: Option<&Secret>, lookup: Option<&dyn KeyLookup>) -> Option<Secret> {
    explicit
        .cloned()
        .or_else(|| lookup.and_then(KeyLookup::key))
}

fn with_key<B>(call: ureq::RequestBuilder<B>, key: Option<&Secret>) -> ureq::RequestBuilder<B> {
    match key {
        Some(secret) => call.header("Authorization", format!("Bearer {}", secret.expose())),
        None => call,
    }
}

/// The body of a 200 answer; any other status is an error.
fn read_reply(
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<String, AiError> {
    let mut response = sent.map_err(transport_error)?;
    let status = response.status().as_u16();
    if status != 200 {
        return Err(AiError::Status(status));
    }
    read_body(&mut response)
}

fn read_body(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, AiError> {
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_REPLY_BYTES)
        .read_to_vec()
        .map_err(transport_error)?;
    String::from_utf8(bytes).map_err(|_not_utf8| AiError::BadReply("not valid UTF-8"))
}

/// The models a server lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelList {
    /// The ids, sorted.
    Listed(Vec<String>),
    /// The server answers, but has no list to offer.
    NotListed,
}

/// `GET {endpoint}/models` with the key. Sends no document, so it needs no
/// consent. A refused key is `AiError::Status(401 or 403)`; a server that
/// does not answer is `AiError::Transport`.
pub fn list_models(config: &OpenAiCompatibleConfig) -> Result<ModelList, AiError> {
    let endpoint = Endpoint::parse(&config.endpoint).map_err(AiError::BadEndpoint)?;
    // Anthropic serves `/models` from its own API, which answers a request
    // without this header with a 400; other servers ignore it.
    let call = agent()
        .get(&endpoint.models_url)
        .header("anthropic-version", "2023-06-01");
    let key = key_of(config.api_key.as_ref(), config.key_lookup.as_deref());
    let mut response = with_key(call, key.as_ref())
        .call()
        .map_err(transport_error)?;
    let status = response.status().as_u16();
    // A refusal or a missing list has no body worth reading.
    let body = if status == 200 {
        read_body(&mut response)?
    } else {
        String::new()
    };
    models_from_answer(status, &body)
}

fn models_from_answer(status: u16, body: &str) -> Result<ModelList, AiError> {
    match status {
        200 => Ok(model_ids(body).map_or(ModelList::NotListed, ModelList::Listed)),
        404 => Ok(ModelList::NotListed),
        other => Err(AiError::Status(other)),
    }
}

/// `data[].id` of a models reply, sorted; `None` when there is none to read.
fn model_ids(body: &str) -> Option<Vec<String>> {
    let Ok(Some(JsonValue::Object(root))) = parse_to_value(body, &ParseOptions::default()) else {
        return None;
    };
    let mut ids: Vec<String> = root
        .get_array("data")?
        .iter()
        .filter_map(|entry| match entry {
            JsonValue::Object(model) => model.get_string("id").map(|id| id.to_string()),
            _ => None,
        })
        .collect();
    if ids.is_empty() {
        return None;
    }
    ids.sort();
    ids.dedup();
    Some(ids)
}

impl OpenAiCompatible {
    fn may_send(&self, source: &Path) -> bool {
        self.endpoint.local || self.ledger.granted(&self.endpoint.origin, source)
    }
}

impl Provider for OpenAiCompatible {
    fn can_answer(&self, source: &Path) -> bool {
        self.may_send(source)
    }

    fn extract(&self, request: &ExtractionRequest) -> Result<Extraction, AiError> {
        // First, before an agent or a request exists: without consent nothing
        // is sent.
        if !self.may_send(&request.source) {
            return Err(AiError::ConsentRequired(self.preview(request)));
        }
        let outcome = self.post(&self.request_body(request).pretty());
        self.remember(&outcome);
        let reply = outcome?;
        self.log_usage(&reply);
        parse_reply(&message_content(&reply)?, request)
    }
}

/// `choices[0].message.content` of a chat completions reply.
fn message_content(reply: &str) -> Result<String, AiError> {
    let parsed = parse_to_value(reply, &ParseOptions::default())
        .map_err(|_syntax| AiError::BadReply("not valid JSON"))?;
    let Some(JsonValue::Object(root)) = parsed else {
        return Err(AiError::BadReply("not a JSON object"));
    };
    root.get_array("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| match choice {
            JsonValue::Object(object) => object.get_object("message"),
            _ => None,
        })
        .and_then(|message| message.get_string("content"))
        .map(|content| outermost_object(content).to_owned())
        .ok_or(AiError::BadReply("no message in the reply"))
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("ncrs/", env!("CARGO_PKG_VERSION")))
        // The document goes to the endpoint the user agreed to and nowhere
        // else, so a redirect is an answer, not something to follow.
        .max_redirects(0)
        .http_status_as_error(false)
        // An environment proxy would be a third party the preview did not show.
        .proxy(None)
        .build();
    ureq::Agent::new_with_config(config)
}

/// Says what kind of failure it was and nothing from the request or the
/// error text, which can hold the address.
fn transport_error(err: ureq::Error) -> AiError {
    match err {
        ureq::Error::Timeout(_) => AiError::Transport("the request timed out"),
        ureq::Error::BodyExceedsLimit(_) => AiError::BadReply("the reply is too large"),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            AiError::Transport("the TLS handshake failed")
        }
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => {
            AiError::Transport("no connection")
        }
        _ => AiError::Transport("the request failed"),
    }
}

#[cfg(test)]
mod tests;
