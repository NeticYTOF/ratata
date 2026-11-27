use crate::model::{JobProgress, JobState};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Clone)]
pub struct ProgressTracker {
    inner: Arc<TrackerInner>,
}

struct TrackerInner {
    tx: mpsc::UnboundedSender<JobProgress>,
    id: usize,
    label: String,
    total: Mutex<Option<u64>>,
    downloaded: AtomicU64,
    state: Mutex<JobState>,
}

impl ProgressTracker {
    pub fn new(
        tx: mpsc::UnboundedSender<JobProgress>,
        id: usize,
        label: String,
        total: Option<u64>,
    ) -> Self {
        Self {
            inner: Arc::new(TrackerInner {
                tx,
                id,
                label,
                total: Mutex::new(total),
                downloaded: AtomicU64::new(0),
                state: Mutex::new(JobState::Queued),
            }),
        }
    }

    pub fn add(&self, bytes: u64) {
        self.inner.downloaded.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn downloaded(&self) -> u64 {
        self.inner.downloaded.load(Ordering::Relaxed)
    }

    pub fn set_total(&self, total: u64) {
        *self.inner.total.lock() = Some(total);
        self.emit(0.0);
    }

    pub fn set_state(&self, state: JobState) {
        *self.inner.state.lock() = state;
        self.emit(0.0);
    }

    fn emit(&self, speed: f64) {
        let inner = &self.inner;
        let _ = inner.tx.send(JobProgress {
            id: inner.id,
            label: inner.label.clone(),
            downloaded: inner.downloaded.load(Ordering::Relaxed),
            total: *inner.total.lock(),
            speed,
            state: *inner.state.lock(),
            error: None,
        });
    }

    pub fn spawn(&self) -> JoinHandle<()> {
        let tracker = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(90));
            let mut previous = tracker.downloaded();
            let mut last = Instant::now();
            let mut smoothed = 0.0;
            loop {
                tick.tick().await;
                let now_bytes = tracker.downloaded();
                let now = Instant::now();
                let elapsed = now.duration_since(last).as_secs_f64().max(0.001);
                let instant = (now_bytes.saturating_sub(previous)) as f64 / elapsed;
                smoothed = smoothed * 0.65 + instant * 0.35;
                previous = now_bytes;
                last = now;
                tracker.emit(smoothed);
                if matches!(
                    *tracker.inner.state.lock(),
                    JobState::Done | JobState::Failed | JobState::Cancelled
                ) {
                    break;
                }
            }
        })
    }
}
