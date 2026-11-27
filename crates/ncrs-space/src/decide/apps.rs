use std::fs;
use std::path::{Path, PathBuf};

use crate::facts::VersionRelation;

/// Which apps are installed on this machine, asked by name only.
pub trait InstalledApps {
    /// Where the app called `name` is installed, if it is.
    fn find(&self, name: &str) -> Option<PathBuf>;

    /// The version of the installed app at `app`, where the system tells it.
    fn version(&self, _app: &Path) -> Option<String> {
        None
    }
}

/// The version in an installer's file name ("Xcode_17.xip" is "17"): the first word after the
/// name that is only a number with dots, with or without a leading "v".
pub fn version_of_installer(file_name: &str) -> Option<String> {
    let stem = Path::new(file_name).file_stem()?.to_str()?;
    stem.split(|character: char| matches!(character, ' ' | '-' | '_'))
        .skip(1)
        .map(|word| word.strip_prefix(['v', 'V']).unwrap_or(word))
        .find(|word| is_dotted_number(word))
        .map(str::to_owned)
}

fn is_dotted_number(word: &str) -> bool {
    !word.is_empty()
        && word.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

fn number_parts(version: &str) -> Option<Vec<u64>> {
    let mut parts: Vec<u64> = version
        .trim()
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
}

/// How the installed version compares with the installer's; trailing zeros do not count ("17" is "17.0").
pub fn compare_versions(installed: Option<&str>, installer: Option<&str>) -> VersionRelation {
    let (Some(installed), Some(installer)) = (
        installed.and_then(number_parts),
        installer.and_then(number_parts),
    ) else {
        return VersionRelation::Unknown;
    };
    match installed.cmp(&installer) {
        std::cmp::Ordering::Equal => VersionRelation::Same,
        std::cmp::Ordering::Greater => VersionRelation::InstalledNewer,
        std::cmp::Ordering::Less => VersionRelation::InstalledOlder,
    }
}

/// Words of an installer's file name that follow the app name and are not part of it.
const NOISE_WORDS: [&str; 20] = [
    "macos",
    "osx",
    "darwin",
    "win",
    "win32",
    "win64",
    "windows",
    "linux",
    "x64",
    "x86",
    "x86_64",
    "amd64",
    "arm64",
    "aarch64",
    "universal",
    "intel",
    "silicon",
    "setup",
    "installer",
    "install",
];

/// The app an installer file is for, from its name: the words before a version
/// number, a platform word or a copy marker. `None` when nothing is left.
pub fn app_name_of_installer(file_name: &str) -> Option<String> {
    let stem = Path::new(file_name).file_stem()?.to_str()?;
    // The first word is always part of the name: "1Password" starts with a digit.
    let words: Vec<&str> = stem
        .split(|character: char| matches!(character, ' ' | '-' | '_'))
        .filter(|word| !word.is_empty())
        .enumerate()
        .take_while(|(position, word)| *position == 0 || !ends_the_name(word))
        .map(|(_, word)| word)
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

fn ends_the_name(word: &str) -> bool {
    let lower = word.to_lowercase();
    let after_v = lower.strip_prefix('v').unwrap_or(&lower);
    word.starts_with('(')
        || lower.starts_with(|character: char| character.is_ascii_digit())
        || after_v.starts_with(|character: char| character.is_ascii_digit())
        || NOISE_WORDS.contains(&lower.as_str())
}

fn normalized(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The apps the system lists: `.app` bundles on macOS, `.desktop` entries on
/// Linux, Start menu shortcuts on Windows. Read once, matched by name.
#[derive(Debug, Clone)]
pub struct SystemInstalledApps {
    known: Vec<(String, PathBuf)>,
}

impl SystemInstalledApps {
    pub fn new(home: &Path) -> Self {
        let mut known = Vec::new();
        if cfg!(target_os = "macos") {
            for folder in [PathBuf::from("/Applications"), home.join("Applications")] {
                collect_bundles(&folder, &mut known);
            }
        } else if cfg!(windows) {
            for variable in ["ProgramData", "APPDATA"] {
                if let Some(base) = std::env::var_os(variable) {
                    let menu = PathBuf::from(base).join("Microsoft/Windows/Start Menu/Programs");
                    collect_shortcuts(&menu, &mut known, 4);
                }
            }
        } else {
            for folder in [
                PathBuf::from("/usr/share/applications"),
                home.join(".local/share/applications"),
            ] {
                collect_desktop_entries(&folder, &mut known);
            }
        }
        Self { known }
    }
}

impl InstalledApps for SystemInstalledApps {
    fn find(&self, name: &str) -> Option<PathBuf> {
        let wanted = normalized(name);
        if wanted.is_empty() {
            return None;
        }
        self.known
            .iter()
            .find(|(known, _)| *known == wanted)
            .map(|(_, path)| path.clone())
    }

    fn version(&self, app: &Path) -> Option<String> {
        bundle_version(app)
    }
}

/// `CFBundleShortVersionString` of the `Info.plist` of a macOS app bundle.
#[cfg(target_os = "macos")]
fn bundle_version(app: &Path) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let text = fs::read_to_string(&plist).ok();
    if let Some(version) = text.as_deref().and_then(plist_version) {
        return Some(version);
    }
    binary_plist_version(&plist)
}

/// A binary plist is not text; `plutil` reads it. Not available in the App Store build.
#[cfg(all(target_os = "macos", not(feature = "sandbox")))]
fn binary_plist_version(plist: &Path) -> Option<String> {
    let output = std::process::Command::new("plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(plist)
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!version.is_empty()).then_some(version)
}

#[cfg(all(target_os = "macos", feature = "sandbox"))]
fn binary_plist_version(_plist: &Path) -> Option<String> {
    None
}

#[cfg(not(target_os = "macos"))]
fn bundle_version(_app: &Path) -> Option<String> {
    None
}

/// The string after the `CFBundleShortVersionString` key of an XML plist.
#[cfg(any(target_os = "macos", test))]
pub(super) fn plist_version(text: &str) -> Option<String> {
    let after_key = text.split_once("<key>CFBundleShortVersionString</key>")?.1;
    let value = after_key
        .split_once("<string>")?
        .1
        .split_once("</string>")?
        .0;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn collect_bundles(folder: &Path, known: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "app") {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                known.push((normalized(stem), path));
            }
        }
    }
}

