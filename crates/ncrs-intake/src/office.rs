//! Text from plain-text and office documents: `.txt` and `.md`, and the XML
//! inside `.docx` and `.odt` packages. Nothing is extracted to disk and no
//! office program is involved.
//!
//! Text found here is data. Nothing in it triggers anything; it is only stored.

use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::Path;

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use crate::text::{Extracted, TextError, TextSource};

/// Extraction stops here; what was read up to then is the text.
const MAX_TEXT_BYTES: usize = 1 << 20;
/// An entry of a package that inflates to more than this is refused, so a zip
/// bomb cannot fill the memory.
const MAX_ENTRY_BYTES: u64 = 32 << 20;

#[derive(Debug, Clone, Copy)]
struct Limits {
    text_bytes: usize,
    entry_bytes: u64,
}

const LIMITS: Limits = Limits {
    text_bytes: MAX_TEXT_BYTES,
    entry_bytes: MAX_ENTRY_BYTES,
};

fn has_extension(path: &Path, wanted: &[&str]) -> bool {
    path.extension().is_some_and(|extension| {
        wanted
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}

fn unreadable(path: &Path, reason: impl ToString) -> TextError {
    TextError::Unreadable {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

/// `.txt` and `.md` files, read as UTF-8 with invalid bytes replaced.
#[derive(Debug, Default)]
pub struct PlainText;

impl PlainText {
    fn extract_within(path: &Path, limits: Limits) -> Result<Extracted, TextError> {
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|file| file.take(limits.text_bytes as u64).read_to_end(&mut bytes))
            .map_err(|err| unreadable(path, err))?;
        Ok(Self::decode(&bytes))
    }

    fn decode(bytes: &[u8]) -> Extracted {
        Extracted::Text(String::from_utf8_lossy(bytes).trim().to_owned())
    }
}

impl TextSource for PlainText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        if has_extension(path, &["txt", "md"]) {
            Self::extract_within(path, LIMITS)
        } else {
            Ok(Extracted::NotApplicable)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Package {
    Docx,
    Odt,
}

impl Package {
    fn of(path: &Path) -> Option<Self> {
        if has_extension(path, &["docx"]) {
            Some(Self::Docx)
        } else if has_extension(path, &["odt"]) {
            Some(Self::Odt)
        } else {
            None
        }
    }

    fn content_entry(self) -> &'static str {
        match self {
            Self::Docx => "word/document.xml",
            Self::Odt => "content.xml",
        }
    }
}

/// `.docx` (`word/document.xml`) and `.odt` (`content.xml`): the text runs,
/// paragraphs as line breaks.
#[derive(Debug, Default)]
pub struct OfficeText;

impl OfficeText {
    fn extract_within(
        path: &Path,
        package: Package,
        limits: Limits,
    ) -> Result<Extracted, TextError> {
        let file = File::open(path).map_err(|err| unreadable(path, err))?;
        Self::extract_from_reader(path, file, package, limits)
    }

    fn extract_from_reader(
        path: &Path,
        reader: impl Read + Seek,
        package: Package,
        limits: Limits,
    ) -> Result<Extracted, TextError> {
        let fail = |reason: String| unreadable(path, reason);
        let mut archive = ZipArchive::new(reader)
            .map_err(|err| fail(format!("cannot open the document: {err}")))?;
        let entry_name = package.content_entry();
        let entry = archive
            .by_name(entry_name)
            .map_err(|err| fail(format!("cannot read {entry_name}: {err}")))?;
        let mut xml = Vec::new();
        entry
            .take(limits.entry_bytes.saturating_add(1))
            .read_to_end(&mut xml)
            .map_err(|err| fail(format!("cannot read {entry_name}: {err}")))?;
        if xml.len() as u64 > limits.entry_bytes {
            return Err(fail(format!(
                "{entry_name} is larger than {} bytes unpacked",
                limits.entry_bytes
            )));
        }
        text_of_xml(&xml, package, limits.text_bytes)
            .map(Extracted::Text)
            .map_err(|reason| fail(format!("cannot read {entry_name}: {reason}")))
    }
}

impl TextSource for OfficeText {
    fn extract(&self, path: &Path) -> Result<Extracted, TextError> {
        match Package::of(path) {
            Some(package) => Self::extract_within(path, package, LIMITS),
            None => Ok(Extracted::NotApplicable),
        }
    }
}

/// The text of a document already in memory, for a caller that holds the file
/// open and must not open the path again. The kind comes from the extension of
/// `path`, the limits are those of [`PlainText`] and [`OfficeText`].
pub fn extract_text_from_bytes(path: &Path, bytes: &[u8]) -> Result<Extracted, TextError> {
    if has_extension(path, &["txt", "md"]) {
        let head = bytes.get(..LIMITS.text_bytes).unwrap_or(bytes);
        Ok(PlainText::decode(head))
    } else if let Some(package) = Package::of(path) {
        OfficeText::extract_from_reader(path, Cursor::new(bytes), package, LIMITS)
    } else {
        Ok(Extracted::NotApplicable)
    }
}

fn text_of_xml(xml: &[u8], package: Package, max_bytes: usize) -> Result<String, String> {
    let mut reader = Reader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut out = String::new();
    // Inside a text run (docx `t`) or a paragraph or heading (odt `p`, `h`).
    let mut in_text: usize = 0;
    // Inside a docx run: `tab` there is a tab character, in `tabs` a tab stop.
    let mut in_run: usize = 0;
    while out.len() < max_bytes {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|err| err.to_string())?;
        match event {
            Event::Start(start) => {
                match (package, start.local_name().as_ref()) {
                    (Package::Docx, "t") | (Package::Odt, "p" | "h") => in_text += 1,
                    (Package::Docx, "r") => in_run += 1,
                    _ => {}
                }
                push_break(&start, package, in_text, in_run, &mut out);
            }
            Event::Empty(empty) => {
                if is_paragraph(&empty, package) {
                    out.push('\n');
                }
                push_break(&empty, package, in_text, in_run, &mut out);
            }
            Event::End(end) => match (package, end.local_name().as_ref()) {
                (Package::Docx, "t") => in_text = in_text.saturating_sub(1),
                (Package::Docx, "r") => in_run = in_run.saturating_sub(1),
                (Package::Docx, "p") => out.push('\n'),
                (Package::Odt, "p" | "h") => {
                    in_text = in_text.saturating_sub(1);
                    out.push('\n');
                }
                _ => {}
            },
            Event::Text(text) if in_text > 0 => {
                out.push_str(&text.xml10_content());
            }
            Event::CData(data) if in_text > 0 => {
                out.push_str(&data.xml10_content());
            }
            Event::GeneralRef(reference) if in_text > 0 => push_reference(&reference, &mut out)?,
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    while !out.is_char_boundary(max_bytes.min(out.len())) {
        out.pop();
    }
    out.truncate(max_bytes);
    Ok(out.trim().to_owned())
}

fn is_paragraph(element: &BytesStart<'_>, package: Package) -> bool {
    matches!(
        (package, element.local_name().as_ref()),
        (Package::Docx, "p") | (Package::Odt, "p" | "h")
    )
}

/// Tabs and line breaks inside text.
fn push_break(
    element: &BytesStart<'_>,
    package: Package,
    in_text: usize,
    in_run: usize,
    out: &mut String,
) {
    match (package, element.local_name().as_ref()) {
        (Package::Docx, "tab") if in_run > 0 => out.push('\t'),
        (Package::Docx, "br" | "cr") if in_run > 0 => out.push('\n'),
        (Package::Odt, "tab") if in_text > 0 => out.push('\t'),
        (Package::Odt, "line-break") if in_text > 0 => out.push('\n'),
        (Package::Odt, "s") if in_text > 0 => out.push(' '),
        _ => {}
    }
}

/// Character references and the five predefined entities. Entities declared in
/// the document are never expanded.
fn push_reference(reference: &BytesRef<'_>, out: &mut String) -> Result<(), String> {
    if let Some(character) = reference
        .resolve_char_ref()
        .map_err(|err| err.to_string())?
    {
        out.push(character);
        return Ok(());
    }
    match &**reference {
        "amp" => out.push('&'),
        "lt" => out.push('<'),
        "gt" => out.push('>'),
        "quot" => out.push('"'),
        "apos" => out.push('\''),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::io::{Cursor, Write as _};

    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::*;
    use crate::test_support::write;

    fn package(entry: &str, xml: &str) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file(entry, options).unwrap();
        writer.write_all(xml.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn docx(body: &str) -> Vec<u8> {
        package(
            "word/document.xml",
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
            ),
        )
    }

    fn odt(body: &str) -> Vec<u8> {
        package(
            "content.xml",
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
            ),
        )
    }

    fn text_of(extracted: Result<Extracted, TextError>) -> String {
        match extracted.unwrap() {
            Extracted::Text(text) => text,
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn a_text_file_yields_its_text() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.txt", b"Invoice 4711 Company A\nPaid");
        assert_eq!(
            text_of(PlainText.extract(&path)),
            "Invoice 4711 Company A\nPaid"
        );
    }

    #[test]
    fn a_markdown_file_with_umlauts_keeps_them() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "note.MD",
            "# Rechnung\nGrüße, Müller äöü ß".as_bytes(),
        );
        assert_eq!(
            text_of(PlainText.extract(&path)),
            "# Rechnung\nGrüße, Müller äöü ß"
        );
    }

    #[test]
    fn invalid_bytes_in_a_text_file_are_replaced() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.txt", b"Company \xFF A");
        assert_eq!(text_of(PlainText.extract(&path)), "Company \u{FFFD} A");
    }

    #[test]
    fn a_text_file_is_cut_at_the_text_limit() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "note.txt", b"0123456789");
        let limits = Limits {
            text_bytes: 4,
            entry_bytes: MAX_ENTRY_BYTES,
        };
        assert_eq!(text_of(PlainText::extract_within(&path, limits)), "0123");
    }

    #[test]
    fn other_extensions_are_not_handled() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "scan.pdf", b"text");
        assert_eq!(PlainText.extract(&path).unwrap(), Extracted::NotApplicable);
        assert_eq!(OfficeText.extract(&path).unwrap(), Extracted::NotApplicable);
    }

    #[test]
    fn a_docx_yields_paragraphs_as_lines_and_keeps_umlauts() {
        let dir = TempDir::new().unwrap();
        let bytes = docx(
            "<w:p><w:r><w:t>Rechnung für Company A</w:t></w:r></w:p>\
             <w:p><w:r><w:t>Grüße &amp; Müller</w:t><w:tab/><w:t>&#228;</w:t></w:r></w:p>\
             <w:p><w:pPr><w:tabs><w:tab w:val=\"left\"/></w:tabs></w:pPr><w:r><w:t>Ende</w:t></w:r></w:p>",
        );
        let path = write(&dir, "letter.docx", &bytes);
        assert_eq!(
            text_of(OfficeText.extract(&path)),
            "Rechnung für Company A\nGrüße & Müller\tä\nEnde"
        );
    }

    #[test]
    fn an_odt_yields_paragraphs_as_lines_and_keeps_umlauts() {
        let dir = TempDir::new().unwrap();
        let bytes = odt("<text:h>Übersicht</text:h>\
             <text:p>Rechnung<text:s/>für <text:span>Company A</text:span></text:p>\
             <text:p>Zeile<text:line-break/>zwei</text:p>");
        let path = write(&dir, "letter.ODT", &bytes);
        assert_eq!(
            text_of(OfficeText.extract(&path)),
            "Übersicht\nRechnung für Company A\nZeile\nzwei"
        );
    }

    #[test]
    fn a_truncated_docx_is_an_error() {
        let dir = TempDir::new().unwrap();
        let bytes = docx("<w:p><w:r><w:t>Company A</w:t></w:r></w:p>");
        let path = write(&dir, "cut.docx", &bytes[..bytes.len().div_euclid(2)]);
        assert!(matches!(
            OfficeText.extract(&path),
            Err(TextError::Unreadable { .. })
        ));
    }

    #[test]
    fn a_docx_without_the_content_entry_is_an_error() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "other.docx", &package("word/other.xml", "<a/>"));
        let err = OfficeText.extract(&path).unwrap_err();
        assert!(err.to_string().contains("word/document.xml"), "{err}");
    }

    #[test]
    fn a_file_that_is_no_package_is_an_error() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "fake.odt", b"not a zip");
        OfficeText.extract(&path).unwrap_err();
    }

    #[test]
    fn broken_xml_in_the_package_is_an_error() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "broken.docx",
            &package("word/document.xml", "<w:p><w:t>Company A</w:p></w:t>"),
        );
        OfficeText.extract(&path).unwrap_err();
    }

    #[test]
    fn an_entry_above_the_size_limit_stops_extraction() {
        let dir = TempDir::new().unwrap();
        let bytes = docx(&format!(
            "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
            "A".repeat(10_000)
        ));
        let path = write(&dir, "big.docx", &bytes);
        let limits = Limits {
            text_bytes: MAX_TEXT_BYTES,
            entry_bytes: 1_000,
        };
        let err = OfficeText::extract_within(&path, Package::Docx, limits).unwrap_err();
        assert!(err.to_string().contains("larger than 1000"), "{err}");
    }

    #[test]
    fn long_text_in_a_package_is_cut_at_the_text_limit() {
        let dir = TempDir::new().unwrap();
        let bytes = docx(&format!(
            "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
            "ä".repeat(1_000)
        ));
        let path = write(&dir, "long.docx", &bytes);
        let limits = Limits {
            text_bytes: 101,
            entry_bytes: MAX_ENTRY_BYTES,
        };
        let text = text_of(OfficeText::extract_within(&path, Package::Docx, limits));
        assert_eq!(text, "ä".repeat(50));
    }
}
