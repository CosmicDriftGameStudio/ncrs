use std::fmt;
use std::path::Path;

/// What kind of data a call hands to a server. The user agrees to each kind
/// on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsentPurpose {
    /// New documents Hermes reads to sort them.
    Inbox,
    /// Text and data of documents already filed, asked about in the chat and
    /// when learning rules.
    Archive,
    /// Folder structure and file names.
    Folders,
}

impl ConsentPurpose {
    pub const ALL: [Self; 3] = [Self::Inbox, Self::Archive, Self::Folders];
}

/// The servers (`scheme://host[:port]`) the user agreed to, for each kind of
/// data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsentedOrigins {
    pub inbox: Vec<String>,
    pub archive: Vec<String>,
    pub folders: Vec<String>,
}

impl ConsentedOrigins {
    /// The same servers for every kind.
    pub fn all(origins: Vec<String>) -> Self {
        Self {
            inbox: origins.clone(),
            archive: origins.clone(),
            folders: origins,
        }
    }

    pub fn of(&self, purpose: ConsentPurpose) -> &[String] {
        match purpose {
            ConsentPurpose::Inbox => &self.inbox,
            ConsentPurpose::Archive => &self.archive,
            ConsentPurpose::Folders => &self.folders,
        }
    }

    pub fn of_mut(&mut self, purpose: ConsentPurpose) -> &mut Vec<String> {
        match purpose {
            ConsentPurpose::Inbox => &mut self.inbox,
            ConsentPurpose::Archive => &mut self.archive,
            ConsentPurpose::Folders => &mut self.folders,
        }
    }

    /// Takes back the agreement to send `purpose` to the server at `origin`,
    /// however it was written. The other kinds stay.
    pub fn withdraw(&mut self, purpose: ConsentPurpose, origin: &str) {
        let wanted = normalized(origin);
        self.of_mut(purpose)
            .retain(|listed| normalized(listed) != wanted);
    }
}

/// Whether the user allowed a kind of data of a source to go to an endpoint.
pub trait ConsentLedger: Send + Sync + fmt::Debug {
    /// `endpoint_origin` is `scheme://host[:port]`; `source` is the inbox
    /// location the document came from.
    fn granted(&self, endpoint_origin: &str, source: &Path, purpose: ConsentPurpose) -> bool;
}

/// Grants the servers the user agreed to in the settings for a kind of data,
/// and no other. The source does not matter: the consent is for the server.
#[derive(Debug, Clone, Default)]
pub struct ConsentGivenInSettings {
    origins: ConsentedOrigins,
}

impl ConsentGivenInSettings {
    pub fn new(origins: &ConsentedOrigins) -> Self {
        let normalize = |list: &[String]| list.iter().map(|origin| normalized(origin)).collect();
        Self {
            origins: ConsentedOrigins {
                inbox: normalize(&origins.inbox),
                archive: normalize(&origins.archive),
                folders: normalize(&origins.folders),
            },
        }
    }
}

impl ConsentLedger for ConsentGivenInSettings {
    fn granted(&self, endpoint_origin: &str, _source: &Path, purpose: ConsentPurpose) -> bool {
        self.origins
            .of(purpose)
            .contains(&normalized(endpoint_origin))
    }
}

/// A hand-written origin may differ in case or carry a closing slash.
fn normalized(origin: &str) -> String {
    origin.trim().trim_end_matches('/').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_server_the_user_agreed_to_is_granted() {
        let ledger = ConsentGivenInSettings::new(&ConsentedOrigins::all(vec![
            "https://ai.example.com".to_owned(),
        ]));
        let source = Path::new("inbox");
        let inbox = ConsentPurpose::Inbox;

        assert!(ledger.granted("https://ai.example.com", source, inbox));
        assert!(ledger.granted("HTTPS://AI.example.com/", source, inbox));
        assert!(!ledger.granted("https://other.example.org", source, inbox));
        assert!(!ledger.granted("https://ai.example.com:8443", source, inbox));
        assert!(!ConsentGivenInSettings::default().granted(
            "https://ai.example.com",
            source,
            inbox
        ));
    }

    #[test]
    fn each_kind_of_data_is_granted_on_its_own() {
        let ledger = ConsentGivenInSettings::new(&ConsentedOrigins {
            inbox: vec!["https://ai.example.com".to_owned()],
            archive: Vec::new(),
            folders: vec!["https://ai.example.com".to_owned()],
        });
        let source = Path::new("inbox");
        let granted = |purpose| ledger.granted("https://ai.example.com", source, purpose);

        assert!(granted(ConsentPurpose::Inbox));
        assert!(!granted(ConsentPurpose::Archive));
        assert!(granted(ConsentPurpose::Folders));
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
