use std::fs;
use std::path::{Path, PathBuf};

/// Which apps are installed on this machine, asked by name only.
pub trait InstalledApps {
    /// Where the app called `name` is installed, if it is.
    fn find(&self, name: &str) -> Option<PathBuf>;
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
