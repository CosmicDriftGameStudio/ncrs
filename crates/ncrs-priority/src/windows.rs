use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_LOWEST,
    THREAD_PRIORITY_NORMAL,
};

use crate::ThreadPriority;

pub fn set(priority: ThreadPriority) {
    let level = match priority {
        ThreadPriority::Background => THREAD_PRIORITY_LOWEST,
        ThreadPriority::Utility => THREAD_PRIORITY_BELOW_NORMAL,
        ThreadPriority::UserInitiated => THREAD_PRIORITY_NORMAL,
    };
    set_level(level);
}

#[allow(unsafe_code)]
fn set_level(level: i32) {
    // SAFETY: `GetCurrentThread` returns a pseudo handle for the calling
    // thread that needs no closing, and `SetThreadPriority` only changes that
    // thread; a refusal is reported by its return value, which is ignored on
    // purpose.
    let _refused = unsafe { SetThreadPriority(GetCurrentThread(), level) };
}
