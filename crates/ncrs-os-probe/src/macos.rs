//! libproc: every process, its working directory and its open files. Buffers are
//! plain bytes or the `libc` structs; the kernel fills them and the path is read
//! from the known offset. Processes of other users refuse and are skipped.

use std::ffi::{c_int, c_void};
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use crate::{FolderUse, Holder};

/// `PROC_PIDFDVNODEPATHINFO`, which `libc` does not define.
const PROC_PIDFDVNODEPATHINFO: c_int = 2;

/// `struct proc_fileinfo`: open flags, status, offset, type, guard flags.
const PROC_FILEINFO_SIZE: usize = 24;

/// `vnode_fdinfowithpath` is a `proc_fileinfo` followed by a `vnode_info_path`;
/// the path is the last part of both.
const FD_INFO_SIZE: usize = PROC_FILEINFO_SIZE + size_of::<libc::vnode_info_path>();
const FD_PATH_OFFSET: usize = PROC_FILEINFO_SIZE + size_of::<libc::vnode_info>();
const CWD_INFO_SIZE: usize = size_of::<libc::proc_vnodepathinfo>();
const CWD_PATH_OFFSET: usize = size_of::<libc::vnode_info>();
const PATH_BYTES: usize = 1024;

pub(crate) fn folder_use(folder: &Path) -> FolderUse {
    let folder = fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let mut held = Vec::new();
    for pid in all_pids() {
        if process_holds(pid, &folder) {
            held.push(Holder {
                pid: u32::try_from(pid).unwrap_or(0),
                name: process_name(pid),
            });
        }
    }
    held.sort_by_key(|holder| holder.pid);
    if held.is_empty() {
        FolderUse::Free
    } else {
        FolderUse::Held(held)
    }
}

fn process_holds(pid: c_int, folder: &Path) -> bool {
    if cwd_of(pid).is_some_and(|cwd| cwd.starts_with(folder)) {
        return true;
    }
    open_files_of(pid)
        .into_iter()
        .any(|path| path.starts_with(folder))
}

fn path_from(buffer: &[u8], offset: usize) -> Option<PathBuf> {
    let raw = buffer.get(offset..offset + PATH_BYTES)?;
    let length = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let text = raw.get(..length).filter(|text| !text.is_empty())?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(text)))
}

fn all_pids() -> Vec<c_int> {
    let wanted = list_all_pids(None);
    if wanted <= 0 {
        return Vec::new();
    }
    // Room for processes that start between the two calls.
    let capacity = usize::try_from(wanted).unwrap_or(0) / size_of::<c_int>() + 64;
    let mut pids: Vec<c_int> = vec![0; capacity];
    let bytes = list_all_pids(Some(&mut pids));
    let count = usize::try_from(bytes).unwrap_or(0) / size_of::<c_int>();
    pids.truncate(count);
    pids.retain(|pid| *pid > 0);
    pids
}

#[allow(unsafe_code)]
fn list_all_pids(into: Option<&mut [c_int]>) -> c_int {
    let (pointer, size) = match into {
        Some(buffer) => (
            buffer.as_mut_ptr().cast::<c_void>(),
            c_int::try_from(size_of_val(buffer)).unwrap_or(c_int::MAX),
        ),
        None => (std::ptr::null_mut(), 0),
    };
    // SAFETY: `pointer` is null with size 0 (the call then only reports the
    // size it needs) or points to `size` writable bytes of `buffer`, which
    // outlives the call.
    unsafe { libc::proc_listallpids(pointer, size) }
}

fn cwd_of(pid: c_int) -> Option<PathBuf> {
    let mut buffer = vec![0_u8; CWD_INFO_SIZE];
    let got = pid_info(pid, libc::PROC_PIDVNODEPATHINFO, &mut buffer)?;
    (got >= CWD_PATH_OFFSET + PATH_BYTES)
        .then(|| path_from(&buffer, CWD_PATH_OFFSET))
        .flatten()
}

fn open_files_of(pid: c_int) -> Vec<PathBuf> {
    let Some(wanted) = descriptor_list_size(pid) else {
        return Vec::new();
    };
    let capacity = wanted / size_of::<libc::proc_fdinfo>() + 8;
    let mut descriptors = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        capacity
    ];
    let Some(bytes) = list_descriptors(pid, &mut descriptors) else {
        return Vec::new();
    };
    descriptors.truncate(bytes / size_of::<libc::proc_fdinfo>());
    let mut paths = Vec::new();
    for descriptor in descriptors {
        if descriptor.proc_fdtype != u32::try_from(libc::PROX_FDTYPE_VNODE).unwrap_or(u32::MAX) {
            continue;
        }
        let mut buffer = vec![0_u8; FD_INFO_SIZE];
        let Some(got) = fd_info(pid, descriptor.proc_fd, &mut buffer) else {
            continue;
        };
        if got >= FD_PATH_OFFSET + PATH_BYTES {
            paths.extend(path_from(&buffer, FD_PATH_OFFSET));
        }
    }
    paths
}

#[allow(unsafe_code)]
fn pid_info(pid: c_int, flavor: c_int, buffer: &mut [u8]) -> Option<usize> {
    let size = c_int::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is `size` writable bytes that outlive the call; the
    // kernel writes at most `size` bytes and reports failure as a value <= 0.
    let got = unsafe { libc::proc_pidinfo(pid, flavor, 0, buffer.as_mut_ptr().cast::<c_void>(), size) };
    usize::try_from(got).ok().filter(|got| *got > 0)
}

#[allow(unsafe_code)]
fn descriptor_list_size(pid: c_int) -> Option<usize> {
    // SAFETY: a null buffer with size 0 only asks for the size the list needs.
    let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    usize::try_from(got).ok().filter(|got| *got > 0)
}

#[allow(unsafe_code)]
fn list_descriptors(pid: c_int, descriptors: &mut [libc::proc_fdinfo]) -> Option<usize> {
    let size = c_int::try_from(size_of_val(descriptors)).ok()?;
    // SAFETY: `descriptors` is `size` writable bytes of plain integers that
    // outlive the call; the kernel writes whole entries, at most `size` bytes.
    let got = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            descriptors.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    usize::try_from(got).ok().filter(|got| *got > 0)
}

#[allow(unsafe_code)]
fn fd_info(pid: c_int, descriptor: c_int, buffer: &mut [u8]) -> Option<usize> {
    let size = c_int::try_from(buffer.len()).ok()?;
    // SAFETY: `buffer` is `size` writable bytes that outlive the call; the
    // kernel writes at most `size` bytes and reports failure as a value <= 0.
    let got = unsafe {
        libc::proc_pidfdinfo(
            pid,
            descriptor,
            PROC_PIDFDVNODEPATHINFO,
            buffer.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    usize::try_from(got).ok().filter(|got| *got > 0)
}

#[allow(unsafe_code)]
fn process_name(pid: c_int) -> String {
    let mut buffer = vec![0_u8; 256];
    let size = u32::try_from(buffer.len()).unwrap_or(0);
    // SAFETY: `buffer` is `size` writable bytes that outlive the call; the
    // kernel NUL-terminates inside them.
    let got = unsafe { libc::proc_name(pid, buffer.as_mut_ptr().cast::<c_void>(), size) };
    let length = usize::try_from(got).unwrap_or(0).min(buffer.len());
    buffer.truncate(length);
    String::from_utf8_lossy(&buffer).into_owned()
}
