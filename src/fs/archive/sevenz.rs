//! What the 7z reader gives the archive layer: entry kinds, times, modes, and
//! the refusals 7z needs: an encrypted archive and one whose dictionary is too
//! large to unpack safely.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::time::SystemTime;

use sevenz_rust2::{ArchiveEntry, ArchiveReader, Coder, EncoderMethod, Error, Password};

use super::index::EntryKind;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// 7-Zip keeps the unix mode in the high 16 bits when this bit is set.
const FILE_ATTRIBUTE_UNIX_EXTENSION: u32 = 0x8000;
const UNIX_TYPE_MASK: u32 = 0o170_000;
const UNIX_SYMLINK: u32 = 0o120_000;

/// Largest LZMA or LZMA2 dictionary ncrs unpacks. It covers every 7-Zip preset
/// (Ultra uses 64 MiB). The decoder grows its window up to the dictionary size
/// as data arrives and sevenz-rust2 sets no memory limit of its own
/// (`MAX_MEM_LIMIT_KB` is `usize::MAX / 1024`), so a tiny hostile file could
/// otherwise ask for gigabytes.
const MAX_SEVENZ_DICTIONARY: u64 = 256 << 20;

const LZMA_PROPERTY_LENGTH: usize = 5;
const LZMA2_UNLIMITED_PROPERTY: u8 = 40;

/// Why a 7z archive is refused, though it may be fine. Carried inside an
/// `io::Error`, so the layers above need no new error type; [`refusal_of`]
/// tells it from any other failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SevenZipRefusal {
    /// Its content or header is encrypted.
    Encrypted,
    /// A dictionary larger than [`MAX_SEVENZ_DICTIONARY`].
    DictionaryTooLarge,
}

impl fmt::Display for SevenZipRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Encrypted => "this archive is encrypted; ncrs cannot open it yet",
            Self::DictionaryTooLarge => {
                "this 7z archive needs more than 256 MB of memory to unpack"
            }
        })
    }
}

impl std::error::Error for SevenZipRefusal {}

fn refused(refusal: SevenZipRefusal) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, refusal)
}

pub fn refusal_of(error: &io::Error) -> Option<SevenZipRefusal> {
    error.get_ref()?.downcast_ref::<SevenZipRefusal>().copied()
}

pub fn io_error(error: Error) -> io::Error {
    match error {
        Error::Io(inner, _) => inner,
        Error::PasswordRequired | Error::MaybeBadPassword(_) => refused(SevenZipRefusal::Encrypted),
        // Built without `aes256`, so an encrypted header or block surfaces as
        // a method the decoder does not know.
        Error::UnsupportedCompressionMethod(method)
            if method == EncoderMethod::AES256_SHA256.name() =>
        {
            refused(SevenZipRefusal::Encrypted)
        }
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// The dictionary a coder asks for, or `None` for a method without one. A
/// property field too short or out of range is a broken archive.
fn dictionary_of(coder: &Coder) -> io::Result<Option<u64>> {
    let broken = |what| io::Error::new(io::ErrorKind::InvalidData, what);
    let properties = coder.properties();
    match coder.encoder_method_id() {
        EncoderMethod::ID_LZMA => {
            let bytes = properties
                .get(1..LZMA_PROPERTY_LENGTH)
                .ok_or_else(|| broken("LZMA properties too short"))?;
            // Little endian, the way 7z stores it.
            Ok(Some(
                bytes
                    .iter()
                    .rev()
                    .fold(0, |size, byte| (size << 8) | u64::from(*byte)),
            ))
        }
        EncoderMethod::ID_LZMA2 => {
            let property = *properties
                .first()
                .ok_or_else(|| broken("LZMA2 properties too short"))?;
            match property {
                LZMA2_UNLIMITED_PROPERTY => Ok(Some(u64::from(u32::MAX))),
                0..LZMA2_UNLIMITED_PROPERTY => Ok(Some(
                    (2 | u64::from(property & 1)) << ((property >> 1) + 11),
                )),
                _ => Err(broken("unsupported LZMA2 dictionary property")),
            }
        }
        _ => Ok(None),
    }
}

/// Opens the archive and reads its header. An archive with any encrypted block,
/// or a dictionary over the limit, is refused here, so nothing later decodes
/// or lists it.
pub fn open(path: &Path) -> io::Result<ArchiveReader<BufReader<File>>> {
    let reader = ArchiveReader::new(BufReader::new(File::open(path)?), Password::empty())
        .map_err(io_error)?;
    let coders = || {
        reader
            .archive()
            .blocks
            .iter()
            .flat_map(|block| block.coders.iter())
    };
    if coders().any(|coder| coder.encoder_method_id() == EncoderMethod::ID_AES256_SHA256) {
        return Err(refused(SevenZipRefusal::Encrypted));
    }
    for coder in coders() {
        if dictionary_of(coder)?.is_some_and(|size| size > MAX_SEVENZ_DICTIONARY) {
            return Err(refused(SevenZipRefusal::DictionaryTooLarge));
        }
    }
    Ok(reader)
}

/// Folders, anti-items (deletion marks), reparse points and unix symlinks are
/// never created, as with links in tar and zip.
pub fn kind_of(entry: &ArchiveEntry) -> EntryKind {
    if entry.is_anti_item {
        return EntryKind::Other;
    }
    if entry.is_directory {
        return EntryKind::Dir;
    }
    let attributes = entry.windows_attributes;
    let is_link = entry.has_windows_attributes
        && (attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (attributes & FILE_ATTRIBUTE_UNIX_EXTENSION != 0
                && (attributes >> 16) & UNIX_TYPE_MASK == UNIX_SYMLINK));
    if is_link {
        EntryKind::Other
    } else {
        EntryKind::File
    }
}

pub fn unix_mode(entry: &ArchiveEntry) -> Option<u32> {
    let attributes = entry.windows_attributes;
    (entry.has_windows_attributes && attributes & FILE_ATTRIBUTE_UNIX_EXTENSION != 0)
        .then_some(attributes >> 16)
}

pub fn modified(entry: &ArchiveEntry) -> Option<SystemTime> {
    entry
        .has_last_modified_date
        .then(|| SystemTime::from(entry.last_modified_date))
}
