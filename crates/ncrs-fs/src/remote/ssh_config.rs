//! The part of `~/.ssh/config` a login needs: where a host alias really points
//! (`HostName`, `Port`, `User`) and which keys (`IdentityFile`) to offer. Read
//! only, and forgiving: a file that is missing, unreadable or odd gives less,
//! never an error, because the login works without it.

use std::path::{Path, PathBuf};

/// What the config says about one host alias.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostSettings {
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    /// In the order of the file, `~` expanded.
    pub identity_files: Vec<PathBuf>,
}

/// The settings of `alias` in `~/.ssh/config` below `home`. Nothing for no
/// home or a file that cannot be read.
pub fn read_settings(home: Option<&Path>, alias: &str) -> HostSettings {
    let Some(home_folder) = home else {
        return HostSettings::default();
    };
    std::fs::read_to_string(home_folder.join(".ssh").join("config")).map_or_else(
        |_unreadable| HostSettings::default(),
        |text| settings_for(&text, alias, home),
    )
}

/// `path` with a leading `~` replaced by `home`; `None` for a `~` without a home.
pub fn expand_home(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let text = path.to_str()?;
    match text.strip_prefix('~') {
        None => Some(path.to_path_buf()),
        Some("") => home.map(Path::to_path_buf),
        Some(rest) => {
            let below = rest.strip_prefix(['/', '\\'])?;
            home.map(|folder| folder.join(below))
        }
    }
}

/// Settings of the blocks of `text` that `alias` matches. As in OpenSSH, the
/// first value of `HostName`, `User` and `Port` wins and every `IdentityFile`
/// counts. `Include` is not followed.
// ponytail: a file pulled in by `Include` is ignored. Ceiling: a user who keeps
// the hosts in an included file gets no alias there. Next step: read the
// included files, with the glob and the depth limit OpenSSH has.
pub fn settings_for(text: &str, alias: &str, home: Option<&Path>) -> HostSettings {
    let wanted = alias.to_lowercase();
    let mut found = HostSettings::default();
    let mut applies = true;
    for line in text.lines() {
        let Some((keyword, value)) = split_line(line) else {
            continue;
        };
        let values = words(value);
        let first = values.first().map(String::as_str);
        match keyword.to_ascii_lowercase().as_str() {
            "host" => applies = host_matches(&values, &wanted),
            "match" => applies = false,
            _ if !applies => {}
            "hostname" => {
                if found.host_name.is_none() {
                    found.host_name = first.filter(|name| !name.contains('%')).map(str::to_owned);
                }
            }
            "user" => {
                if found.user.is_none() {
                    found.user = first.map(str::to_owned);
                }
            }
            "port" => {
                if found.port.is_none() {
                    found.port = first
                        .and_then(|digits| digits.parse::<u16>().ok())
                        .filter(|number| *number != 0);
                }
            }
            "identityfile" => {
                let expanded = first
                    .filter(|file| !file.contains('%'))
                    .and_then(|file| expand_home(Path::new(file), home))
                    .filter(|file| file.is_absolute());
                found.identity_files.extend(expanded);
            }
            _ => {}
        }
    }
    found
}

/// The keyword and the rest of a line: `key value` or `key=value`. `None` for
/// a blank line and a comment.
fn split_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let end = trimmed.find(|c: char| c.is_whitespace() || c == '=')?;
    let (keyword, rest) = trimmed.split_at(end);
    let value = rest
        .trim_start()
        .strip_prefix('=')
        .unwrap_or(rest)
        .trim_start();
    Some((keyword, value))
}

