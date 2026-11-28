use crate::Holder;

/// `None` when the processes cannot be listed (the App Store build).
#[cfg(feature = "sandbox")]
pub fn running_processes() -> Option<Vec<Holder>> {
    None
}

/// `None` when the processes cannot be listed (the App Store build).
#[cfg(not(feature = "sandbox"))]
pub fn running_processes() -> Option<Vec<Holder>> {
    use sysinfo::{ProcessesToUpdate, System};

    let mut system = System::new().ok()?;
    system.refresh_processes(ProcessesToUpdate::All, true);
    let mut found: Vec<Holder> = system
        .processes()
        .iter()
        .map(|(pid, process)| Holder {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().into_owned(),
        })
        .collect();
    found.sort_by_key(|holder| holder.pid);
    Some(found)
}
