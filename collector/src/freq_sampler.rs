//! Background CPU-frequency sampler for time-bounded torture runs.
//!
//! Spawns a thread that wakes ~1Hz, samples per-core CPU frequencies, and
//! records them with a wall-clock offset from sampler start. Pairs with
//! `--duration` to turn a sustained load into evidence of thermal throttling
//! (boost-clock samples that decay toward base-clock over time).
//!
//! Platform backends:
//!   - **Linux**: sysinfo (returns real per-core MHz from `/proc/cpuinfo`).
//!   - **Windows**: PDH counter `\Processor Information(*)\% Processor
//!     Performance`, multiplied by the per-core base from sysinfo. sysinfo
//!     alone on Windows returns the *base* frequency every sample, which is
//!     useless for throttle detection (verified on i5-1340P). See
//!     `freq_windows.rs` and bead fleetbench-8ez for context.
//!   - **macOS** (esp. Apple Silicon): sysinfo does not expose per-core
//!     frequency and returns a placeholder. Samples still emit, but the
//!     values are not meaningful. Acceptable because fleetbench's real
//!     thermal-throttle targets are Linux and Windows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sysinfo::System;

use crate::schema::FrequencySample;

#[cfg(target_os = "windows")]
use crate::freq_windows::PdhFreqBackend;

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

        let handle = thread::spawn(move || sampler_loop(start, interval, stop_thread));

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

fn sampler_loop(start: Instant, interval: Duration, stop: Arc<AtomicBool>) -> Vec<FrequencySample> {
    let mut backend = make_backend();
    let mut samples: Vec<FrequencySample> = Vec::new();
    let mut next_at = start;

    loop {
        let now = Instant::now();
        if now >= next_at {
            let per_core_mhz = backend.sample();
            let t_offset_seconds = now.duration_since(start).as_secs_f64();
            let mean_mhz = if per_core_mhz.is_empty() {
                0
            } else {
                (per_core_mhz.iter().map(|&m| m as u64).sum::<u64>()
                    / per_core_mhz.len() as u64) as u32
            };
            samples.push(FrequencySample { t_offset_seconds, per_core_mhz, mean_mhz });
            next_at += interval;
        }

        if stop.load(Ordering::Relaxed) {
            return samples;
        }
        thread::sleep(POLL_TICK);
    }
}

/// Per-iteration sampler. Trait-like, but using an enum to avoid Box<dyn ...>
/// in the hot loop. Variants are cfg-selected at compile time.
enum Backend {
    Sysinfo(System),
    #[cfg(target_os = "windows")]
    Pdh {
        pdh: PdhFreqBackend,
        // Fallback values in case PDH initialization itself failed at startup.
        base_mhz: Vec<u32>,
    },
}

impl Backend {
    fn sample(&mut self) -> Vec<u32> {
        match self {
            Backend::Sysinfo(sys) => {
                sys.refresh_cpu_frequency();
                sys.cpus().iter().map(|c| c.frequency() as u32).collect()
            }
            #[cfg(target_os = "windows")]
            Backend::Pdh { pdh, base_mhz } => {
                let out = pdh.sample();
                if out.is_empty() { base_mhz.clone() } else { out }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn make_backend() -> Backend {
    // Read per-core base frequencies from sysinfo once. On Windows sysinfo
    // *does* read the base correctly — the bug is that it never moves under
    // load. We capture it here as the multiplier for PDH percentages.
    let mut sys = System::new();
    sys.refresh_cpu_frequency();
    let base_mhz: Vec<u32> = sys.cpus().iter().map(|c| c.frequency() as u32).collect();

    match PdhFreqBackend::new(base_mhz.clone()) {
        Ok(pdh) => Backend::Pdh { pdh, base_mhz },
        Err(_e) => {
            // PDH failed to initialize. Fall back to the (broken) sysinfo
            // path rather than producing no samples; the operator can see
            // from the constant values that PDH didn't work.
            Backend::Sysinfo(sys)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn make_backend() -> Backend {
    Backend::Sysinfo(System::new())
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
