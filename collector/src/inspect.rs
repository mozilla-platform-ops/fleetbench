use sysinfo::System;

use crate::schema::{
    CpuInfo, HostInfo, Output, Status, COLLECTOR_VERSION, CPU_SUITE_VERSION, SCHEMA_VERSION,
};

pub fn run(json: bool) -> i32 {
    let mut sys = System::new();
    sys.refresh_cpu_all();

    let host = collect_host(&sys);
    let cpu = collect_cpu(&sys);

    let out = Output {
        schema_version: SCHEMA_VERSION,
        collector_version: COLLECTOR_VERSION.into(),
        cpu_suite_version: CPU_SUITE_VERSION.into(),
        timestamp_utc: current_timestamp_utc(),
        status: Status::Ok,
        host,
        cpu,
        config: None,
        environment: None,
        results: None,
        error: None,
    };

    if json {
        match serde_json::to_string_pretty(&out) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("inspect: failed to serialize output: {e}");
                1
            }
        }
    } else {
        print_human(&out);
        0
    }
}

pub fn collect_host(sys: &System) -> HostInfo {
    HostInfo {
        hostname: System::host_name().unwrap_or_else(|| "unknown".into()),
        os_family: std::env::consts::OS.to_string(),
        os_version: System::long_os_version().or_else(System::os_version),
        kernel_version: System::kernel_version(),
        arch: std::env::consts::ARCH.to_string(),
        logical_cpus: sys.cpus().len() as u32,
        physical_cpus: sys.physical_core_count().map(|n| n as u32),
    }
}

pub fn collect_cpu(sys: &System) -> CpuInfo {
    let first = sys.cpus().first();
    CpuInfo {
        brand: first.map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty()),
        vendor: first.map(|c| c.vendor_id().trim().to_string()).filter(|s| !s.is_empty()),
        frequency_mhz: first.map(|c| c.frequency() as u32).filter(|f| *f > 0),
    }
}

pub fn current_timestamp_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_host_populates_required_fields() {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        let host = collect_host(&sys);
        assert!(!host.hostname.is_empty());
        assert!(!host.os_family.is_empty());
        assert!(!host.arch.is_empty());
        assert!(host.logical_cpus > 0);
    }

    #[test]
    fn current_timestamp_is_iso8601_utc() {
        let ts = current_timestamp_utc();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
        chrono::DateTime::parse_from_rfc3339(&ts).expect("valid RFC3339");
    }
}

fn print_human(out: &Output) {
    println!("hostname:       {}", out.host.hostname);
    println!("os_family:      {}", out.host.os_family);
    if let Some(v) = &out.host.os_version {
        println!("os_version:     {v}");
    }
    if let Some(k) = &out.host.kernel_version {
        println!("kernel_version: {k}");
    }
    println!("arch:           {}", out.host.arch);
    println!("logical_cpus:   {}", out.host.logical_cpus);
    if let Some(p) = out.host.physical_cpus {
        println!("physical_cpus:  {p}");
    }
    if let Some(b) = &out.cpu.brand {
        println!("cpu_brand:      {b}");
    }
    if let Some(v) = &out.cpu.vendor {
        println!("cpu_vendor:     {v}");
    }
    if let Some(f) = out.cpu.frequency_mhz {
        println!("cpu_freq_mhz:   {f}");
    }
}
