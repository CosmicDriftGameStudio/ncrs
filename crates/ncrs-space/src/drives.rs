//! The drives the Platz tab knows by name: the system drive and the mounted ones, for the
//! title of the measured drive and for choosing which drives warn when they fill up.

use std::path::{Path, PathBuf};

use ncrs_fs::Volume;
use ncrs_os_probe::VolumeSpace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// The label of the drive ("Macintosh HD"), or its letter or mount point.
    pub name: String,
    pub path: PathBuf,
    /// The drive the home folder lies on.
    pub is_home_drive: bool,
}

impl Drive {
    /// Blocking; asks the drive, which can take long for a share that went away.
    pub fn space(&self) -> Option<VolumeSpace> {
        ncrs_os_probe::volume_space(&self.path).ok()
    }
}

/// The drives that are connected now, system drive first. Cloud folders and the home folder
/// are places on a drive, not drives, and are left out. Reads names only.
pub fn drives(home: &Path) -> Vec<Drive> {
    drives_of(ncrs_fs::list_volumes(), home)
}

/// The label of the drive that `path` lies on.
pub fn drive_name_of(path: &Path, home: &Path) -> Option<String> {
    drive_holding(&drives(home), path).map(|drive| drive.name.clone())
}

pub fn drives_of(volumes: Vec<Volume>, home: &Path) -> Vec<Drive> {
    let mut found: Vec<Drive> = volumes
        .into_iter()
        .filter(|volume| !volume.path.starts_with(home))
        .map(|volume| Drive {
            name: volume.name,
            path: volume.path,
            is_home_drive: false,
        })
        .collect();
    let home_drive = drive_holding(&found, home).map(|drive| drive.path.clone());
    for drive in &mut found {
        drive.is_home_drive = home_drive.as_ref() == Some(&drive.path);
    }
    found
}

/// The drive with the longest path that `path` lies below.
fn drive_holding<'drives>(drives: &'drives [Drive], path: &Path) -> Option<&'drives Drive> {
    drives
        .iter()
        .filter(|drive| path.starts_with(&drive.path))
        .max_by_key(|drive| drive.path.components().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(name: &str, path: &str) -> Volume {
        Volume {
            name: name.to_owned(),
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn the_home_folder_is_not_a_drive_and_its_drive_is_marked() {
        let volumes = vec![
            volume("Macintosh HD", "/"),
            volume("marc", "/Users/marc"),
            volume("iCloud Drive", "/Users/marc/Library/Mobile Documents/x"),
            volume("Archiv", "/Volumes/Archiv"),
        ];

        let found = drives_of(volumes, Path::new("/Users/marc"));

        let names: Vec<&str> = found.iter().map(|drive| drive.name.as_str()).collect();
        assert_eq!(names, ["Macintosh HD", "Archiv"]);
        assert!(found[0].is_home_drive);
        assert!(!found[1].is_home_drive);
    }

    #[test]
    fn a_home_folder_on_an_external_drive_marks_that_drive() {
        let volumes = vec![volume("Macintosh HD", "/"), volume("Work", "/Volumes/Work")];

        let found = drives_of(volumes, Path::new("/Volumes/Work/marc"));

        assert!(!found[0].is_home_drive);
        assert!(found[1].is_home_drive);
        assert_eq!(
            drive_holding(&found, Path::new("/Volumes/Work/marc/x")).map(|d| d.name.as_str()),
            Some("Work")
        );
        assert_eq!(
            drive_holding(&found, Path::new("/etc")).map(|d| d.name.as_str()),
            Some("Macintosh HD")
        );
    }
}