fn collect_shortcuts(folder: &Path, known: &mut Vec<(String, PathBuf)>, depth: usize) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 0 {
                collect_shortcuts(&path, known, depth - 1);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
        {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                known.push((normalized(stem), path));
            }
        }
    }
}

fn collect_desktop_entries(folder: &Path, known: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_none_or(|extension| extension != "desktop")
        {
            continue;
        }
        if let Some(name) = fs::read_to_string(&path)
            .ok()
            .as_deref()
            .and_then(desktop_name)
        {
            known.push((normalized(&name), path));
        }
    }
}

/// The untranslated `Name=` of the `[Desktop Entry]` group.
pub(super) fn desktop_name(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(name) = line.strip_prefix("Name=") {
                return Some(name.trim().to_owned()).filter(|name| !name.is_empty());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_untranslated_name_of_the_desktop_entry_group_counts() {
        let text = "[Desktop Action new]\nName=Wrong\n\n[Desktop Entry]\nName[de]=Deutsch\nName=Firefox\nExec=firefox\n";

        assert_eq!(desktop_name(text).as_deref(), Some("Firefox"));
    }

    #[test]
    fn the_version_is_the_number_after_the_name() {
        assert_eq!(version_of_installer("Xcode_17.xip").as_deref(), Some("17"));
        assert_eq!(
            version_of_installer("GitKraken-v9.1.0-arm64.dmg").as_deref(),
            Some("9.1.0")
        );
        assert_eq!(version_of_installer("1Password.dmg"), None);
        assert_eq!(version_of_installer("Setup.exe"), None);
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert_eq!(
            compare_versions(Some("17.0"), Some("17")),
            VersionRelation::Same
        );
        assert_eq!(
            compare_versions(Some("1.10"), Some("1.9")),
            VersionRelation::InstalledNewer
        );
        assert_eq!(
            compare_versions(Some("1.7.0"), Some("1.7.1")),
            VersionRelation::InstalledOlder
        );
        assert_eq!(compare_versions(None, Some("1")), VersionRelation::Unknown);
        assert_eq!(
            compare_versions(Some("beta"), Some("1")),
            VersionRelation::Unknown
        );
    }

    #[test]
    fn the_bundle_version_is_read_from_an_xml_plist() {
        let text = "<dict><key>CFBundleName</key><string>X</string>\n<key>CFBundleShortVersionString</key>\n<string>17.0</string></dict>";
        assert_eq!(plist_version(text).as_deref(), Some("17.0"));
        assert_eq!(plist_version("<dict></dict>"), None);
    }

    #[test]
    fn names_match_without_case_spaces_or_punctuation() {
        let apps = SystemInstalledApps {
            known: vec![(
                normalized("Visual Studio Code"),
                PathBuf::from("/Applications/Visual Studio Code.app"),
            )],
        };

        assert!(apps.find("visual-studio-code").is_some());
        assert!(apps.find("Firefox").is_none());
    }
}
