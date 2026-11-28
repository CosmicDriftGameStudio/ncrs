//! Restart Manager: it names the processes that hold given files open. It
//! works on files, not folders, so the files under a folder are registered in
//! batches in one session; the caller's time limit is the only bound.

use std::fs;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
    RM_PROCESS_INFO,
};

use crate::{failed, FolderUse, Holder, UnknownWhy};

const BATCH: usize = 1000;

/// What went wrong while collecting the holders.
enum Trouble {
    Disk(std::io::Error),
    Manager(u32),
}

pub fn folder_use(folder: &Path) -> FolderUse {
    match holders_under(folder) {
        Ok(held) if held.is_empty() => FolderUse::Free,
        Ok(held) => FolderUse::Held(held),
        Err(Trouble::Disk(err)) => failed(&err),
        Err(Trouble::Manager(code)) => FolderUse::Unknown(UnknownWhy::Failed(format!(
            "Restart Manager answered {code}"
        ))),
    }
}

/// The processes that hold the file, or any file below the folder, open.
fn holders_under(path: &Path) -> Result<Vec<Holder>, Trouble> {
    let metadata = fs::symlink_metadata(path).map_err(Trouble::Disk)?;
    let session = Session::start().map_err(Trouble::Manager)?;
    if metadata.is_file() {
        register_files(&session, &[path.to_path_buf()]).map_err(Trouble::Manager)?;
    } else {
        let mut batch: Vec<PathBuf> = Vec::new();
        let mut pending = vec![path.to_path_buf()];
        while let Some(next) = pending.pop() {
            for read in fs::read_dir(&next).map_err(Trouble::Disk)? {
                let entry = read.map_err(Trouble::Disk)?;
                if entry.file_type().map_err(Trouble::Disk)?.is_dir() {
                    pending.push(entry.path());
                } else if entry.metadata().map_err(Trouble::Disk)?.is_file() {
                    batch.push(entry.path());
                    if batch.len() == BATCH {
                        register_files(&session, &batch).map_err(Trouble::Manager)?;
                        batch.clear();
                    }
                }
            }
        }
        if !batch.is_empty() {
            register_files(&session, &batch).map_err(Trouble::Manager)?;
        }
    }
    let mut held: Vec<Holder> = session
        .list()
        .map_err(Trouble::Manager)?
        .into_iter()
        .map(|info| Holder {
            pid: info.Process.dwProcessId,
            name: name_of(&info.strAppName),
        })
        .collect();
    held.sort_by_key(|holder| holder.pid);
    held.dedup_by_key(|holder| holder.pid);
    Ok(held)
}

fn register_files(session: &Session, files: &[PathBuf]) -> Result<(), u32> {
    let wide: Vec<Vec<u16>> = files
        .iter()
        .map(|file| file.as_os_str().encode_wide().chain(Some(0)).collect())
        .collect();
    let pointers: Vec<*const u16> = wide.iter().map(|name| name.as_ptr()).collect();
    session.register(&pointers)
}

/// The serial number of the volume the path lies on, the Windows counterpart of the unix
/// device number. `None` when the path cannot be opened.
#[allow(unsafe_code)]
pub fn volume_serial_of(path: &Path) -> Option<u64> {
    let file = fs::OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid plain-data value.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle belongs to `file`, which lives past the call, and `info` is a
    // live struct the call fills in.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    (ok != 0).then_some(u64::from(info.dwVolumeSerialNumber))
}

fn name_of(units: &[u16]) -> String {
    let length = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    String::from_utf16_lossy(units.get(..length).unwrap_or_default())
}

#[derive(Debug)]
struct Session(u32);

impl Session {
    #[allow(unsafe_code)]
    fn start() -> Result<Self, u32> {
        let mut handle: u32 = 0;
        let mut key = [0; CCH_RM_SESSION_KEY as usize + 1];
        // SAFETY: `handle` is a live `u32` and `key` has the room of
        // CCH_RM_SESSION_KEY characters plus the NUL the call writes.
        let code = unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) };
        if code == ERROR_SUCCESS {
            Ok(Self(handle))
        } else {
            Err(code)
        }
    }

    #[allow(unsafe_code)]
    fn register(&self, files: &[*const u16]) -> Result<(), u32> {
        let count = u32::try_from(files.len()).map_err(|_too_many| ERROR_MORE_DATA)?;
        // SAFETY: `files` holds `count` pointers to NUL-terminated names that
        // the caller keeps alive for the call; no applications or services are
        // passed, with null pointers and zero counts.
        let code = unsafe {
            RmRegisterResources(
                self.0,
                count,
                files.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
        };
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(code)
        }
    }

    #[allow(unsafe_code)]
    fn list(&self) -> Result<Vec<RM_PROCESS_INFO>, u32> {
        let mut needed: u32 = 0;
        let mut have: u32 = 0;
        let mut reasons: u32 = 0;
        // SAFETY: with a null array and `have` 0 the call only reports the
        // count it needs; the out pointers point to live `u32`s.
        let code = unsafe {
            RmGetList(
                self.0,
                &mut needed,
                &mut have,
                std::ptr::null_mut(),
                &mut reasons,
            )
        };
        if code == ERROR_SUCCESS {
            return Ok(Vec::new());
        }
        if code != ERROR_MORE_DATA {
            return Err(code);
        }
        // Processes may join between the two calls.
        let mut infos = vec![RM_PROCESS_INFO::default(); needed as usize + 8];
        have = u32::try_from(infos.len()).map_err(|_too_many| ERROR_MORE_DATA)?;
        // SAFETY: `infos` has `have` elements, which the call fills in part
        // and reports how many in `have`; the other pointers are live `u32`s.
        let second_code = unsafe {
            RmGetList(
                self.0,
                &mut needed,
                &mut have,
                infos.as_mut_ptr(),
                &mut reasons,
            )
        };
        if second_code != ERROR_SUCCESS {
            return Err(second_code);
        }
        infos.truncate(have as usize);
        Ok(infos)
    }
}

impl Drop for Session {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful `RmStartSession` and is
        // closed exactly once, here.
        let _closed = unsafe { RmEndSession(self.0) };
    }
}
