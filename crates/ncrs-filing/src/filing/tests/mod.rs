use std::fs;

use ncrs_index::{InMemoryKeyStore, KeyStoreError};
use ncrs_intake::LocationMode;
use tempfile::TempDir;

use super::index_ops::{index_files, put_index};
use super::update::ask_recovery_key;

use super::*;

fn fixed_group(_count: usize) -> usize {
    4
}

fn location(path: &Path) -> FilingLocation {
    FilingLocation {
        path: path.to_path_buf(),
        mode: LocationMode::Inbox,
        images: false,
        is_target: false,
    }
}

fn filing_with(locations: Vec<FilingLocation>) -> Filing {
    Filing {
        enabled: true,
        locations,
        load: Load::default(),
        ai: ncrs_config::config::AiSetting::Off,
        default_pattern: None,
        own_names: Vec::new(),
        own_ibans: Vec::new(),
        own_tax_ids: Vec::new(),
    }
}

fn data_dir(dir: &TempDir) -> PathBuf {
    dir.path().join("data")
}

fn db_in(dir: &TempDir) -> PathBuf {
    data_dir(dir).join("index.db")
}

fn shared(store: &Arc<InMemoryKeyStore>) -> SharedStore {
    Arc::<InMemoryKeyStore>::clone(store)
}

/// Runs the work and feeds every result back, as the app does.
fn settle(module: &mut FilingModule, mut work: Option<Work>) {
    while let Some(next) = work {
        work = module.update(next.run());
    }
}

fn started(db: &Path, store: SharedStore) -> FilingModule {
    let (mut module, work) = FilingModule::start_picking(
        &filing_with(Vec::new()),
        Some(db.to_path_buf()),
        store,
        fixed_group,
    )
    .unwrap();
    settle(&mut module, work);
    module
}

fn send(module: &mut FilingModule, message: FilingMessage) {
    let work = module.update(message);
    settle(module, work);
}

fn type_key(module: &mut FilingModule, text: &str) {
    send(module, FilingMessage::KeyInput(SecretText::from(text)));
}

fn make_encrypted(db: &Path, store: &InMemoryKeyStore) -> String {
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    let (index, key) = Index::create_encrypted(db, store).unwrap();
    drop(index);
    key.to_string()
}

fn make_plain(db: &Path) {
    fs::create_dir_all(db.parent().unwrap()).unwrap();
    drop(Index::create_plain(db).unwrap());
}

fn wrong_key_text() -> String {
    "A".repeat(52)
}

fn is_open(module: &FilingModule) -> bool {
    module.start_pipeline().is_ok()
}

#[derive(Debug)]
struct UnavailableStore;

impl KeyStore for UnavailableStore {
    fn load(&self) -> Result<Option<Vec<u8>>, KeyStoreError> {
        Err(KeyStoreError::Unavailable("no keystore".to_owned()))
    }

    fn store_new(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("no keystore".to_owned()))
    }

    fn replace(&self, _key: &[u8]) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::Unavailable("no keystore".to_owned()))
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        Ok(())
    }
}

mod access;
mod protection;
mod start;
