//! Watching one folder for changes made by other programs.

use std::path::Path;
use std::time::Duration;

use futures::Stream;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
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
    _watcher: Option<RecommendedWatcher>,
}

impl FolderWatch {
    /// Nothing when the system gives no native watcher for the folder; there
    /// is no polling instead, the manual reload remains.
    fn start(folder: &Path) -> Option<Self> {
        let (sender, raw) = mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(move |received: notify::Result<Event>| {
            // Reading the folder is no change. An error means events may have
            // been lost, so it counts as one.
            let is_change =
                received.map_or(true, |event| !matches!(event.kind, EventKind::Access(_)));
            if is_change {
                // The receiver is gone once the watch is dropped.
                let _ = sender.send(());
            }
        })
        .ok()?;
        watcher.watch(folder, RecursiveMode::NonRecursive).ok()?;
        Some(Self {
            raw,
            _watcher: Some(watcher),
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
    futures::stream::unfold(FolderWatch::start(folder), |started| async move {
        let mut watch = started?;
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
    use futures::StreamExt as _;
    use tokio::sync::mpsc::error::TryRecvError;

    fn fed_by_hand() -> (mpsc::UnboundedSender<()>, FolderWatch) {
        let (sender, raw) = mpsc::unbounded_channel();
        (
            sender,
            FolderWatch {
                raw,
                _watcher: None,
            },
        )
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_events_is_one_change() {
        let (sender, mut watch) = fed_by_hand();
        for _ in 0..20 {
            sender.send(()).unwrap();
        }

        let started = Instant::now();
        assert_eq!(watch.next_change().await, Some(()));

        assert_eq!(started.elapsed(), QUIET_FOR);
        assert_eq!(watch.raw.try_recv(), Err(TryRecvError::Empty));
    }

    #[tokio::test(start_paused = true)]
    async fn events_that_never_stop_are_still_reported() {
        let (sender, mut watch) = fed_by_hand();
        let feeder = tokio::spawn(async move {
            loop {
                sender.send(()).unwrap();
                tokio::time::sleep(QUIET_FOR / 3).await;
            }
        });

        let started = Instant::now();
        assert_eq!(watch.next_change().await, Some(()));
        feeder.abort();

        assert!(started.elapsed() >= REPORT_AT_LEAST_EVERY);
        assert!(started.elapsed() < REPORT_AT_LEAST_EVERY + QUIET_FOR);
    }

    #[tokio::test]
    async fn a_write_in_the_watched_folder_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let mut changes = Box::pin(folder_changes(root.path()));

        std::fs::write(root.path().join("a"), b"x").unwrap();

        let first = tokio::time::timeout(Duration::from_secs(10), changes.next()).await;
        assert!(matches!(first, Ok(Some(()))), "no change was reported");
    }
}
