//! Background CPU-frequency sampler for time-bounded torture runs.
//!
//! Spawns a thread that wakes ~1Hz, refreshes per-core CPU frequencies via
//! sysinfo, and records them with a wall-clock offset from sampler start.
//! Pairs with `--duration` to turn a sustained load into evidence of thermal
//! throttling (boost-clock samples that decay toward base-clock over time).
//!
//! Platform notes:
//!   - Linux, Windows: sysinfo returns real per-core MHz. These are the
//!     primary fleetbench targets and where this signal matters.
//!   - macOS (esp. Apple Silicon): sysinfo does not expose per-core frequency
//!     and returns a placeholder (typically 0/low). Samples will still emit,
//!     but `per_core_mhz` and `mean_mhz` are not meaningful here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sysinfo::System;

use crate::schema::FrequencySample;

const POLL_TICK: Duration = Duration::from_millis(50);

pub struct Sampler {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Vec<FrequencySample>>>,
}

impl Sampler {
    /// Start sampling. The first sample is taken immediately; subsequent
    /// samples follow at `interval` cadence.
    pub fn start(interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let start = Instant::now();

        let handle = thread::spawn(move || {
            let mut sys = System::new();
            let mut samples: Vec<FrequencySample> = Vec::new();
            let mut next_at = start;

            loop {
                let now = Instant::now();
                if now >= next_at {
                    sys.refresh_cpu_frequency();
                    let t_offset_seconds = now.duration_since(start).as_secs_f64();
                    let per_core_mhz: Vec<u32> =
                        sys.cpus().iter().map(|c| c.frequency() as u32).collect();
                    let mean_mhz = if per_core_mhz.is_empty() {
                        0
                    } else {
                        (per_core_mhz.iter().map(|&m| m as u64).sum::<u64>()
                            / per_core_mhz.len() as u64) as u32
                    };
                    samples.push(FrequencySample { t_offset_seconds, per_core_mhz, mean_mhz });
                    next_at += interval;
                }

                if stop_thread.load(Ordering::Relaxed) {
                    return samples;
                }
                thread::sleep(POLL_TICK);
            }
        });

        Self { stop, handle: Some(handle) }
    }

    pub fn stop(mut self) -> Vec<FrequencySample> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle
            .take()
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_at_least_one_sample_on_immediate_stop() {
        let s = Sampler::start(Duration::from_millis(100));
        thread::sleep(Duration::from_millis(20));
        let samples = s.stop();
        assert!(!samples.is_empty(), "expected at least the initial sample");
        let first = &samples[0];
        assert!(first.t_offset_seconds >= 0.0);
        // per_core_mhz may be empty on some CI hosts where sysinfo cannot
        // read frequency, but the sample itself must exist.
    }

    #[test]
    fn captures_multiple_samples_over_time() {
        let s = Sampler::start(Duration::from_millis(60));
        thread::sleep(Duration::from_millis(250));
        let samples = s.stop();
        assert!(samples.len() >= 2, "expected multiple samples, got {}", samples.len());
        for w in samples.windows(2) {
            assert!(w[1].t_offset_seconds >= w[0].t_offset_seconds);
        }
    }
}
