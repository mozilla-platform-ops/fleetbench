//! Windows-specific CPU frequency backend via PDH.
//!
//! sysinfo on Windows returns the **nominal base** frequency per core, which
//! is identical across samples and useless for thermal-throttle detection
//! (verified on i5-1340P: every sample showed [1900x8, 1400x8] under
//! sustained load, never moving). To get the actual running frequency we
//! read `\Processor Information(*)\% Processor Performance`, which returns
//! a percentage relative to nominal (100 = base, 200 = boosting 2x, 79 =
//! throttled to ~80% of base), and multiply by the per-core base value
//! sysinfo gives us at startup.
//!
//! Trap: `\Processor Information(*)\Processor Frequency` (without the `%`)
//! returns the base, not the actual. Do not use it. That's the same trap
//! sysinfo's per-core frequency API falls into.

#![cfg(target_os = "windows")]

use std::ptr;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
};

const COUNTER_PATH: &str = r"\Processor Information(*)\% Processor Performance";

pub struct PdhFreqBackend {
    query: isize,
    counter: isize,
    /// Per-core base frequency in MHz, indexed by logical processor. Captured
    /// once at start via sysinfo (which gets the base correctly). PDH returns
    /// percentages relative to these.
    base_mhz: Vec<u32>,
}

impl PdhFreqBackend {
    /// Open the PDH query, add the counter, and prime it. PDH percentage
    /// counters need an initial collect before the first formatted read
    /// returns meaningful values.
    pub fn new(base_mhz: Vec<u32>) -> Result<Self, String> {
        let mut query: isize = 0;
        let status = unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) };
        if status != ERROR_SUCCESS {
            return Err(format!("PdhOpenQueryW failed: 0x{:08x}", status));
        }

        let path: Vec<u16> = COUNTER_PATH
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut counter: isize = 0;
        let status =
            unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
        if status != ERROR_SUCCESS {
            unsafe {
                PdhCloseQuery(query);
            }
            return Err(format!("PdhAddEnglishCounterW failed: 0x{:08x}", status));
        }

        // Prime the counter — `% Processor Performance` needs a non-trivial
        // delta window before it returns meaningful per-core values. Without
        // this sleep the first user sample reads zeros on most cores because
        // no perf-state transitions were observed between the two collects.
        // Verified on i5-1340P: without sleep, t=0 sample shows [4243,0,0,0,
        // 0,4257,4261,0,...]; with sleep, all cores report.
        unsafe {
            PdhCollectQueryData(query);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));

        Ok(Self { query, counter, base_mhz })
    }

    /// Return per-core actual frequency in MHz. On any PDH failure, falls
    /// back to the base frequencies (degrades gracefully — caller sees a
    /// flat series rather than a panic).
    pub fn sample(&self) -> Vec<u32> {
        unsafe {
            PdhCollectQueryData(self.query);
        }

        // First call discovers required buffer size.
        let mut buffer_size: u32 = 0;
        let mut item_count: u32 = 0;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                ptr::null_mut(),
            )
        };
        // Expect PDH_MORE_DATA here; anything else is a failure.
        if status != PDH_MORE_DATA {
            return self.base_mhz.clone();
        }

        let mut buffer = vec![0u8; buffer_size as usize];
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_size,
                &mut item_count,
                buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W,
            )
        };
        if status != ERROR_SUCCESS {
            return self.base_mhz.clone();
        }

        let items = unsafe {
            std::slice::from_raw_parts(
                buffer.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W,
                item_count as usize,
            )
        };

        let mut out: Vec<u32> = self.base_mhz.clone();
        for item in items {
            let name = unsafe { wide_ptr_to_string(item.szName) };
            // Instance names: "_Total" (skip), "0,0", "0,1", ... "<group>,<core>".
            // We index by the processor number after the comma.
            if name == "_Total" {
                continue;
            }
            let core_idx: Option<usize> = name
                .rsplit(',')
                .next()
                .and_then(|s| s.parse::<usize>().ok());
            let Some(idx) = core_idx else { continue };
            if idx >= out.len() {
                continue;
            }
            let pct = unsafe { item.FmtValue.Anonymous.doubleValue };
            // Guard against PDH returning negative or absurd values.
            let pct = pct.max(0.0).min(10_000.0);
            let base = self.base_mhz.get(idx).copied().unwrap_or(0) as f64;
            out[idx] = (base * pct / 100.0).round() as u32;
        }
        out
    }
}

impl Drop for PdhFreqBackend {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

// SAFETY: PDH handles are thread-safe per Microsoft docs (one query may be
// used from multiple threads). We only use them from the sampler thread
// anyway, but Send is required to move the backend into the spawned thread.
unsafe impl Send for PdhFreqBackend {}

unsafe fn wide_ptr_to_string(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    String::from_utf16_lossy(slice)
}
