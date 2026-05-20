use crate::schema::LoadSample;

const SAMPLE_INTERVAL_MS: u64 = 100;

pub fn sample_load() -> LoadSample {
    LoadSample {
        cpu_percent: cpu_percent_sample(),
        load_1: None,
        load_5: None,
        load_15: None,
        processor_queue_length: None,
    }
}

#[cfg(target_os = "linux")]
fn cpu_percent_sample() -> Option<f64> {
    let a = read_proc_stat_cpu()?;
    std::thread::sleep(std::time::Duration::from_millis(SAMPLE_INTERVAL_MS));
    let b = read_proc_stat_cpu()?;
    cpu_percent_from_samples(a, b)
}

#[cfg(not(target_os = "linux"))]
fn cpu_percent_sample() -> Option<f64> {
    let _ = SAMPLE_INTERVAL_MS;
    None
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct ProcStatCpu {
    idle: u64,
    total: u64,
}

#[cfg(target_os = "linux")]
fn read_proc_stat_cpu() -> Option<ProcStatCpu> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    parse_proc_stat_cpu(&s)
}

fn parse_proc_stat_cpu(contents: &str) -> Option<ProcStatCpu> {
    let line = contents.lines().next()?;
    let mut parts = line.split_ascii_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    let values: Vec<u64> = parts.filter_map(|p| p.parse::<u64>().ok()).collect();
    // user, nice, system, idle, iowait, irq, softirq, steal, guest, guest_nice
    if values.len() < 4 {
        return None;
    }
    let idle = values[3];
    let iowait = values.get(4).copied().unwrap_or(0);
    let idle_all = idle + iowait;
    let total: u64 = values.iter().sum();
    Some(ProcStatCpu { idle: idle_all, total })
}

fn cpu_percent_from_samples(a: ProcStatCpu, b: ProcStatCpu) -> Option<f64> {
    let total_d = b.total.checked_sub(a.total)?;
    let idle_d = b.idle.checked_sub(a.idle)?;
    if total_d == 0 {
        return None;
    }
    let busy_d = total_d.saturating_sub(idle_d);
    Some((busy_d as f64 / total_d as f64) * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_stat_first_line() {
        let s = "cpu  100 0 50 1000 10 0 5 0 0 0\ncpu0 50 0 25 500 5 0 2 0 0 0\n";
        let got = parse_proc_stat_cpu(s).unwrap();
        // idle = 1000, iowait = 10 -> idle_all = 1010
        // total = 100+0+50+1000+10+0+5+0+0+0 = 1165
        assert_eq!(got, ProcStatCpu { idle: 1010, total: 1165 });
    }

    #[test]
    fn parse_rejects_unexpected_first_line() {
        assert!(parse_proc_stat_cpu("intr 1 2 3\n").is_none());
        assert!(parse_proc_stat_cpu("").is_none());
    }

    #[test]
    fn parse_handles_short_lines() {
        // Three values is below the minimum (need at least idle field)
        assert!(parse_proc_stat_cpu("cpu 1 2 3\n").is_none());
    }

    #[test]
    fn cpu_percent_computes_expected_value() {
        // 1000 total jiffies elapsed, 200 idle -> 80% busy
        let a = ProcStatCpu { idle: 500, total: 5000 };
        let b = ProcStatCpu { idle: 700, total: 6000 };
        let p = cpu_percent_from_samples(a, b).unwrap();
        assert!((p - 80.0).abs() < 1e-9, "got {p}");
    }

    #[test]
    fn cpu_percent_zero_when_fully_idle() {
        let a = ProcStatCpu { idle: 100, total: 1000 };
        let b = ProcStatCpu { idle: 200, total: 1100 };
        let p = cpu_percent_from_samples(a, b).unwrap();
        assert!(p.abs() < 1e-9);
    }

    #[test]
    fn cpu_percent_returns_none_for_zero_elapsed() {
        let a = ProcStatCpu { idle: 100, total: 1000 };
        assert!(cpu_percent_from_samples(a, a).is_none());
    }

    #[test]
    fn sample_load_returns_load_sample_struct() {
        let s = sample_load();
        // load_1/5/15/proc_queue_len intentionally unimplemented in this task
        assert!(s.load_1.is_none());
        assert!(s.load_5.is_none());
        assert!(s.load_15.is_none());
        assert!(s.processor_queue_length.is_none());
        // cpu_percent populated on Linux, None elsewhere
        #[cfg(target_os = "linux")]
        assert!(s.cpu_percent.is_some());
        #[cfg(not(target_os = "linux"))]
        assert!(s.cpu_percent.is_none());
    }
}
