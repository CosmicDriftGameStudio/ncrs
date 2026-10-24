//! Helpers for tests that need a rendered PDF page, here and in crates that
//! recognise text in page images (feature `test-support`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::text::{PageImage, PdfText};

const LIBRARY_DIR_VARIABLE: &str = "NCRS_PDFIUM_DIR";

fn library_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(LIBRARY_DIR_VARIABLE) {
        return PathBuf::from(dir);
    }
    let sub = if cfg!(windows) { "bin" } else { "lib" };
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/pdfium")
        .join(sub)
}

/// Every page of the PDF as the OCR sees a scan, text layer or not.
pub fn rendered_pages(pdf: &PdfText, path: &Path) -> Vec<PageImage> {
    pdf.render_file(path).unwrap()
}

pub fn pdf_text() -> PdfText {
    let dir = library_dir();
    PdfText::load(&dir).unwrap_or_else(|err| {
        panic!(
            "{err}. Run scripts/fetch-pdfium.sh <rust-target-triple> target/pdfium, \
             or set {LIBRARY_DIR_VARIABLE} to the folder with the library"
        )
    })
}

/// A one-page PDF with the given content stream and extra page resources.
pub fn pdf_bytes(content: &str, resources: &str, extra_objects: &[String]) -> Vec<u8> {
    let mut objects = vec![
        "<</Type/Catalog/Pages 2 0 R>>".to_owned(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_owned(),
        format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Contents 4 0 R/Resources<<{resources}>>>>"
        ),
        format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
    ];
    objects.extend_from_slice(extra_objects);
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (number, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", number + 1).as_bytes());
    }
    let xref_at = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<</Root 1 0 R/Size {}>>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

pub fn text_pdf(text: &str) -> Vec<u8> {
    pdf_bytes(
        &format!("BT /F1 14 Tf 20 100 Td ({text}) Tj ET"),
        "/Font<</F1 5 0 R>>",
        &["<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_owned()],
    )
}

pub fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    path
}
