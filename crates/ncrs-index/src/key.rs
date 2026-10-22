use std::fmt;

use std::str::FromStr;

use crate::error::{IndexError, RecoveryKeyError};

pub const KEY_LEN: usize = 32;

/// The database key. Debug is written by hand so the bytes cannot reach a log.
pub struct IndexKey([u8; KEY_LEN]);

impl IndexKey {
    pub fn generate() -> Result<Self, IndexError> {
        let mut bytes: [u8; KEY_LEN] = [0; KEY_LEN];
        getrandom::fill(&mut bytes).map_err(|err| IndexError::Random(err.to_string()))?;
        Ok(Self(bytes))
    }

    pub fn from_stored(bytes: &[u8]) -> Result<Self, IndexError> {
        let Ok(array) = <[u8; KEY_LEN]>::try_from(bytes) else {
            return Err(IndexError::InvalidKeyLength { found: bytes.len() });
        };
        Ok(Self(array))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn recovery_key(&self) -> RecoveryKey {
        RecoveryKey(Self(self.0))
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl fmt::Debug for IndexKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IndexKey(<redacted>)")
    }
}

const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const BASE32_LEN: usize = 52;
const GROUP_LEN: usize = 4;

/// The database key as shown to the user once: RFC 4648 base32 without
/// padding, in groups of four separated by hyphens.
pub struct RecoveryKey(IndexKey);

impl RecoveryKey {
    pub(crate) fn key(&self) -> &IndexKey {
        &self.0
    }
}

impl fmt::Display for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = Vec::with_capacity(BASE32_LEN);
        let mut buffer: u16 = 0;
        let mut bits: u32 = 0;
        for byte in self.0.as_bytes() {
            buffer = (buffer << 8) | u16::from(*byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(base32_char(buffer >> bits));
            }
        }
        if bits > 0 {
            chars.push(base32_char(buffer << (5 - bits)));
        }
        for (position, ch) in chars.iter().enumerate() {
            if position > 0 && position % GROUP_LEN == 0 {
                f.write_str("-")?;
            }
            write!(f, "{}", char::from(*ch))?;
        }
        Ok(())
    }
}

fn base32_char(value: u16) -> u8 {
    BASE32_ALPHABET
        .get(usize::from(value & 0x1f))
        .copied()
        .unwrap_or(b'?')
}

impl FromStr for RecoveryKey {
    type Err = RecoveryKeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let cleaned: Vec<char> = text
            .chars()
            .filter(|ch| !ch.is_whitespace() && *ch != '-')
            .map(|ch| ch.to_ascii_uppercase())
            .collect();
        if cleaned.len() != BASE32_LEN {
            return Err(RecoveryKeyError::WrongLength {
                found: cleaned.len(),
            });
        }
        let mut bytes = Vec::with_capacity(KEY_LEN);
        let mut buffer: u16 = 0;
        let mut bits: u32 = 0;
        for ch in cleaned {
            let value = BASE32_ALPHABET
                .iter()
                .position(|candidate| char::from(*candidate) == ch)
                .ok_or(RecoveryKeyError::InvalidCharacter(ch))?;
            buffer = (buffer << 5) | u16::try_from(value).unwrap_or(0);
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push(u8::try_from(buffer >> bits).unwrap_or(0));
                buffer &= (1 << bits) - 1;
            }
        }
        if buffer != 0 {
            return Err(RecoveryKeyError::NonCanonical);
        }
        match IndexKey::from_stored(&bytes) {
            Ok(key) => Ok(RecoveryKey(key)),
            Err(_) => Err(RecoveryKeyError::NonCanonical),
        }
    }
}

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryKey(<redacted>)")
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    impl RecoveryKey {
        pub(crate) fn from_str_for_test(bytes: &[u8]) -> Self {
            Self(IndexKey::from_stored(bytes).unwrap())
        }
    }

    #[test]
    fn key_never_appears_in_debug_output() {
        let key = IndexKey::from_stored(&[0xab; 32]).unwrap();
        let recovery = key.recovery_key();
        for shown in [format!("{key:?}"), format!("{recovery:?}")] {
            assert!(shown.contains("redacted"), "{shown}");
            assert!(!shown.contains("171"), "{shown}");
        }
    }

    #[test]
    fn recovery_key_is_grouped_in_fours_with_a_short_last_group() {
        let shown = RecoveryKey::from_str_for_test(&[0; 32]).to_string();
        assert_eq!(
            shown,
            "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA"
        );
        let ones = RecoveryKey::from_str_for_test(&[0xff; 32]).to_string();
        assert_eq!(ones, format!("{}777Q", "7777-".repeat(12)));
    }

    #[test]
    fn recovery_key_matches_the_rfc_4648_encoding() {
        // "foobar" is the RFC 4648 test vector; its base32 is MZXW6YTBOI.
        let mut bytes: [u8; 32] = [0; 32];
        bytes[..6].copy_from_slice(b"foobar");
        assert!(RecoveryKey::from_str_for_test(&bytes)
            .to_string()
            .starts_with("MZXW-6YTB-OIAA"));
    }

    #[test]
    fn recovery_key_round_trips_in_every_spelling() {
        let original = IndexKey::generate().unwrap().recovery_key();
        let shown = original.to_string();
        let spellings = [
            shown.clone(),
            shown.to_lowercase(),
            shown.replace('-', ""),
            shown.replace('-', " "),
            format!("  {}  ", shown.to_lowercase().replace('-', "--")),
        ];
        for spelling in spellings {
            let parsed: RecoveryKey = spelling.parse().unwrap();
            assert_eq!(
                parsed.key().as_bytes(),
                original.key().as_bytes(),
                "{spelling}"
            );
        }
    }

    #[test]
    fn malformed_recovery_keys_are_rejected() {
        assert_eq!(
            "ABCD".parse::<RecoveryKey>().unwrap_err(),
            RecoveryKeyError::WrongLength { found: 4 }
        );
        let with_bad_char = format!("{}1", "A".repeat(51));
        assert_eq!(
            with_bad_char.parse::<RecoveryKey>().unwrap_err(),
            RecoveryKeyError::InvalidCharacter('1')
        );
        let non_canonical = "7".repeat(52);
        assert_eq!(
            non_canonical.parse::<RecoveryKey>().unwrap_err(),
            RecoveryKeyError::NonCanonical
        );
    }
}
