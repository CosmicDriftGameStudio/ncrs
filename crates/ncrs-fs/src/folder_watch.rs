//! Watching one folder for changes made by other programs.

use std::path::Path;
use std::time::Duration;

use futures::Stream;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::time::{timeout_at, Instant};

/// How long the folder must stay quiet before a change is reported.
const QUIET_FOR: Duration = Duration::from_millis(300);
/// A stream of events that never goes quiet, such as a running download, is
/// reported at least this often.
const REPORT_AT_LEAST_EVERY: Duration = Duration::from_secs(2);

/// The watcher and the events it has seen. Dropping it stops the watching.
struct FolderWatch {
    raw: mpsc::UnboundedReceiver<()>,
    _watcher: RecommendedWatcher,
}

impl FolderWatch {
    /// Nothing when the system gives no native watcher for the folder; there
    /// is no polling instead, the manual reload remains.
    fn start(folder: &Path) -> Option<Self> {
        let (sender, raw) = mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            // Reading the folder is no change. An error means events may have
            // been lost, so it counts as one.
            let is_change = event.map_or(true, |event| !matches!(event.kind, EventKind::Access(_)));
            if is_change {
                // The receiver is gone once the watch is dropped.
                let _ = sender.send(());
            }
        })
        .ok()?;
        watcher.watch(folder, RecursiveMode::NonRecursive).ok()?;
        Some(Self {
            raw,
            _watcher: watcher,
        })
    }

    /// Waits for a burst of events to end and counts it as one change.
    async fn next_change(&mut self) -> Option<()> {
        self.raw.recv().await?;
        let deadline = Instant::now() + REPORT_AT_LEAST_EVERY;
        loop {
            let quiet_until = (Instant::now() + QUIET_FOR).min(deadline);
            match timeout_at(quiet_until, self.raw.recv()).await {
                Ok(Some(())) => {}
                Ok(None) | Err(_) => return Some(()),
            }
        }
    }
}

/// One item per change in `folder` (not its subfolders), after the burst of
/// events has settled. Ends at once when the folder cannot be watched.
pub fn folder_changes(folder: &Path) -> impl Stream<Item = ()> + Send + 'static {
    futures::stream::unfold(FolderWatch::start(folder), |watch| async move {
        let mut watch = watch?;
        watch.next_change().await?;
        Some(((), Some(watch)))
    })
}

#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    const GENEROUS: Duration = Duration::from_secs(10);

    #[tokio::test]
    async fn a_burst_of_writes_is_one_change() {
        let root = tempfile::tempdir().unwrap();
        let mut changes = Box::pin(folder_changes(root.path()));

        for n in 0..20 {
            std::fs::write(root.path().join(format!("f{n}")), b"x").unwrap();
        }

        let first = tokio::time::timeout(GENEROUS, changes.next()).await;
        assert!(matches!(first, Ok(Some(()))), "no change was reported");

        // Nothing more is queued for the same burst.
        let second = tokio::time::timeout(QUIET_FOR * 4, changes.next()).await;
        assert!(second.is_err(), "the burst was reported twice");
    }

    #[tokio::test]
    async fn a_later_change_is_reported_again() {
        let root = tempfile::tempdir().unwrap();
        let mut changes = Box::pin(folder_changes(root.path()));

        std::fs::write(root.path().join("a"), b"x").unwrap();
        assert!(tokio::time::timeout(GENEROUS, changes.next()).await.is_ok());

        std::fs::remove_file(root.path().join("a")).unwrap();
        assert!(matches!(
            tokio::time::timeout(GENEROUS, changes.next()).await,
            Ok(Some(()))
        ));
    }
}