/// The words of a value: split at white space, with `"quoted words"` kept as
/// one, and an unquoted `#` ending the line.
fn words(value: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '#' if !quoted && !started => break,
            _ if c.is_whitespace() && !quoted => {
                if started {
                    found.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        found.push(current);
    }
    found
}

/// Whether the patterns of a `Host` line take `alias`: one of them matches and
/// no negated one does. Lower case on both sides, as OpenSSH compares.
fn host_matches(patterns: &[String], alias: &str) -> bool {
    let name: Vec<char> = alias.chars().collect();
    let mut matched = false;
    for pattern in patterns {
        let lower = pattern.to_lowercase();
        match lower.strip_prefix('!') {
            Some(negated) => {
                if glob(&negated.chars().collect::<Vec<_>>(), &name) {
                    return false;
                }
            }
            None => matched |= glob(&lower.chars().collect::<Vec<_>>(), &name),
        }
    }
    matched
}

/// `*` for any run of characters, `?` for one.
fn glob(pattern: &[char], text: &[char]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut resume: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                resume = Some((p, t));
                p += 1;
            }
            Some(c) if *c == '?' || Some(c) == text.get(t) => {
                p += 1;
                t += 1;
            }
            _ => match resume {
                Some((star, from)) => {
                    resume = Some((star, from + 1));
                    p = star + 1;
                    t = from + 1;
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const HOME: &str = "/home/marc";

    fn settings(text: &str, alias: &str) -> HostSettings {
        settings_for(text, alias, Some(Path::new(HOME)))
    }

    #[test]
    fn the_first_value_wins_and_identity_files_add_up_in_order() {
        let found = settings(
            "Host files\n  HostName one.example\n  User marc\n  Port 2222\n  IdentityFile ~/.ssh/a\n\
             Host *\n  HostName two.example\n  User other\n  Port 22\n  IdentityFile ~/.ssh/b\n",
            "files",
        );
        assert_eq!(found.host_name.as_deref(), Some("one.example"));
        assert_eq!(found.user.as_deref(), Some("marc"));
        assert_eq!(found.port, Some(2222));
        assert_eq!(
            found.identity_files,
            [
                PathBuf::from("/home/marc/.ssh/a"),
                PathBuf::from("/home/marc/.ssh/b")
            ]
        );
    }

    #[test]
    fn a_block_applies_only_when_a_pattern_matches_and_no_negation_does() {
        let text = "Host web-* db?\n  User ops\nHost * !secret\n  Port 2200\n";
        assert_eq!(settings(text, "web-1").user.as_deref(), Some("ops"));
        assert_eq!(settings(text, "db1").user.as_deref(), Some("ops"));
        assert_eq!(settings(text, "db12").user, None, "? is one character");
        assert_eq!(settings(text, "other").port, Some(2200));
        assert_eq!(settings(text, "secret").port, None, "negated");
        assert_eq!(settings(text, "WEB-2").user.as_deref(), Some("ops"));
    }

    #[test]
    fn lines_before_the_first_host_apply_to_every_host() {
        let found = settings("User everyone\nHost x\n  User x\n", "y");
        assert_eq!(found.user.as_deref(), Some("everyone"));
    }

    #[test]
    fn a_match_block_is_skipped_up_to_the_next_host() {
        let text = "Host a\n  Port 1\nMatch host a\n  User skipped\n  IdentityFile /k\nHost a\n  User kept\n";
        let found = settings(text, "a");
        assert_eq!(found.user.as_deref(), Some("kept"));
        assert!(found.identity_files.is_empty());
    }

    #[test]
    fn keywords_ignore_case_and_values_may_use_equals_quotes_and_comments() {
        let text = "# a comment\n\nHOST=files\n  hostname = \"my host.example\" # trailing\n  PORT=2022\n  IdentityFile \"/keys/with space\"\n";
        let found = settings(text, "files");
        assert_eq!(found.host_name.as_deref(), Some("my host.example"));
        assert_eq!(found.port, Some(2022));
        assert_eq!(found.identity_files, [PathBuf::from("/keys/with space")]);
    }

    #[test]
    fn identity_files_with_tokens_or_without_a_place_are_dropped() {
        let text =
            "Host a\n  IdentityFile ~/.ssh/%h_key\n  IdentityFile relative\n  IdentityFile ~/ok\n";
        assert_eq!(
            settings(text, "a").identity_files,
            [PathBuf::from("/home/marc/ok")]
        );
        assert!(settings_for(text, "a", None).identity_files.is_empty());
    }

    #[test]
    fn a_bad_port_is_ignored_and_the_next_one_counts() {
        let found = settings("Host a\n  Port x\n  Port 0\n  Port 99\n", "a");
        assert_eq!(found.port, Some(99));
    }

    #[test]
    fn a_missing_file_or_home_is_empty() {
        let scratch = tempfile::tempdir().unwrap();
        assert_eq!(
            read_settings(Some(scratch.path()), "a"),
            HostSettings::default()
        );
        assert_eq!(read_settings(None, "a"), HostSettings::default());
    }

    #[test]
    fn the_file_below_ssh_is_read() {
        let scratch = tempfile::tempdir().unwrap();
        std::fs::create_dir(scratch.path().join(".ssh")).unwrap();
        std::fs::write(
            scratch.path().join(".ssh").join("config"),
            "Host a\n  User u\n",
        )
        .unwrap();
        assert_eq!(
            read_settings(Some(scratch.path()), "a").user.as_deref(),
            Some("u")
        );
    }
}
