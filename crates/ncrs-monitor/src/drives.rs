//! The drives are measured apart from everything else: a network drive that
//! went away lets `statfs` hang for as long as the kernel retries. While one
//! measurement has not come back, no other is started, so the blocking pool
//! does not fill up with threads stuck in it, and the rest of the page keeps
//! running.

use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;

use crate::gate::{Gate, GateGuard};
use crate::snapshot::DriveRow;

/// What measures the drives; the real one asks the system, tests bring their
/// own.
pub trait DriveProbe: Send + Sync + Debug {
    fn measure(&self) -> Vec<DriveRow>;
}

#[derive(Debug)]
pub struct DriveMeasurer {
    probe: Arc<dyn DriveProbe>,
    gate: Gate,
}

impl DriveMeasurer {
    pub fn new(probe: Arc<dyn DriveProbe>) -> Self {
        Self {
            probe,
            gate: Gate::default(),
        }
    }

    /// A measurement to run on a blocking thread, or `None` while the last
    /// one has not finished.
    pub fn start(&self) -> Option<DriveRun> {
        Some(DriveRun {
            probe: Arc::clone(&self.probe),
            guard: self.gate.enter()?,
        })
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.gate.is_running()
    }
}

#[derive(Debug)]
pub struct DriveRun {
    probe: Arc<dyn DriveProbe>,
    guard: GateGuard,
}

impl DriveRun {
    /// Blocks for as long as the system takes. The gate opens when this
    /// returns, or when the run is dropped without having been run.
    pub fn run(self) -> Vec<DriveRow> {
        let rows = self.probe.measure();
        drop(self.guard);
        rows
    }
}

const PSEUDO_FILE_SYSTEMS: [&str; 22] = [
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "tmpfs",
    "ramfs",
    "cgroup",
    "cgroup2",
    "squashfs",
    "overlay",
    "efivarfs",
    "securityfs",
    "debugfs",
    "tracefs",
    "configfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "pstore",
    "bpf",
    "autofs",
    "binfmt_misc",
];

const SYSTEM_MOUNT_PREFIXES: [&str; 7] = [
    "/System/Volumes",
    "/Library/Developer/CoreSimulator",
    "/boot",
    "/snap",
    "/sys",
    "/proc",
    "/dev",
];

/// A volume the system keeps for itself: a macOS helper volume, a Linux
/// pseudo file system, a partition without a mount point.
pub fn is_system_volume(mount: &Path, file_system: &str) -> bool {
    mount.as_os_str().is_empty()
        || PSEUDO_FILE_SYSTEMS.contains(&file_system)
        || SYSTEM_MOUNT_PREFIXES
            .iter()
            .any(|prefix| mount.starts_with(prefix))
        || (mount.starts_with("/run") && !mount.starts_with("/run/media"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct BlockingProbe {
        started: AtomicUsize,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl DriveProbe for BlockingProbe {
        fn measure(&self) -> Vec<DriveRow> {
            self.started.fetch_add(1, Ordering::SeqCst);
            let _ = self.release.lock().unwrap().recv();
            Vec::new()
        }
    }

    #[test]
    fn a_measurement_that_hangs_is_started_only_once() {
        let (release, wait) = mpsc::channel();
        let probe = Arc::new(BlockingProbe {
            started: AtomicUsize::new(0),
            release: Mutex::new(wait),
        });
        let measurer = DriveMeasurer::new(Arc::<BlockingProbe>::clone(&probe));

        let run = measurer.start().expect("the first one starts");
        let blocked = std::thread::spawn(move || run.run());
        for _ in 0..5 {
            assert!(measurer.start().is_none(), "no second one while it hangs");
        }
        assert!(measurer.is_running());

        release.send(()).unwrap();
        blocked.join().unwrap();
        assert_eq!(probe.started.load(Ordering::SeqCst), 1);
        assert!(
            measurer.start().is_some(),
            "it starts again when it came back"
        );
    }

    #[test]
    fn a_run_that_is_dropped_opens_the_gate_again() {
        let (_release, wait) = mpsc::channel();
        let measurer = DriveMeasurer::new(Arc::new(BlockingProbe {
            started: AtomicUsize::new(0),
            release: Mutex::new(wait),
        }));
        drop(measurer.start());
        assert!(measurer.start().is_some());
    }

    #[test]
    fn system_volumes_are_told_from_the_ones_a_user_knows() {
        for system in [
            ("/System/Volumes/VM", "apfs"),
            ("/dev/shm", "tmpfs"),
            ("/run/user/1000", "tmpfs"),
            ("/boot/efi", "vfat"),
            ("", "ntfs"),
        ] {
            assert!(
                is_system_volume(Path::new(system.0), system.1),
                "{system:?}"
            );
        }
        for user in [
            ("/", "apfs"),
            ("/Volumes/Backup", "apfs"),
            ("/home", "ext4"),
            ("/run/media/marc/stick", "vfat"),
            ("C:\\", "NTFS"),
        ] {
            assert!(!is_system_volume(Path::new(user.0), user.1), "{user:?}");
        }
    }
}
