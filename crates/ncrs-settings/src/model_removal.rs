//! Deleting a downloaded model from this computer. Only the files the
//! download of that model wrote are removed, never the folder around them.

use std::io::ErrorKind;
use std::path::Path;

use ncrs_ai::LOCAL_MODEL;
use ncrs_intake::OCR_MODEL_FILES;

use crate::facts::ModelKind;

/// Blocking. A file that is not there is already gone.
pub fn remove_model_files(kind: ModelKind, dir: &Path) -> Result<(), String> {
    let names: Vec<&str> = match kind {
        ModelKind::Ocr => OCR_MODEL_FILES.iter().map(|file| file.name).collect(),
        ModelKind::Ai => vec![LOCAL_MODEL.name],
    };
    for name in names {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn only_the_files_of_the_model_go() -> TestResult {
        let dir = tempfile::tempdir()?;
        let [detection, recognition] = &OCR_MODEL_FILES;
        for name in [detection.name, recognition.name, "other.txt"] {
            std::fs::write(dir.path().join(name), b"x")?;
        }

        assert_eq!(remove_model_files(ModelKind::Ocr, dir.path()), Ok(()));

        let mut left = Vec::new();
        for entry in std::fs::read_dir(dir.path())? {
            left.push(entry?.file_name().to_string_lossy().into_owned());
        }
        assert_eq!(left, vec!["other.txt".to_owned()]);
        Ok(())
    }

    #[test]
    fn a_model_that_is_not_there_is_removed_without_error() -> TestResult {
        let dir = tempfile::tempdir()?;
        assert_eq!(remove_model_files(ModelKind::Ai, dir.path()), Ok(()));
        Ok(())
    }
}
