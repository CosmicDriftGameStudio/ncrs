//! Which host keys to trust. The user's own `~/.ssh/known_hosts` is read, never
//! written; what the user confirms in ncrs goes to a file of ncrs's own. The
//! parser is tolerant: a line it does not understand is skipped, because a
//! file written by OpenSSH over the years holds all kinds.

use std::io;
use std::path::{Path, PathBuf};

use data_encoding::BASE64_MIME;
use hmac::{Hmac, KeyInit as _, Mac as _};
use russh::keys::{Algorithm, PublicKey};
use sha1::Sha1;

/// Where the host keys are looked up. Both are optional: tests and a machine
/// without a home folder have neither.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownHostsFiles {
    /// The user's OpenSSH file, read only.
    pub ssh: Option<PathBuf>,
    /// The file ncrs writes the keys the user confirmed to.
    pub own: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostPattern {
    /// `host` or `[host]:port`, as written.
    Plain(String),
    Hashed {
        salt: Vec<u8>,
        hash: Vec<u8>,
    },
}

/// One usable line of a known_hosts file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    line: usize,
    revoked: bool,
    hosts: Vec<HostPattern>,
    key: PublicKey,
}

/// What one file says about a key a server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Known,
    Unknown,
    /// The host has a key of this type on `line`, and it is another one.
    Changed {
        line: usize,
    },
    /// The host has a key on record, but of another type than the one offered:
    /// a server that offers another type than it did is not the one that was
    /// trusted. `line` is the entry that is on record.
    OtherTypeKnown {
        line: usize,
    },
    Revoked,
}

/// What the files together say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostVerdict {
    Known,
    Unknown,
    Changed {
        file: PathBuf,
        line: usize,
    },
    OtherTypeKnown {
        file: PathBuf,
        line: usize,
    },
    /// Nothing says the key is known, but a file that might could not be read,
    /// so the server cannot be checked and there is nothing to trust on.
    Unreadable {
        file: PathBuf,
        reason: String,
    },
    Revoked,
}

/// Reads the lines of a known_hosts file. Comments, `@cert-authority` lines,
/// patterns with `*`, `?` or `!` and anything unreadable are left out.
pub fn parse(text: &str) -> Vec<Entry> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_line(index + 1, line))
        .collect()
}

fn parse_line(line_number: usize, line: &str) -> Option<Entry> {
    let mut words = line.split_whitespace();
    let first = words.next().filter(|word| !word.starts_with('#'))?;
    let (revoked, hosts_word) = match first {
        "@revoked" => (true, words.next()?),
        marker if marker.starts_with('@') => return None,
        hosts => (false, hosts),
    };
    let algorithm = words.next()?;
    let base64 = words.next()?;
    let key = PublicKey::from_openssh(&format!("{algorithm} {base64}")).ok()?;
    let hosts: Vec<HostPattern> = hosts_word.split(',').filter_map(pattern_of).collect();
    if hosts.is_empty() {
        return None;
    }
    Some(Entry {
        line: line_number,
        revoked,
        hosts,
        key,
    })
}

fn pattern_of(word: &str) -> Option<HostPattern> {
    if let Some(hashed) = word.strip_prefix("|1|") {
        let (salt, hash) = hashed.split_once('|')?;
        return Some(HostPattern::Hashed {
            salt: BASE64_MIME.decode(salt.as_bytes()).ok()?,
            hash: BASE64_MIME.decode(hash.as_bytes()).ok()?,
        });
    }
    if word.is_empty() || word.starts_with('!') || word.contains(['*', '?']) {
        return None;
    }
    Some(HostPattern::Plain(word.to_lowercase()))
}

/// The name a server is entered under: the bare host on port 22, otherwise
/// `[host]:port`.
fn entry_name(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_lowercase()
    } else {
        format!("[{}]:{port}", host.to_lowercase())
    }
}

impl HostPattern {
    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Plain(text) => text == name,
            Self::Hashed { salt, hash } => Hmac::<Sha1>::new_from_slice(salt)
                .is_ok_and(|mac| mac.chain_update(name).verify_slice(hash).is_ok()),
        }
    }
}

