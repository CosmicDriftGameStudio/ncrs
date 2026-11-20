//! The encryption of a note: one 256-bit key, AES-GCM, and the recovery key
//! that writes the key down.
//!
//! A file holds `MAGIC`, the version, a fresh 12-byte nonce and the
//! ciphertext with its tag. The header is authenticated with the text, so a
//! changed header fails like a wrong key.

use std::fmt;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use zeroize::Zeroize;

/// The ending of an encrypted note: `Name.md.enc`.
pub const ENCRYPTED_SUFFIX: &str = ".md.enc";

const MAGIC: &[u8; 8] = b"NCRSNOTE";
const VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = MAGIC.len() + 1;
const KEY_LEN: usize = 32;
/// Characters of the recovery key between two dashes.
const RECOVERY_GROUP: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptError {
    /// The bytes are not a note this program wrote.
    NotANote,
    /// A newer program wrote it.
    NewerVersion(u8),
    /// The key does not open it, or the file is damaged.
    WrongKeyOrDamaged,
    /// The system gave no random bytes.
    Random(String),
    /// The text is not a recovery key.
    BadRecoveryKey,
}

impl fmt::Display for CryptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotANote => f.write_str("not an encrypted note"),
            Self::NewerVersion(version) => {
                write!(f, "written by a newer version of ncrs (format {version})")
            }
            Self::WrongKeyOrDamaged => f.write_str("wrong key, or the file is damaged"),
            Self::Random(reason) => write!(f, "no random bytes: {reason}"),
            Self::BadRecoveryKey => f.write_str("not a recovery key"),
        }
    }
}

impl std::error::Error for CryptError {}

/// The key of all encrypted notes. Wiped from memory when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct NoteKey([u8; KEY_LEN]);

impl Drop for NoteKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for NoteKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NoteKey(..)")
    }
}

impl NoteKey {
    pub fn generate() -> Result<Self, CryptError> {
        let mut bytes = [0_u8; KEY_LEN];
        getrandom::fill(&mut bytes).map_err(|err| CryptError::Random(err.to_string()))?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let array: [u8; KEY_LEN] = bytes.try_into().ok()?;
        Some(Self(array))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The key as words to write down: base32 in groups of four.
    pub fn recovery_text(&self) -> String {
        let encoded = data_encoding::BASE32_NOPAD.encode(&self.0);
        let groups: Vec<&str> = encoded
            .as_bytes()
            .chunks(RECOVERY_GROUP)
            .filter_map(|chunk| std::str::from_utf8(chunk).ok())
            .collect();
        groups.join("-")
    }

    /// The key a recovery text names. Dashes, spaces and case do not matter.
    pub fn from_recovery_text(text: &str) -> Result<Self, CryptError> {
        let cleaned: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let mut bytes = data_encoding::BASE32_NOPAD
            .decode(cleaned.as_bytes())
            .map_err(|_| CryptError::BadRecoveryKey)?;
        let key = Self::from_bytes(&bytes).ok_or(CryptError::BadRecoveryKey);
        bytes.zeroize();
        key
    }

    fn cipher(&self) -> Result<Aes256Gcm, CryptError> {
        let key =
            Key::<Aes256Gcm>::try_from(self.0.as_slice()).map_err(|_| CryptError::NotANote)?;
        Ok(Aes256Gcm::new(&key))
    }
}

/// Whether the file at `path` is an encrypted note by its name.
pub fn is_encrypted_path(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .is_some_and(|name| name.ends_with(ENCRYPTED_SUFFIX))
}

/// `text` as the bytes of an encrypted note.
pub fn encrypt(key: &NoteKey, text: &str) -> Result<Vec<u8>, CryptError> {
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes).map_err(|err| CryptError::Random(err.to_string()))?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|_| CryptError::NotANote)?;
    let mut header = MAGIC.to_vec();
    header.push(VERSION);
    let sealed = key
        .cipher()?
        .encrypt(
            &nonce,
            Payload {
                msg: text.as_bytes(),
                aad: &header,
            },
        )
        .map_err(|_| CryptError::WrongKeyOrDamaged)?;
    let mut file = header;
    file.extend_from_slice(&nonce_bytes);
    file.extend_from_slice(&sealed);
    Ok(file)
}

/// The text of an encrypted note. A wrong key or a changed file is an error,
/// never a panic.
pub fn decrypt(key: &NoteKey, bytes: &[u8]) -> Result<String, CryptError> {
    if bytes.len() < HEADER_LEN + NONCE_LEN || !bytes.starts_with(MAGIC) {
        return Err(CryptError::NotANote);
    }
    let version = bytes
        .get(MAGIC.len())
        .copied()
        .ok_or(CryptError::NotANote)?;
    if version != VERSION {
        return Err(CryptError::NewerVersion(version));
    }
    let (header, rest) = bytes.split_at(HEADER_LEN);
    let (nonce_bytes, sealed) = rest.split_at(NONCE_LEN);
    let nonce = Nonce::try_from(nonce_bytes).map_err(|_| CryptError::NotANote)?;
    let plain = key
        .cipher()?
        .decrypt(
            &nonce,
            Payload {
                msg: sealed,
                aad: header,
            },
        )
        .map_err(|_| CryptError::WrongKeyOrDamaged)?;
    String::from_utf8(plain).map_err(|_| CryptError::WrongKeyOrDamaged)
}

/// Whether `bytes` start like an encrypted note.
pub fn looks_encrypted(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}
