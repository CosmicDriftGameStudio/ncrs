use crate::ThreadPriority;

const BACKGROUND_NICE: libc::c_int = 19;
const UTILITY_NICE: libc::c_int = 10;
const USER_INITIATED_NICE: libc::c_int = 0;

/// Raising a thread back to a lower nice value needs rights an ordinary
/// process does not have; then the thread keeps the value it has.
pub fn set(priority: ThreadPriority) {
    let nice = match priority {
        ThreadPriority::Background => BACKGROUND_NICE,
        ThreadPriority::Utility => UTILITY_NICE,
        ThreadPriority::UserInitiated => USER_INITIATED_NICE,
    };
    set_nice(nice);
}

#[allow(unsafe_code)]
fn set_nice(nice: libc::c_int) {
    // SAFETY: `gettid` takes no arguments and cannot fail.
    let thread = unsafe { libc::gettid() };
    let who = libc::id_t::try_from(thread).unwrap_or(0);
    // SAFETY: `setpriority` with PRIO_PROCESS and a thread id changes only that
    // thread's nice value on Linux; it takes no pointers, and a refusal is
    // reported by its return value, which is ignored on purpose.
    let _refused = unsafe { libc::setpriority(libc::PRIO_PROCESS, who, nice) };
}
