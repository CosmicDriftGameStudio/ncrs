//! The hint in the status bar that the drive of the home folder is nearly
//! full: when to show it, and the stamp that keeps it to once a day.

use std::path::{Path, PathBuf};

use ncrs_config::space::{warns_for_drive, SpaceSettings};
use ncrs_os_probe::VolumeSpace;

pub const THROTTLE_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskHint {
    pub used_percent: u8,
    pub available: u64,
}

/// The hint for `volume`, when it is used up to `warn_from` percent or more.
pub fn disk_hint(volume: VolumeSpace, warn_from: u8) -> Option<DiskHint> {
    if volume.total == 0 {
        return None;
    }
    let used = volume.total.saturating_sub(volume.available);
    let percent =
        u8::try_from((u128::from(used) * 100).div_euclid(u128::from(volume.total))).unwrap_or(100);
    (percent >= warn_from).then_some(DiskHint {
        used_percent: percent,
        available: volume.available,
    })
}

/// The hint for the drive of the home folder, called `home_drive_name`: none when the user
/// turned the warning off for it, else [`disk_hint`] at the warn level of the settings.
pub fn home_disk_hint(
    volume: VolumeSpace,
    settings: &SpaceSettings,
    home_drive_name: &str,
) -> Option<DiskHint> {
    if !warns_for_drive(settings, home_drive_name, true) {
        return None;
    }
    disk_hint(volume, settings.warn_from_percent)
}

/// The space of the volume the home folder lies on; `None` when the system
/// does not say. One call of the system for this path, not the list of drives,
/// so a dead network mount cannot hang it.
pub fn measure_home(home: &Path) -> Option<VolumeSpace> {
    ncrs_os_probe::volume_space(home).ok()
}

/// When the hint was last shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HintStamp {
    pub last_shown_unix: Option<i64>,
}

impl HintStamp {
    /// At most once per 24 hours. A stamp in the future (the clock was set
    /// back) does not block the hint for good.
    pub fn is_due(&self, now_unix: i64) -> bool {
        self.last_shown_unix
            .is_none_or(|last| now_unix < last || now_unix - last >= THROTTLE_SECONDS)
    }
}

/// The file of the stamp, next to the session state file the app already knows.
pub fn stamp_file_beside(session_state_path: &Path) -> PathBuf {
    session_state_path.with_file_name("space-hint")
}

/// A missing or damaged file is a first run.
pub fn load_stamp(path: &Path) -> HintStamp {
    let last_shown_unix = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok());
    HintStamp { last_shown_unix }
}

/// Remembers that the hint is shown at `now_unix`. A failed write only means
/// the hint may come again.
pub fn save_stamp(path: &Path, now_unix: i64) {
    if let Some(folder) = path.parent() {
        if std::fs::create_dir_all(folder).is_err() {
            return;
        }
    }
    if let Err(err) = std::fs::write(path, now_unix.to_string()) {
        eprintln!("ncrs: space hint stamp {} not saved: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;

    fn volume(total: u64, available: u64) -> VolumeSpace {
        VolumeSpace { total, available }
    }

    #[test]
    fn the_hint_starts_at_the_threshold() {
        assert_eq!(disk_hint(volume(100 * GB, 11 * GB), 90), None);
        let at = disk_hint(volume(100 * GB, 10 * GB), 90).unwrap();
        assert_eq!(at.used_percent, 90);
        assert_eq!(at.available, 10 * GB);
        assert_eq!(
            disk_hint(volume(100 * GB, 0), 99).unwrap().used_percent,
            100
        );
    }

    #[test]
    fn the_threshold_is_the_users_not_a_fixed_one() {
        assert!(disk_hint(volume(100 * GB, 40 * GB), 90).is_none());
        assert!(disk_hint(volume(100 * GB, 40 * GB), 60).is_some());
    }

    #[test]
    fn no_hint_for_the_home_drive_when_its_warning_is_off() {
        let full = volume(100 * GB, GB);
        let mut settings = SpaceSettings::default();
        assert!(home_disk_hint(full, &settings, "Macintosh HD").is_some());
        settings.warn_drives = Some(Vec::new());
        assert!(home_disk_hint(full, &settings, "Macintosh HD").is_none());
        settings.warn_drives = Some(vec!["Macintosh HD".to_owned()]);
        assert!(home_disk_hint(full, &settings, "Macintosh HD").is_some());
    }

    #[test]
    fn a_volume_of_no_size_gives_no_hint() {
        assert_eq!(disk_hint(volume(0, 0), 50), None);
    }

    #[test]
    fn the_hint_is_due_once_a_day() {
        let day = THROTTLE_SECONDS;
        let never = HintStamp::default();
        assert!(never.is_due(1_000));
        let shown = HintStamp {
            last_shown_unix: Some(1_000),
        };
        assert!(!shown.is_due(1_000));
        assert!(!shown.is_due(1_000 + day - 1));
        assert!(shown.is_due(1_000 + day));
    }

    #[test]
    fn a_clock_set_back_counts_as_due() {
        let shown = HintStamp {
            last_shown_unix: Some(5_000_000),
        };
        assert!(shown.is_due(4_999_999));
    }

    #[test]
    fn the_stamp_survives_a_restart_and_a_damaged_file_is_a_first_run() {
        let folder = tempfile::tempdir().unwrap();
        let session = folder.path().join("session.toml");
        let file = stamp_file_beside(&session);
        assert_eq!(load_stamp(&file), HintStamp::default());
        save_stamp(&file, 42);
        assert_eq!(load_stamp(&file).last_shown_unix, Some(42));
        std::fs::write(&file, "not a number").unwrap();
        assert_eq!(load_stamp(&file), HintStamp::default());
    }
}
