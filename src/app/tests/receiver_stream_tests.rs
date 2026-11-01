use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use futures::Stream;

use crate::app::*;

struct CountingWaker(AtomicUsize);

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn poll_once<S: Stream + Unpin>(
    stream: &mut S,
    counting: &Arc<CountingWaker>,
) -> Poll<Option<S::Item>> {
    let waker = Waker::from(Arc::clone(counting));
    Pin::new(stream).poll_next(&mut Context::from_waker(&waker))
}

#[test]
fn an_empty_channel_is_pending_without_waking_anyone() {
    let (sender, receiver) = mpsc::unbounded_channel();
    let wakes = Arc::new(CountingWaker(AtomicUsize::new(0)));
    let mut stream = Box::pin(receiver_stream(Arc::new(tokio::sync::Mutex::new(receiver))));

    assert!(poll_once(&mut stream, &wakes).is_pending());
    assert!(poll_once(&mut stream, &wakes).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0, "no busy polling");

    sender
        .send(fs::transfer::Tick { done: 1, total: 2 })
        .unwrap();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1, "a tick wakes the task");
    assert_eq!(
        poll_once(&mut stream, &wakes),
        Poll::Ready(Some(fs::transfer::Tick { done: 1, total: 2 }))
    );

    drop(sender);
    assert_eq!(poll_once(&mut stream, &wakes), Poll::Ready(None));
}
