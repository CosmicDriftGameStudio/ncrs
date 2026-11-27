//! Restart Manager: it names the processes that hold given files open. It
//! works on files, not folders, so the files under the folder are registered,
//! up to a cap; a bigger folder is unknown.

use std::fs;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
    RM_PROCESS_INFO,
};

use crate::{failed, FolderUse, Holder, UnknownWhy};

const MAX_FILES: usize = 2000;

pub(crate) fn folder_use(folder: &Path) -> FolderUse {
    let files = match files_under(folder) {
        Ok(Some(files)) => files,
        Ok(None) => return FolderUse::Unknown(UnknownWhy::TooManyFiles),
        Err(err) => return failed(&err),
    };
    if files.is_empty() {
        return FolderUse::Free;
    }
    match holders_of(&files) {
        Ok(held) if held.is_empty() => FolderUse::Free,
        Ok(held) => FolderUse::Held(held),
        Err(code) => FolderUse::Unknown(UnknownWhy::Failed(format!(
            "Restart Manager answered {code}"
        ))),
    }
}

/// `None` when there are more than [`MAX_FILES`].
fn files_under(folder: &Path) -> std::io::Result<Option<Vec<PathBuf>>> {
    let mut files = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                if files.len() == MAX_FILES {
                    return Ok(None);
                }
                files.push(entry.path());
            }
        }
    }
    Ok(Some(files))
}

fn holders_of(files: &[PathBuf]) -> Result<Vec<Holder>, u32> {
    let session = Session::start()?;
    let wide: Vec<Vec<u16>> = files
        .iter()
        .map(|file| file.as_os_str().encode_wide().chain(Some(0)).collect())
        .collect();
    let pointers: Vec<*const u16> = wide.iter().map(|name| name.as_ptr()).collect();
    session.register(&pointers)?;
    let mut held: Vec<Holder> = session
        .list()?
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
        let mut handle = 0_u32;
        let mut key = [0_u16; CCH_RM_SESSION_KEY as usize + 1];
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
        let count = u32::try_from(files.len()).map_err(|_| ERROR_MORE_DATA)?;
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
        let mut needed = 0_u32;
        let mut have = 0_u32;
        let mut reasons = 0_u32;
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
        have = u32::try_from(infos.len()).map_err(|_| ERROR_MORE_DATA)?;
        // SAFETY: `infos` has `have` elements, which the call fills in part
        // and reports how many in `have`; the other pointers are live `u32`s.
        let code = unsafe {
            RmGetList(
                self.0,
                &mut needed,
                &mut have,
                infos.as_mut_ptr(),
                &mut reasons,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(code);
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
