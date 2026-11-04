use crate::ThreadPriority;

pub fn set(priority: ThreadPriority) {
    let class = match priority {
        ThreadPriority::Background => libc::qos_class_t::QOS_CLASS_BACKGROUND,
        ThreadPriority::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
        ThreadPriority::UserInitiated => libc::qos_class_t::QOS_CLASS_USER_INITIATED,
    };
    set_class(class);
}

#[allow(unsafe_code)]
fn set_class(class: libc::qos_class_t) {
    // SAFETY: the call only changes the QoS class of the calling thread; it
    // takes no pointers, and a refusal is reported by its return value, which
    // is ignored on purpose.
    let _refused = unsafe { libc::pthread_set_qos_class_self_np(class, 0) };
}
