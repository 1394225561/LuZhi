use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::Arc;

#[derive(Debug)]
pub struct MediaSender<T> {
    inner: SyncSender<T>,
    dropped: Arc<AtomicU64>,
    /// Source identifier for drop logging (e.g. "system", "mic", "video").
    source: &'static str,
}

#[derive(Debug)]
pub struct MediaReceiver<T> {
    inner: Receiver<T>,
    dropped: Arc<AtomicU64>,
}

pub fn bounded_media_channel<T>(
    capacity: usize,
    source: &'static str,
) -> (MediaSender<T>, MediaReceiver<T>) {
    assert!(
        capacity > 0,
        "media channel capacity must be greater than zero"
    );
    let (inner_sender, inner_receiver) = sync_channel(capacity);
    let dropped = Arc::new(AtomicU64::new(0));

    (
        MediaSender {
            inner: inner_sender,
            dropped: dropped.clone(),
            source,
        },
        MediaReceiver {
            inner: inner_receiver,
            dropped,
        },
    )
}

impl<T> Clone for MediaSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            dropped: self.dropped.clone(),
            source: self.source,
        }
    }
}

impl<T> MediaSender<T> {
    pub fn try_send_drop_newest(&self, item: T) -> bool {
        match self.inner.try_send(item) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                let count = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                // Log every 100th drop to avoid log spam, and always log the first.
                if count == 1 || count % 100 == 0 {
                    eprintln!("警告: {} 媒体通道丢弃 (累计 {} 次)", self.source, count);
                }
                false
            }
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl<T> MediaReceiver<T> {
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.inner.try_recv()
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_newest_when_full_without_blocking() {
        let (sender, receiver) = bounded_media_channel::<u32>(1, "test");

        assert!(sender.try_send_drop_newest(1));
        assert!(!sender.try_send_drop_newest(2));

        assert_eq!(receiver.dropped_count(), 1);
        assert_eq!(receiver.try_recv().unwrap(), 1);
    }

    #[test]
    fn reports_disconnected_receiver_as_drop() {
        let (sender, receiver) = bounded_media_channel::<u32>(1, "test");
        drop(receiver);

        assert!(!sender.try_send_drop_newest(1));
        assert_eq!(sender.dropped_count(), 1);
    }
}
