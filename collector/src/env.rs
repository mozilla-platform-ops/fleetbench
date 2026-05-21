use crate::schema::{CpuCounters, LoadSample};

const KIND_LINUX_PROC_STAT: &str = "linux_proc_stat";

pub fn sample_load() -> LoadSample {
    let (load_1, load_5, load_15) = loadavg_sample();
    LoadSample {
        cpu_counters: cpu_counters_snapshot(),
        load_1,
        load_5,
        load_15,
        processor_queue_length: None,
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn cpu_counters_snapshot() -> Option<CpuCounters> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    parse_proc_stat_counters(&s)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn cpu_counters_snapshot() -> Option<CpuCounters> {
    None
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn loadavg_sample() -> (Option<f64>, Option<f64>, Option<f64>) {
    match std::fs::read_to_string("/proc/loadavg")
        .ok()
        .as_deref()
        .and_then(parse_proc_loadavg)
    {
        Some((a, b, c)) => (Some(a), Some(b), Some(c)),
        None => (None, None, None),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn loadavg_sample() -> (Option<f64>, Option<f64>, Option<f64>) {
    (None, None, None)
}

fn parse_proc_stat_counters(contents: &str) -> Option<CpuCounters> {
    let line = contents.lines().next()?;
    let mut parts = line.split_ascii_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    // user, nice, system, idle, iowait, irq, softirq, steal, guest, guest_nice
    let values: Vec<u64> = parts.filter_map(|p| p.parse::<u64>().ok()).collect();
    if values.len() < 4 {
        return None;
    }
    let idle = values[3];
    let iowait = values.get(4).copied();
    let total: u64 = values.iter().sum();
    Some(CpuCounters {
        kind: KIND_LINUX_PROC_STAT.into(),
        idle_units: idle,
        iowait_units: iowait,
        total_units: total,
    })
}

fn parse_proc_loadavg(contents: &str) -> Option<(f64, f64, f64)> {
    let line = contents.lines().next()?;
    let mut parts = line.split_ascii_whitespace();
    let a: f64 = parts.next()?.parse().ok()?;
    let b: f64 = parts.next()?.parse().ok()?;
    let c: f64 = parts.next()?.parse().ok()?;
    Some((a, b, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_counters() {
        let s = "cpu  100 0 50 1000 10 0 5 0 0 0\ncpu0 50 0 25 500 5 0 2 0 0 0\n";
        let got = parse_proc_stat_counters(s).unwrap();
        assert_eq!(got.kind, "linux_proc_stat");
        assert_eq!(got.idle_units, 1000);
        assert_eq!(got.iowait_units, Some(10));
        // total = 100+0+50+1000+10+0+5+0+0+0 = 1165
        assert_eq!(got.total_units, 1165);
    }

    #[test]
    fn parses_proc_stat_without_iowait() {
        // Old kernels may not have iowait; require at least idle (4 fields).
        let s = "cpu 100 0 50 1000\n";
        let got = parse_proc_stat_counters(s).unwrap();
        assert_eq!(got.idle_units, 1000);
        assert!(got.iowait_units.is_none());
        assert_eq!(got.total_units, 1150);
    }

    #[test]
    fn parse_counters_rejects_unexpected_first_line() {
        assert!(parse_proc_stat_counters("intr 1 2 3\n").is_none());
        assert!(parse_proc_stat_counters("").is_none());
    }

    #[test]
    fn parse_counters_handles_short_lines() {
        assert!(parse_proc_stat_counters("cpu 1 2 3\n").is_none());
    }

    #[test]
    fn differencing_counters_recovers_cpu_percent() {
        // Analysis-layer math: given two snapshots, recover the busy fraction
        // over the window between them.
        let a = parse_proc_stat_counters("cpu 100 0 100 1000 0 0 0 0 0 0\n").unwrap();
        let b = parse_proc_stat_counters("cpu 200 0 200 1200 0 0 0 0 0 0\n").unwrap();
        // a: total=1200, idle=1000.  b: total=1600, idle=1200.
        let total_d = b.total_units - a.total_units; // 400
        let idle_d = b.idle_units - a.idle_units; // 200
        let busy_pct = (total_d - idle_d) as f64 / total_d as f64 * 100.0;
        assert!((busy_pct - 50.0).abs() < 1e-9, "got {busy_pct}");
    }

    #[test]
    fn parses_proc_loadavg() {
        let (a, b, c) = parse_proc_loadavg("0.42 0.55 0.61 1/234 56789\n").unwrap();
        assert!((a - 0.42).abs() < 1e-9);
        assert!((b - 0.55).abs() < 1e-9);
        assert!((c - 0.61).abs() < 1e-9);
    }

    #[test]
    fn parse_loadavg_rejects_short_lines() {
        assert!(parse_proc_loadavg("0.42 0.55\n").is_none());
        assert!(parse_proc_loadavg("").is_none());
    }

    #[test]
    fn parse_loadavg_rejects_garbage() {
        assert!(parse_proc_loadavg("a b c 1/1 1\n").is_none());
    }

    #[test]
    fn sample_load_platform_expectations() {
        let s = sample_load();
        assert!(s.processor_queue_length.is_none());
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let c = s.cpu_counters.expect("linux populates cpu_counters");
            assert_eq!(c.kind, "linux_proc_stat");
            assert!(c.total_units > c.idle_units);
            assert!(s.load_1.is_some());
            assert!(s.load_5.is_some());
            assert!(s.load_15.is_some());
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            assert!(s.cpu_counters.is_none());
            assert!(s.load_1.is_none());
            assert!(s.load_5.is_none());
            assert!(s.load_15.is_none());
        }
    }
}
