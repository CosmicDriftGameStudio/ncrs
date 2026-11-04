//! The scheduling priority of the calling thread. The only crate of the
//! workspace that makes system calls for it, so the `unsafe` stays in one place.
//! Best effort: a system that refuses leaves the thread as it was.

/// How much the calling thread yields to what the user is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPriority {
    /// Indexing in the background: on macOS the efficiency cores, throttled
    /// while the user works.
    Background,
    /// Work the user does not wait for, but that should not crawl.
    Utility,
    /// The user waits for the result.
    UserInitiated,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as system;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as system;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as system;

/// Sets the priority of the calling thread. Errors are ignored; on a system
/// without an implementation this does nothing.
pub fn set_current_thread_priority(priority: ThreadPriority) {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    system::set(priority);
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let _ = priority;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    #[cfg(target_os = "macos")]
    mod qos {
        use std::thread;

        use crate::{set_current_thread_priority, ThreadPriority};

        #[allow(unsafe_code)]
        fn qos_of_this_thread() -> libc::qos_class_t {
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            // SAFETY: `pthread_self` takes no arguments and cannot fail.
            let this_thread = unsafe { libc::pthread_self() };
            // SAFETY: `this_thread` is the calling thread, and both out
            // pointers refer to live locals for the duration of the call.
            let status =
                unsafe { libc::pthread_get_qos_class_np(this_thread, &mut class, &mut relative) };
            assert_eq!(status, 0, "the QoS class of the thread could not be read");
            class
        }

        #[test]
        fn background_priority_puts_the_thread_in_the_background_qos_class() {
            let class = thread::spawn(|| {
                set_current_thread_priority(ThreadPriority::Background);
                qos_of_this_thread()
            })
            .join()
            .unwrap();
            assert!(
                matches!(class, libc::qos_class_t::QOS_CLASS_BACKGROUND),
                "the thread is not in the background class"
            );
        }

        #[test]
        fn the_priority_can_be_raised_again() {
            let class = thread::spawn(|| {
                set_current_thread_priority(ThreadPriority::Background);
                set_current_thread_priority(ThreadPriority::UserInitiated);
                qos_of_this_thread()
            })
            .join()
            .unwrap();
            assert!(
                matches!(class, libc::qos_class_t::QOS_CLASS_USER_INITIATED),
                "the thread did not get back to the user initiated class"
            );
        }
    }
}
