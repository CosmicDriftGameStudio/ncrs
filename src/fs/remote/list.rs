//! Reading a folder on a server.

use std::ffi::OsString;
use std::time::{Duration, UNIX_EPOCH};

use iced::futures::future::join_all;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use tokio::time::{timeout, timeout_at};

use super::session::{Pool, RemoteError};
use super::{is_deceptive, RemotePath};
use crate::fs::reader::Listing;
use crate::fs::{sort_entries, FileEntry, ReadError, SortKey};

/// How many links in one folder are followed to learn whether they lead to a
/// folder. Each costs a request, and a folder of thousands of links must still
/// open.
const LINKS_FOLLOWED: usize = 256;

/// How many of those lookups are on their way at once.
const LINKS_AT_ONCE: usize = 8;

/// The entries of `path`, sorted. Never fails from the caller's side: a failure
/// is the `error` of the listing, which then holds only `..`.
pub async fn list(pool: &Pool, path: RemotePath, sorted_by: SortKey) -> Listing {
    let panel_path = path.to_path_buf();
    let parent = path
        .parent()
        .map(|above| FileEntry::parent(&above.to_path_buf()));
    let failed = |error: ReadError| Listing {
        path: panel_path.clone(),
        entries: parent.clone().into_iter().collect(),
        error: Some(error),
        sorted_by,
        archive: None,
    };
    let server = path.server();
    let Some(session) = pool.session(server) else {
        return failed(ReadError::RemoteDisconnected {
            server: server.clone(),
        });
    };
    let read = timeout(
        pool.timeouts().list,
        read_all(session.sftp(), &path, pool.timeouts().links),
    )
    .await;
    let error = match read {
        Ok(Ok((mut entries, left_out))) => {
            entries.extend(parent.clone());
            sort_entries(&mut entries, sorted_by);
            return Listing {
                path: panel_path.clone(),
                entries,
                error: (left_out > 0).then_some(ReadError::EntriesLeftOut { count: left_out }),
                sorted_by,
                archive: None,
            };
        }
        Ok(Err(error)) => RemoteError::of_sftp(&error),
        Err(_) => RemoteError::Timeout,
    };
    pool.note(server, &error);
    failed(match error {
        RemoteError::Timeout => ReadError::RemoteTimeout {
            host: server.host.clone(),
        },
        RemoteError::Lost(_) => ReadError::RemoteDisconnected {
            server: server.clone(),
        },
        RemoteError::NotFound => ReadError::Remote {
            reason: "no such folder".to_owned(),
        },
        RemoteError::Refused(reason) => ReadError::Remote { reason },
    })
}

async fn read_all(
    sftp: &SftpSession,
    path: &RemotePath,
    links_budget: Duration,
) -> Result<(Vec<FileEntry>, usize), SftpError> {
    let mut entries = Vec::new();
    let mut left_out = 0;
    // Which entries are links worth following, and where they lead from.
    let mut links: Vec<(usize, String)> = Vec::new();
    for item in sftp.read_dir(path.remote_str()).await? {
        let name = item.file_name();
        if !is_plain_name(&name) {
            left_out += 1;
            continue;
        }
        let child = path.join(&name);
        let meta = item.metadata();
        let is_symlink = meta.file_type().is_symlink();
        if is_symlink && links.len() < LINKS_FOLLOWED {
            links.push((entries.len(), child.remote_str().to_owned()));
        }
        entries.push(entry_of(name, &child, &meta, is_symlink));
    }
    resolve_links(sftp, &mut entries, &links, links_budget).await;
    Ok((entries, left_out))
}

fn entry_of(
    name: String,
    child: &RemotePath,
    shown: &russh_sftp::client::fs::Metadata,
    is_symlink: bool,
) -> FileEntry {
    let is_dir = shown.file_type().is_dir();
    FileEntry {
        name: OsString::from(name),
        path: child.to_path_buf(),
        is_dir,
        is_symlink,
        is_parent: false,
        size: if is_dir { 0 } else { shown.len() },
        modified: shown
            .mtime
            .map(|secs| UNIX_EPOCH + Duration::from_secs(u64::from(secs))),
        folder_size: None,
    }
}