/// What `entries` say about `key` offered by `host:port`. A revoked key is
/// refused whatever else is listed; a key of the same type that differs from
/// the recorded one is a change; a recorded key of another type, with none of
/// this type, is a change of type.
pub fn check(entries: &[Entry], host: &str, port: u16, key: &PublicKey) -> Verdict {
    let name = entry_name(host, port);
    let mut changed = None;
    let mut other_type = None;
    let mut known = false;
    for entry in entries
        .iter()
        .filter(|entry| entry.hosts.iter().any(|pattern| pattern.matches(&name)))
    {
        let same_key = entry.key.key_data() == key.key_data();
        if entry.revoked {
            if same_key {
                return Verdict::Revoked;
            }
            continue;
        }
        if entry.key.algorithm() != key.algorithm() {
            other_type.get_or_insert(entry.line);
            continue;
        }
        if same_key {
            known = true;
        } else if changed.is_none() {
            changed = Some(entry.line);
        }
    }
    match (known, changed, other_type) {
        (true, _, _) => Verdict::Known,
        (false, Some(line), _) => Verdict::Changed { line },
        (false, None, Some(line)) => Verdict::OtherTypeKnown { line },
        (false, None, None) => Verdict::Unknown,
    }
}

/// The key types on record for `host:port`. A server is asked for these first,
/// so that it offers the key that is known instead of one of another type.
pub fn algorithms_on_record(entries: &[Entry], host: &str, port: u16) -> Vec<Algorithm> {
    let name = entry_name(host, port);
    let mut found: Vec<Algorithm> = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| !entry.revoked && entry.hosts.iter().any(|pattern| pattern.matches(&name)))
    {
        let algorithm = entry.key.algorithm();
        if !found.contains(&algorithm) {
            found.push(algorithm);
        }
    }
    found
}

/// The parsed files, in the order they are asked.
#[derive(Debug, Clone, Default)]
pub struct KnownHosts {
    files: Vec<(PathBuf, Vec<Entry>)>,
    /// Files that exist but could not be read, with the system's reason.
    unreadable: Vec<(PathBuf, String)>,
}

impl KnownHosts {
    /// Blocking. A file that is missing counts as empty. One that is there and
    /// cannot be read does not: it may hold the very key that decides, so a
    /// server whose key no readable file knows is not trusted on that basis.
    pub fn load(files: &KnownHostsFiles) -> Self {
        let mut loaded = Self::default();
        for path in [&files.ssh, &files.own].into_iter().flatten() {
            match std::fs::read(path) {
                Ok(bytes) => loaded
                    .files
                    .push((path.clone(), parse(&String::from_utf8_lossy(&bytes)))),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    loaded.files.push((path.clone(), Vec::new()));
                }
                Err(error) => loaded.unreadable.push((path.clone(), error.to_string())),
            }
        }
        loaded
    }

    /// The key types on record for the host, to ask the server for them first.
    pub fn algorithms_for(&self, host: &str, port: u16) -> Vec<Algorithm> {
        let mut found: Vec<Algorithm> = Vec::new();
        for (_, entries) in &self.files {
            for algorithm in algorithms_on_record(entries, host, port) {
                if !found.contains(&algorithm) {
                    found.push(algorithm);
                }
            }
        }
        found
    }

    /// A change or a revocation in either file refuses the key, even when the
    /// other file knows it.
    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> HostVerdict {
        let verdicts: Vec<(&PathBuf, Verdict)> = self
            .files
            .iter()
            .map(|(path, entries)| (path, check(entries, host, port, key)))
            .collect();
        if verdicts
            .iter()
            .any(|(_, verdict)| *verdict == Verdict::Revoked)
        {
            return HostVerdict::Revoked;
        }
        if let Some((file, line)) = verdicts.iter().find_map(|(path, verdict)| match verdict {
            Verdict::Changed { line } => Some(((*path).clone(), *line)),
            _ => None,
        }) {
            return HostVerdict::Changed { file, line };
        }
        if let Some((file, line)) = verdicts.iter().find_map(|(path, verdict)| match verdict {
            Verdict::OtherTypeKnown { line } => Some(((*path).clone(), *line)),
            _ => None,
        }) {
            return HostVerdict::OtherTypeKnown { file, line };
        }
        if verdicts
            .iter()
            .any(|(_, verdict)| *verdict == Verdict::Known)
        {
            return HostVerdict::Known;
        }
        match self.unreadable.first() {
            Some((file, reason)) => HostVerdict::Unreadable {
                file: file.clone(),
                reason: reason.clone(),
            },
            None => HostVerdict::Unknown,
        }
    }
}

