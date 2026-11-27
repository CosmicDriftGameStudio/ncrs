use std::io;
use std::path::Path;

use crate::VolumeSpace;

/// Space of the volume `path` lives on. Stats only that path, never the mount list.
pub fn volume_space(path: &Path) -> io::Result<VolumeSpace> {
    platform_volume_space(path)
}

#[cfg(unix)]
fn platform_volume_space(path: &Path) -> io::Result<VolumeSpace> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;

    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    let stat = statvfs_of(&c_path)?;
    let unit = stat.f_frsize as u64;
    Ok(VolumeSpace {
        total: (stat.f_blocks as u64).saturating_mul(unit),
        available: (stat.f_bavail as u64).saturating_mul(unit),
    })
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn statvfs_of(path: &std::ffi::CStr) -> io::Result<libc::statvfs> {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is NUL-terminated and `stat` points to writable memory
    // for one `statvfs`, both alive for the call.
    let status = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call returned 0, so it filled the whole struct (which was
    // zeroed to begin with, a valid value for these integer fields).
    Ok(unsafe { stat.assume_init() })
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn platform_volume_space(path: &Path) -> io::Result<VolumeSpace> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0_u64;
    let mut total = 0_u64;
    // SAFETY: `wide` is NUL-terminated and alive for the call; the two out
    // pointers point to live `u64`s and the third (total free) may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(VolumeSpace { total, available })
}

#[cfg(not(any(unix, windows)))]
fn platform_volume_space(_path: &Path) -> io::Result<VolumeSpace> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no volume space on this platform",
    ))
}
