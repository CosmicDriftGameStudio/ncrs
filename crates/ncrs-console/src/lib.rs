//! The Windows build is a GUI program, so it starts without a console. A
//! command line run (`--version`, `--help`, `--check-ai`) still has to print
//! to the console it was started from. The only crate that makes system calls
//! for it, so the `unsafe` stays in one place.

/// Connects the output of the process to the console of its parent, if it has
/// one. A stream the caller redirected (a pipe, a file) is left as it is. Does
/// nothing off Windows and when the program was not started from a console.
pub fn attach_to_parent_console() {
    #[cfg(windows)]
    windows::attach();
}

#[cfg(windows)]
mod windows {
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_HANDLE, STD_OUTPUT_HANDLE,
    };

    pub(super) fn attach() {
        // SAFETY: `AttachConsole` takes no pointers; a refusal (no parent
        // console) is reported by its return value.
        #[allow(unsafe_code)]
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0;
        if !attached {
            return;
        }
        for stream in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if !has_target(stream) {
                point_at_console(stream);
            }
        }
    }

    /// A GUI process gets no standard handles unless the parent redirected
    /// them, so a missing one means the stream should go to the console.
    #[allow(unsafe_code)]
    fn has_target(stream: STD_HANDLE) -> bool {
        // SAFETY: `GetStdHandle` takes no pointers and only reads the table of
        // standard handles of this process.
        let handle = unsafe { GetStdHandle(stream) };
        !handle.is_null() && handle != INVALID_HANDLE_VALUE
    }

    #[allow(unsafe_code)]
    fn point_at_console(stream: STD_HANDLE) {
        let Ok(console) = OpenOptions::new().read(true).write(true).open("CONOUT$") else {
            return;
        };
        // Read access too: the standard library asks the console for its mode
        // before it writes, and that needs it. The handle must stay open for
        // the life of the process.
        let handle = console.into_raw_handle();
        // SAFETY: `handle` is a valid, open console handle that nothing else
        // owns now; a refusal is reported by the return value, which is
        // ignored on purpose.
        let _refused = unsafe { SetStdHandle(stream, handle) };
    }
}
