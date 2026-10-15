use std::collections::VecDeque;
use std::time::{Duration, Instant};
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

pub struct SpeedLimiter {
    limit_bps: u64,
    last_check: Instant,
    bytes_in_period: u64,
}

impl SpeedLimiter {
    pub fn new(limit_bps: u64) -> Self {
        Self {
            limit_bps,
            last_check: Instant::now(),
            bytes_in_period: 0,
        }
    }

    pub async fn throttle(&mut self, bytes: usize) {
        if self.limit_bps == 0 {
            return;
        }

        self.bytes_in_period += bytes as u64;
        let elapsed = self.last_check.elapsed();

        if elapsed >= Duration::from_millis(500) {
            let expected_time = Duration::from_secs_f64(self.bytes_in_period as f64 / self.limit_bps as f64);
            if expected_time > elapsed {
                sleep(expected_time - elapsed).await;
            }
            self.last_check = Instant::now();
            self.bytes_in_period = 0;
        }
    }
}
