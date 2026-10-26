#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

// Its own test binary: pdfium can be bound once per process, and a load that
// finds it already bound would not touch the path this test is about. On
// Windows the canonical path carries the `\\?\` prefix, which is what ncrs
// loads the DLL from when winget starts it through a link.
#[cfg(test)]
mod tests {
    use ncrs_intake::{pdfium_library_dir, PdfText};

    #[test]
    fn the_library_loads_through_a_canonicalized_folder() {
        let dir = pdfium_library_dir().expect("no pdfium folder");
        let canonical = std::fs::canonicalize(dir).unwrap();
        PdfText::load(&canonical).unwrap_or_else(|err| {
            panic!(
                "{err}. Run scripts/fetch-pdfium.sh <rust-target-triple> target/pdfium, \
                 or set NCRS_PDFIUM_DIR to the folder with the library"
            )
        });
    }
}
