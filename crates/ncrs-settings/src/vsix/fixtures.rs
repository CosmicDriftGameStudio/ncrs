use std::io::Write as _;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;

/// A package with the given entries, each named below `extension/`.
pub fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in entries {
        writer
            .start_file(format!("extension/{name}"), options)
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// The manifest of an extension with these themes: label, uiTheme, path.
pub fn manifest(themes: &[(&str, &str, &str)]) -> String {
    let listed: Vec<String> = themes
        .iter()
        .map(|(label, ui, path)| {
            format!(r#"{{"label": {label:?}, "uiTheme": {ui:?}, "path": {path:?}}}"#)
        })
        .collect();
    format!(r#"{{"contributes": {{"themes": [{}]}}}}"#, listed.join(","))
}

pub const THEME_JSON: &str = r##"{"colors": {"editor.background": "#102030"}}"##;
