//! Sender aliases and the identifiers they are made of.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SenderAliasKind {
    Name,
    Domain,
    Iban,
    TaxId,
}

impl SenderAliasKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Domain => "domain",
            Self::Iban => "iban",
            Self::TaxId => "tax_id",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "name" => Ok(Self::Name),
            "domain" => Ok(Self::Domain),
            "iban" => Ok(Self::Iban),
            "tax_id" => Ok(Self::TaxId),
            other => Err(corrupt("sender_alias.kind", other)),
        }
    }

    /// The form in which values of this kind are stored and compared.
    pub fn normalise(self, value: &str) -> String {
        match self {
            Self::Name => {
                crate::names::name_key(&value.split_whitespace().collect::<Vec<_>>().join(" "))
            }
            Self::Domain => {
                let lowered = value.trim().to_lowercase();
                lowered.strip_prefix("www.").unwrap_or(&lowered).to_owned()
            }
            Self::Iban => value
                .chars()
                .filter(|ch| !is_iban_separator(*ch))
                .flat_map(char::to_uppercase)
                .collect(),
            Self::TaxId => value
                .chars()
                .filter(|ch| !is_tax_id_separator(*ch))
                .flat_map(char::to_uppercase)
                .collect(),
        }
    }
}

pub fn is_iban_separator(ch: char) -> bool {
    ch.is_whitespace()
}

/// ISO 13616: separators are ignored, case does not matter.
pub fn iban_is_valid(value: &str) -> bool {
    let chars: Vec<char> = value
        .chars()
        .filter(|ch| !is_iban_separator(*ch))
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    if !(15..=34).contains(&chars.len()) || !chars.iter().all(char::is_ascii_alphanumeric) {
        return false;
    }
    let country_is_letters = chars.iter().take(2).all(char::is_ascii_alphabetic);
    let check_is_digits = chars.iter().skip(2).take(2).all(char::is_ascii_digit);
    if !country_is_letters || !check_is_digits {
        return false;
    }
    let remainder =
        chars
            .iter()
            .skip(4)
            .chain(chars.iter().take(4))
            .try_fold(0, |rest: u32, ch| {
                let digit_value = ch.to_digit(36)?;
                let shift = if digit_value < 10 { 10 } else { 100 };
                Some((rest * shift + digit_value) % 97)
            });
    remainder == Some(1)
}

pub fn is_tax_id_separator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '/' | '-' | '.')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SenderAliasState {
    Offered,
    Confirmed,
    Discarded,
}

impl SenderAliasState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Offered => "offered",
            Self::Confirmed => "confirmed",
            Self::Discarded => "discarded",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, IndexError> {
        match value {
            "offered" => Ok(Self::Offered),
            "confirmed" => Ok(Self::Confirmed),
            "discarded" => Ok(Self::Discarded),
            other => Err(corrupt("sender_alias.state", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderAlias {
    pub id: SenderAliasId,
    pub sender: String,
    pub kind: SenderAliasKind,
    /// Normalised.
    pub value: String,
    pub state: SenderAliasState,
}

/// The confirmed aliases, which is what planning and detection need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SenderAliases {
    senders: std::collections::HashMap<(SenderAliasKind, String), String>,
}

impl SenderAliases {
    /// Entries are (kind, value, sender); values are normalised here.
    pub fn from_confirmed<'a>(
        entries: impl IntoIterator<Item = (SenderAliasKind, &'a str, &'a str)>,
    ) -> Self {
        Self {
            senders: entries
                .into_iter()
                .map(|(kind, value, sender)| ((kind, kind.normalise(value)), sender.to_owned()))
                .collect(),
        }
    }

    /// The sender a confirmed alias of this kind and value stands for.
    pub fn sender_for(&self, kind: SenderAliasKind, raw_value: &str) -> Option<&str> {
        self.senders
            .get(&(kind, kind.normalise(raw_value)))
            .map(String::as_str)
    }

    /// The sender a confirmed name alias maps `name` to, else `name` itself.
    pub fn canonical_sender<'a>(&'a self, name: &'a str) -> &'a str {
        self.sender_for(SenderAliasKind::Name, name).unwrap_or(name)
    }
}
