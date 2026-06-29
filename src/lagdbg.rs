//! Optional timing instrumentation for the data()/lock hot path.
//!
//! Compiled in only with the `lagdbg` feature. When enabled, [`Timing::sample`]
//! is called once per game-loop tick and returns a formatted line roughly once
//! per second.

use std::time::{Duration, Instant};

pub struct Timing {
    samples: u32,
    data_accum: Duration,
    lock_accum: Duration,
    last: Instant,
}

impl Timing {
    pub fn new() -> Self {
        Self {
            samples: 0,
            data_accum: Duration::ZERO,
            lock_accum: Duration::ZERO,
            last: Instant::now(),
        }
    }

    /// Accumulate one tick's measurements. Returns a `[lagdbg]` summary line
    /// (without the player count) about once per second, otherwise `None`.
    pub fn sample(&mut self, lock_wait: Duration, data_elapsed: Duration) -> Option<String> {
        self.samples += 1;
        self.data_accum += data_elapsed;
        self.lock_accum += lock_wait;

        if self.last.elapsed() < Duration::from_secs(1) {
            return None;
        }

        let n = self.samples.max(1) as f32;
        let line = format!(
            "[lagdbg] samples/s={} avg_data()={:.2}ms avg_lock_wait={:.3}ms",
            self.samples,
            self.data_accum.as_secs_f32() * 1000.0 / n,
            self.lock_accum.as_secs_f32() * 1000.0 / n,
        );

        self.samples = 0;
        self.data_accum = Duration::ZERO;
        self.lock_accum = Duration::ZERO;
        self.last = Instant::now();

        Some(line)
    }
}
