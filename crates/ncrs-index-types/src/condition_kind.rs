/// Substring: the text contains the value, without regard to case. Word: the
/// text contains the word or phrase between word boundaries. Regex: the value
/// is a regex source run over the text. Sender: the sender of the document
/// equals the value, without regard to case. SenderAddress: an e-mail domain or
/// address found in the text. Iban and TaxId: the number occurs in the text,
/// whatever the spacing. Recipient: the name or address stands in the addressee
/// block. Folder: the folder the document lies in. ReadFolder: the document
/// lies under the read folder whose path is the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionKind {
    Substring,
    Word,
    Regex,
    Sender,
    SenderAddress,
    Iban,
    TaxId,
    Recipient,
    Folder,
    /// The document has a tag (own or automatic) that is this one or below it.
    Tag,
    /// The kind field of the document is this kind; the value is the kind key.
    Kind,
    /// The document's current path lies under this read folder (the value is its absolute path).
    ReadFolder,
}

impl ConditionKind {
    pub const ALL: [Self; 12] = [
        Self::Substring,
        Self::Word,
        Self::Regex,
        Self::Sender,
        Self::SenderAddress,
        Self::Iban,
        Self::TaxId,
        Self::Recipient,
        Self::Folder,
        Self::Tag,
        Self::Kind,
        Self::ReadFolder,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Substring => "substring",
            Self::Word => "word",
            Self::Regex => "regex",
            Self::Sender => "sender",
            Self::SenderAddress => "sender_address",
            Self::Iban => "iban",
            Self::TaxId => "tax_id",
            Self::Recipient => "recipient",
            Self::Folder => "folder",
            Self::Tag => "tag",
            Self::Kind => "kind",
            Self::ReadFolder => "read_folder",
        }
    }
}
