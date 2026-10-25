//! The one model the built-in provider runs, pinned by size and checksum. Not
//! behind the `llama` feature: a build without the runtime still has to know
//! which file to download and how to check it.

use std::fs::{self, File};
use std::io::Read as _;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use crate::error::AiError;

/// A model file as it must be: where it comes from, how big it is and its
/// checksum. Nothing from the file is trusted before it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelPin {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub url: &'static str,
}

/// Qwen3 0.6B (Apache-2.0), 8-bit, at a fixed revision of its Hugging Face
/// repository. scripts/fetch-ai-model.sh carries the same values; change both
/// together.
pub const LOCAL_MODEL: ModelPin = ModelPin {
    name: "Qwen3-0.6B-Q8_0.gguf",
    size: 639_446_688,
    sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
    url: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/23749fefcc72300e3a2ad315e1317431b06b590a/Qwen3-0.6B-Q8_0.gguf",
};

/// The folder inside the data folder that holds the model.
pub const LOCAL_MODEL_FOLDER: &str = "ai-models";

/// Checks that `path` is a regular file (not a link) with the size and the
/// checksum of `pin`. The file is hashed as a stream, never held in memory.
pub fn verify_model_file(path: &Path, pin: &ModelPin) -> Result<(), AiError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_unreadable| AiError::Model("the model file is not there"))?;
    if !metadata.is_file() {
        return Err(AiError::Model("the model file is not a regular file"));
    }
    if metadata.len() != pin.size {
        return Err(AiError::Model("the model file has the wrong size"));
    }
    let unreadable = |_err: std::io::Error| AiError::Model("the model file could not be read");
    let mut file = File::open(path).map_err(unreadable)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![u8::MIN; 1 << 20];
    let mut read_total: u64 = 0;
    loop {
        let read = file.read(&mut buffer).map_err(unreadable)?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
        read_total = read_total.saturating_add(read as u64);
        if read_total > pin.size {
            return Err(AiError::Model("the model file has the wrong size"));
        }
    }
    if hex_of(&hasher.finalize()) == pin.sha256 {
        Ok(())
    } else {
        Err(AiError::Model("the model file is not the expected one"))
    }
}

fn hex_of(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn pin_of(bytes: &[u8]) -> ModelPin {
        let sha256: &'static str = Box::leak(hex_of(&Sha256::digest(bytes)).into_boxed_str());
        ModelPin {
            name: "model.gguf",
            size: bytes.len() as u64,
            sha256,
            url: "https://example.invalid/model.gguf",
        }
    }

    fn write(dir: &TempDir, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join("model.gguf");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn the_right_bytes_are_accepted() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model bytes");

        assert_eq!(verify_model_file(&path, &pin_of(b"model bytes")), Ok(()));
    }

    #[test]
    fn a_wrong_size_is_refused() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model bytes and more");

        assert_eq!(
            verify_model_file(&path, &pin_of(b"model bytes")),
            Err(AiError::Model("the model file has the wrong size"))
        );
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, b"model BYTES");

        assert_eq!(
            verify_model_file(&path, &pin_of(b"model bytes")),
            Err(AiError::Model("the model file is not the expected one"))
        );
    }

    #[test]
    fn a_missing_file_is_refused() {
        let dir = TempDir::new().unwrap();

        assert_eq!(
            verify_model_file(&dir.path().join("none.gguf"), &pin_of(b"x")),
            Err(AiError::Model("the model file is not there"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_the_right_bytes_is_refused() {
        let dir = TempDir::new().unwrap();
        let target = write(&dir, b"model bytes");
        let link = dir.path().join("link.gguf");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(
            verify_model_file(&link, &pin_of(b"model bytes")),
            Err(AiError::Model("the model file is not a regular file"))
        );
    }

    #[test]
    fn the_pinned_checksum_is_a_sha256_in_hex() {
        assert_eq!(LOCAL_MODEL.sha256.len(), 64);
        assert!(LOCAL_MODEL.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(LOCAL_MODEL.url.ends_with(LOCAL_MODEL.name));
    }
}
