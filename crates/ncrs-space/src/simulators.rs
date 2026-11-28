//! The simulators `simctl` lists, by availability, measured on the disk. Only on macOS
//! outside the App Store build; elsewhere there is nothing to find.

use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    Unavailable,
}

/// The size of the devices of `which`, not of the whole devices folder. `None` when stopped.
#[cfg(all(target_os = "macos", not(feature = "sandbox")))]
pub fn device_bytes(devices: &Path, which: Availability, stop: &AtomicBool) -> Option<u64> {
    let word = match which {
        Availability::Available => "available",
        Availability::Unavailable => "unavailable",
    };
    let listed = std::process::Command::new("xcrun")
        .args(["simctl", "list", "devices", word])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let mut total: u64 = 0;
    for id in listed.lines().filter_map(device_id_of) {
        total = total.saturating_add(ncrs_fs::measure_folder(&devices.join(id), stop)?.bytes);
    }
    Some(total)
}

#[cfg(not(all(target_os = "macos", not(feature = "sandbox"))))]
pub fn device_bytes(_devices: &Path, _which: Availability, _stop: &AtomicBool) -> Option<u64> {
    Some(0)
}

/// The UUID in parentheses of a `simctl list` line.
#[cfg(any(all(target_os = "macos", not(feature = "sandbox")), test))]
pub fn device_id_of(line: &str) -> Option<&str> {
    line.split(['(', ')'])
        .map(str::trim)
        .find(|part| part.len() == 36 && part.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uuid_of_a_device_line_is_found() {
        let line = "    iPhone 15 (3F2B9C1A-0D4E-4B7A-9C55-AB12CD34EF56) (Shutdown) ";
        assert_eq!(
            device_id_of(line),
            Some("3F2B9C1A-0D4E-4B7A-9C55-AB12CD34EF56")
        );
        assert_eq!(device_id_of("-- iOS 17.0 --"), None);
    }
}