/// Appends the key the user confirmed to ncrs's own file. Never called with
/// the OpenSSH file.
pub fn learn(own: &Path, host: &str, port: u16, key: &PublicKey) -> io::Result<()> {
    russh::keys::known_hosts::learn_known_hosts_path(host, port, key, own)
        .map_err(|error| io::Error::other(error.to_string()))
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

    const KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X";
    const HASHED: &str = "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";

    fn key(text: &str) -> PublicKey {
        PublicKey::from_openssh(text).unwrap()
    }

    #[test]
    fn plain_entry_matches_host_and_key() {
        let entries = parse(&format!("example.org,10.0.0.1 {KEY_A} comment\n"));
        assert_eq!(
            check(&entries, "Example.org", 22, &key(KEY_A)),
            Verdict::Known
        );
        assert_eq!(check(&entries, "10.0.0.1", 22, &key(KEY_A)), Verdict::Known);
        assert_eq!(
            check(&entries, "other.org", 22, &key(KEY_A)),
            Verdict::Unknown
        );
    }

    #[test]
    fn non_default_port_uses_bracket_form() {
        let entries = parse(&format!("[example.org]:2222 {KEY_A}\n"));
        assert_eq!(
            check(&entries, "example.org", 2222, &key(KEY_A)),
            Verdict::Known
        );
        assert_eq!(
            check(&entries, "example.org", 22, &key(KEY_A)),
            Verdict::Unknown
        );
        assert_eq!(
            check(&entries, "example.org", 2223, &key(KEY_A)),
            Verdict::Unknown
        );
    }

    #[test]
    fn hashed_entry_matches() {
        let entries = parse(&format!("{HASHED}\n"));
        let hashed_key = key(HASHED.split_once(' ').unwrap().1);

        assert_eq!(entries.len(), 1);
        assert_eq!(
            check(&entries, "example.com", 22, &hashed_key),
            Verdict::Known
        );
        assert_eq!(
            check(&entries, "example.org", 22, &hashed_key),
            Verdict::Unknown
        );
    }

    #[test]
    fn hashed_entry_made_here_is_found() {
        let salt = b"0123456789abcdefghij";
        let mut mac = Hmac::<Sha1>::new_from_slice(salt).unwrap();
        mac.update(b"secret.example.org");
        let hash = mac.finalize().into_bytes();
        let line = format!(
            "|1|{}|{} {KEY_A}\n",
            data_encoding::BASE64.encode(salt),
            data_encoding::BASE64.encode(&hash)
        );
        let entries = parse(&line);
        assert_eq!(
            check(&entries, "secret.example.org", 22, &key(KEY_A)),
            Verdict::Known
        );
        assert_eq!(
            check(&entries, "other.example.org", 22, &key(KEY_A)),
            Verdict::Unknown
        );
    }

    #[test]
    fn changed_key_is_reported_with_file_and_line() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("known_hosts");
        std::fs::write(
            &ssh,
            format!("# a comment\nother.org {KEY_A}\nexample.org {KEY_A}\n"),
        )
        .unwrap();
        let hosts = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(ssh.clone()),
            own: None,
        });

        assert_eq!(
            hosts.check("example.org", 22, &key(KEY_B)),
            HostVerdict::Changed { file: ssh, line: 3 }
        );
    }

    #[test]
    fn revoked_marker_rejects() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("known_hosts");
        let own = dir.path().join("own");
        std::fs::write(&ssh, format!("@revoked example.org {KEY_A}\n")).unwrap();
        std::fs::write(&own, format!("example.org {KEY_A}\n")).unwrap();
        let hosts = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(ssh),
            own: Some(own),
        });

        assert_eq!(
            hosts.check("example.org", 22, &key(KEY_A)),
            HostVerdict::Revoked
        );
        assert_eq!(
            hosts.check("other.org", 22, &key(KEY_A)),
            HostVerdict::Unknown
        );
    }

    #[test]
    fn a_change_in_one_file_wins_over_a_match_in_the_other() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("known_hosts");
        let own = dir.path().join("own");
        std::fs::write(&ssh, format!("example.org {KEY_B}\n")).unwrap();
        std::fs::write(&own, format!("example.org {KEY_A}\n")).unwrap();
        let hosts = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(ssh.clone()),
            own: Some(own),
        });

        assert_eq!(
            hosts.check("example.org", 22, &key(KEY_A)),
            HostVerdict::Changed { file: ssh, line: 1 }
        );
    }

    #[test]
    fn unparsable_lines_are_skipped() {
        let text = format!(
            "\n# comment\n@cert-authority *.example.org {KEY_A}\n*.example.org {KEY_A}\n!x.org {KEY_A}\nbroken line\nexample.org ssh-ed25519 not-base64!!\nexample.org\ngood.org {KEY_A}\n"
        );
        let entries = parse(&text);
        assert_eq!(entries.len(), 1);
        assert_eq!(check(&entries, "good.org", 22, &key(KEY_A)), Verdict::Known);
    }

    #[test]
    fn other_algorithm_for_same_host_is_a_change_of_type_not_unknown() {
        let ecdsa = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=";
        let entries = parse(&format!("example.org {ecdsa}\n"));
        assert_eq!(
            check(&entries, "example.org", 22, &key(KEY_A)),
            Verdict::OtherTypeKnown { line: 1 }
        );
        assert_eq!(
            check(&entries, "elsewhere.org", 22, &key(KEY_A)),
            Verdict::Unknown,
            "only the host with a key on record is concerned"
        );
        let both = parse(&format!("example.org {ecdsa}\nexample.org {KEY_A}\n"));
        assert_eq!(
            check(&both, "example.org", 22, &key(KEY_A)),
            Verdict::Known,
            "a known key of the offered type wins"
        );
    }

    #[test]
    fn the_types_on_record_come_first_in_the_order_of_the_files() {
        let ecdsa = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBEmKSENjQEezOmxkZMy7opKgwFB9nkt5YRrYMjNuG5N87uRgg6CLrbo5wAdT/y6v0mKV0U2w0WZ2YB/++Tpockg=";
        let entries = parse(&format!(
            "example.org {ecdsa}\nother.org {KEY_A}\n@revoked example.org {KEY_B}\n"
        ));
        assert_eq!(
            algorithms_on_record(&entries, "example.org", 22),
            [Algorithm::Ecdsa {
                curve: russh::keys::EcdsaCurve::NistP256
            }]
        );
        assert!(algorithms_on_record(&entries, "unknown.org", 22).is_empty());
    }

    #[test]
    fn a_file_that_cannot_be_read_leaves_an_unknown_key_unusable_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: reading it fails, and not as
        // "not found".
        let unreadable = dir.path().join("known_hosts");
        std::fs::create_dir(&unreadable).unwrap();
        let hosts = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(unreadable.clone()),
            own: Some(dir.path().join("missing")),
        });

        let verdict = hosts.check("example.org", 22, &key(KEY_A));

        assert!(
            matches!(&verdict, HostVerdict::Unreadable { file, .. } if *file == unreadable),
            "{verdict:?}"
        );
    }

    #[test]
    fn a_missing_file_is_empty_and_a_key_known_elsewhere_still_counts() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("own");
        std::fs::write(&own, format!("example.org {KEY_A}\n")).unwrap();
        let unreadable = dir.path().join("ssh");
        std::fs::create_dir(&unreadable).unwrap();

        let none = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(dir.path().join("missing")),
            own: Some(dir.path().join("also-missing")),
        });
        let beside = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(unreadable),
            own: Some(own),
        });

        assert_eq!(
            none.check("example.org", 22, &key(KEY_A)),
            HostVerdict::Unknown
        );
        assert_eq!(
            beside.check("example.org", 22, &key(KEY_A)),
            HostVerdict::Known
        );
    }

    #[test]
    fn learned_key_goes_to_own_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh_known_hosts");
        std::fs::write(&ssh, "# the user's file\n").unwrap();
        let own = dir.path().join("sub").join("known_hosts");

        learn(&own, "example.org", 2222, &key(KEY_A)).unwrap();

        assert_eq!(
            std::fs::read_to_string(&ssh).unwrap(),
            "# the user's file\n"
        );
        let hosts = KnownHosts::load(&KnownHostsFiles {
            ssh: Some(ssh),
            own: Some(own),
        });
        assert_eq!(
            hosts.check("example.org", 2222, &key(KEY_A)),
            HostVerdict::Known
        );
        assert_eq!(
            hosts.check("example.org", 22, &key(KEY_A)),
            HostVerdict::Unknown
        );
    }
}
