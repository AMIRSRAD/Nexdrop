use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::time::sleep;

pub struct SpeedCalculator {
    samples: VecDeque<(Instant, u64)>,
    window_duration: Duration,
    total_bytes_downloaded: u64,
}

impl SpeedCalculator {
    pub fn new(window_secs: u64) -> Self {
        Self {
            samples: VecDeque::new(),
            window_duration: Duration::from_secs(window_secs),
            total_bytes_downloaded: 0,
        }
    }

    pub fn record_bytes(&mut self, bytes: u64) {
        let now = Instant::now();
        self.samples.push_back((now, bytes));
        self.total_bytes_downloaded += bytes;
        self.prune_old_samples(now);
    }

    fn prune_old_samples(&mut self, now: Instant) {
        while let Some(&(time, _)) = self.samples.front() {
            if now.duration_since(time) > self.window_duration {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn current_speed_bps(&mut self) -> u64 {
        let now = Instant::now();
        self.prune_old_samples(now);

        if self.samples.len() < 2 {
            return 0;
        }

        let first = self.samples.front().unwrap().0;
        let last = self.samples.back().unwrap().0;
        let elapsed = last.duration_since(first).as_secs_f64();

        if elapsed < 0.1 {
            return 0;
        }

        let bytes_in_window: u64 = self.samples.iter().map(|(_, b)| *b).sum();
        ((bytes_in_window as f64) / elapsed) as u64
    }

    pub fn calculate_eta(remaining_bytes: u64, speed_bps: u64) -> Option<u64> {
        if speed_bps == 0 || remaining_bytes == 0 {
            None
        } else {
            Some(remaining_bytes / speed_bps)
        }
    }
}

/// A token-bucket rate limiter that many tasks can share (one per download
/// plus one global). `0` means unlimited.
pub struct RateLimiter {
    rate_bps: AtomicU64,
    state: Mutex<BucketState>,
}

struct BucketState {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    pub fn new(rate_bps: u64) -> Self {
        Self {
            rate_bps: AtomicU64::new(rate_bps),
            state: Mutex::new(BucketState { tokens: 0.0, last: Instant::now() }),
        }
    }

    pub fn set_rate(&self, rate_bps: u64) {
        self.rate_bps.store(rate_bps, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate_bps.load(Ordering::Relaxed)
    }

    /// How long the caller must wait before it may use `bytes` more bandwidth.
    /// Spending ahead of the allowance is allowed (it becomes a wait), so a
    /// chunk larger than one second's worth of tokens still gets through.
    fn reserve(&self, bytes: u64) -> Duration {
        let rate = self.rate() as f64;
        if rate <= 0.0 {
            return Duration::ZERO;
        }
        let mut st = self.state.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(st.last).as_secs_f64();
        st.last = now;
        // Allow at most a quarter second of burst.
        st.tokens = (st.tokens + elapsed * rate).min(rate * 0.25);
        st.tokens -= bytes as f64;
        if st.tokens < 0.0 {
            Duration::from_secs_f64(-st.tokens / rate)
        } else {
            Duration::ZERO
        }
    }

    pub async fn acquire(&self, bytes: u64) {
        let wait = self.reserve(bytes);
        if !wait.is_zero() {
            sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_never_waits() {
        let l = RateLimiter::new(0);
        assert_eq!(l.reserve(10_000_000), Duration::ZERO);
    }

    #[test]
    fn limited_rate_produces_proportional_waits() {
        let l = RateLimiter::new(1_000_000); // 1 MB/s
        // The first request is mostly "free" (burst), a big second one must wait ~1s.
        let _ = l.reserve(100_000);
        let wait = l.reserve(1_000_000);
        assert!(wait > Duration::from_millis(800) && wait < Duration::from_millis(1200), "{wait:?}");
    }

    #[test]
    fn rate_can_change_live() {
        let l = RateLimiter::new(1000);
        assert_eq!(l.rate(), 1000);
        l.set_rate(0);
        assert_eq!(l.reserve(1 << 30), Duration::ZERO);
    }
}
