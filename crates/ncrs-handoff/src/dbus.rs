//! `org.freedesktop.FileManager1`: what browsers, IDEs and download dialogs
//! call for "Show in folder". The calls go to the same inbox the second start
//! uses, so the running ncrs opens them the same way.

use std::path::PathBuf;

use crate::{requests_from_uris, send};

const NAME: &str = "org.freedesktop.FileManager1";
const OBJECT_PATH: &str = "/org/freedesktop/FileManager1";

struct FileManager {
    data_dir: PathBuf,
}

impl FileManager {
    fn hand_over(&self, uris: &[String], reveal: bool) -> zbus::fdo::Result<()> {
        let requests = requests_from_uris(uris, reveal);
        if requests.is_empty() {
            return Ok(());
        }
        send(&self.data_dir, &requests).map_err(|error| {
            zbus::fdo::Error::Failed(format!("the inbox is not writable: {error}"))
        })
    }
}

#[zbus::interface(name = "org.freedesktop.FileManager1")]
impl FileManager {
    fn show_folders(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.hand_over(&uris, false)
    }

    fn show_items(&self, uris: Vec<String>, _startup_id: String) -> zbus::fdo::Result<()> {
        self.hand_over(&uris, true)
    }

    fn show_item_properties(
        &self,
        uris: Vec<String>,
        _startup_id: String,
    ) -> zbus::fdo::Result<()> {
        self.hand_over(&uris, true)
    }
}

/// Offers the service on the session bus from a thread of its own. The name is
/// only taken when it is free: another file manager that owns it, or no session
/// bus, leaves everything as it was.
pub fn serve_file_manager(data_dir: PathBuf) {
    drop(
        std::thread::Builder::new()
            .name("ncrs-file-manager-service".to_owned())
            .spawn(move || {
                let Ok(connection) = connect(data_dir) else {
                    return;
                };
                // The connection answers calls on its own executor; it only
                // has to stay alive for as long as the process runs.
                Box::leak(Box::new(connection));
            }),
    );
}

fn connect(data_dir: PathBuf) -> zbus::Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::session()?
        .name(NAME)?
        .serve_at(OBJECT_PATH, FileManager { data_dir })?
        .build()
}