/// Finds out where the links lead, a few at a time, for as long as `budget`
/// lasts. What is not found out by then, and what leads nowhere, stays a link
/// to something that is not a folder: a slow server must not hold up the
/// listing for the sake of a link's icon.
async fn resolve_links(
    sftp: &SftpSession,
    entries: &mut [FileEntry],
    links: &[(usize, String)],
    budget: Duration,
) {
    let deadline = tokio::time::Instant::now() + budget;
    for batch in links.chunks(LINKS_AT_ONCE) {
        let lookups = join_all(
            batch
                .iter()
                .map(|(_, remote)| sftp.metadata(remote.as_str())),
        );
        let Ok(found) = timeout_at(deadline, lookups).await else {
            return;
        };
        for ((index, _), result) in batch.iter().zip(found) {
            let (Ok(target), Some(entry)) = (result, entries.get_mut(*index)) else {
                continue;
            };
            entry.is_dir = target.file_type().is_dir();
            entry.size = if entry.is_dir { 0 } else { target.len() };
            entry.modified = target
                .mtime
                .map(|secs| UNIX_EPOCH + Duration::from_secs(u64::from(secs)));
        }
    }
}

/// A name a server may send and that can be one entry of a folder: no
/// separator, no control character and no bidirectional control in it.
/// Anything else could address something else when it is turned into a path,
/// or show as something it is not.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.chars().any(is_deceptive)
        && !(cfg!(windows) && name.contains('\\'))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::super::test_server::{short_timeouts, symlink, Fixture};
    #[cfg(unix)]
    use super::super::{test_server::timeouts, Timeouts};
    use super::*;

    #[tokio::test]
    async fn listing_shows_files_folders_and_links_without_dot_entries() {
        let fixture = Fixture::connected().await;
        let docs = fixture.server.local("/docs");
        std::fs::create_dir(&docs).unwrap();
        std::fs::write(docs.join("a.txt"), "abc").unwrap();
        std::fs::create_dir(docs.join("sub")).unwrap();
        symlink(&docs.join("sub"), &docs.join("to-sub"));
        symlink(&docs.join("nowhere"), &docs.join("dangling"));

        let listing = list(&fixture.pool, Fixture::at("/docs"), SortKey::default()).await;

        assert_eq!(listing.error, None);
        let names: Vec<String> = listing
            .entries
            .iter()
            .map(|entry| entry.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.first().map(String::as_str), Some(".."), "{names:?}");
        assert!(!names.iter().any(|name| name == "."));
        let find = |name: &str| listing.entries.iter().find(|e| e.name == name).unwrap();
        assert!(find("sub").is_dir);
        assert!(!find("a.txt").is_dir);
        assert_eq!(find("a.txt").size, 3);
        assert_eq!(
            find("a.txt").path,
            std::path::PathBuf::from("sftp://tester@test.invalid/docs/a.txt")
        );
        assert_eq!(
            find("..").path,
            std::path::PathBuf::from("sftp://tester@test.invalid/")
        );
        #[cfg(unix)]
        {
            assert!(find("to-sub").is_symlink && find("to-sub").is_dir);
            assert!(find("dangling").is_symlink && !find("dangling").is_dir);
        }
    }

    #[tokio::test]
    async fn the_root_has_no_parent_entry() {
        let fixture = Fixture::connected().await;

        let listing = list(&fixture.pool, Fixture::at("/"), SortKey::default()).await;

        assert!(listing.entries.iter().all(|entry| !entry.is_parent));
        assert!(listing.entries.iter().any(|entry| entry.name == "home"));
    }

    #[test]
    fn names_that_cannot_be_one_entry_are_not_plain() {
        for bad in [
            "a/b",
            "a\0b",
            "",
            "..",
            ".",
            "a\u{202E}b",
            "a\u{2066}b",
            "a\nb",
            "a\u{1b}[0m",
        ] {
            assert!(!is_plain_name(bad), "{bad:?}");
        }
        assert!(is_plain_name("ordinary name.txt"));
        assert_eq!(is_plain_name(r"a\b"), !cfg!(windows));
    }

    #[tokio::test]
    async fn a_missing_folder_is_an_error_of_the_listing_and_keeps_the_connection() {
        let fixture = Fixture::connected().await;

        let listing = list(
            &fixture.pool,
            Fixture::at("/nope/deeper"),
            SortKey::default(),
        )
        .await;

        assert!(
            matches!(listing.error, Some(ReadError::Remote { .. })),
            "{:?}",
            listing.error
        );
        assert!(listing.entries.iter().all(|entry| entry.is_parent));
        assert!(fixture.pool.is_connected(&Fixture::id()));
    }

    #[tokio::test]
    async fn a_stalled_listing_times_out_and_the_connection_stays() {
        let fixture = Fixture::connected_with(short_timeouts()).await;
        fixture.server.control.stall_listing();

        let listing = list(&fixture.pool, Fixture::at("/home"), SortKey::default()).await;

        assert!(
            matches!(listing.error, Some(ReadError::RemoteTimeout { .. })),
            "{:?}",
            listing.error
        );
        assert!(
            fixture.pool.is_connected(&Fixture::id()),
            "a slow answer is no proof that the connection is gone"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_server_that_will_not_say_where_links_lead_cannot_hold_the_listing() {
        let limits = Timeouts {
            links: Duration::from_millis(150),
            ..timeouts()
        };
        let fixture = Fixture::connected_with(limits).await;
        let docs = fixture.server.local("/home/tester/docs");
        std::fs::create_dir_all(docs.join("target")).unwrap();
        for index in 0..40 {
            symlink(&docs.join("target"), &docs.join(format!("link-{index:02}")));
        }
        fixture.server.control.stall_stat();
        let started = std::time::Instant::now();

        let listing = list(
            &fixture.pool,
            Fixture::at("/home/tester/docs"),
            SortKey::default(),
        )
        .await;

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(listing.error, None);
        let links: Vec<_> = listing.entries.iter().filter(|e| e.is_symlink).collect();
        assert_eq!(links.len(), 40, "every link is still listed");
        assert!(
            links.iter().all(|link| !link.is_dir),
            "and stays a plain link"
        );
        assert!(fixture.pool.is_connected(&Fixture::id()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn many_links_are_all_followed_when_the_server_answers() {
        let fixture = Fixture::connected().await;
        let docs = fixture.server.local("/home/tester/docs");
        std::fs::create_dir_all(docs.join("target")).unwrap();
        for index in 0..30 {
            symlink(&docs.join("target"), &docs.join(format!("link-{index:02}")));
        }

        let listing = list(
            &fixture.pool,
            Fixture::at("/home/tester/docs"),
            SortKey::default(),
        )
        .await;

        let links: Vec<_> = listing.entries.iter().filter(|e| e.is_symlink).collect();
        assert_eq!(links.len(), 30);
        assert!(links.iter().all(|link| link.is_dir));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn names_that_hide_what_they_are_are_left_out_and_counted() {
        let fixture = Fixture::connected().await;
        let docs = fixture.server.local("/home/tester/docs");
        std::fs::create_dir_all(&docs).unwrap();
        for name in [
            "fine.txt",
            "evil\u{202E}gnp.exe",
            "bell\u{7}",
            "line\nbreak",
        ] {
            std::fs::write(docs.join(name), "x").unwrap();
        }

        let listing = list(
            &fixture.pool,
            Fixture::at("/home/tester/docs"),
            SortKey::default(),
        )
        .await;

        let names: Vec<String> = listing
            .entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["..", "fine.txt"]);
        assert_eq!(listing.error, Some(ReadError::EntriesLeftOut { count: 3 }));
    }
    #[tokio::test]
    async fn without_a_connection_the_listing_says_so() {
        let fixture = Fixture::trusted();

        let listing = list(&fixture.pool, Fixture::at("/"), SortKey::default()).await;

        assert!(matches!(
            listing.error,
            Some(ReadError::RemoteDisconnected { .. })
        ));
    }
}
