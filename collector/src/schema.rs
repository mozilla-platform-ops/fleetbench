use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 6;
pub const COLLECTOR_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Short git SHA of the commit this binary was built from, with `-dirty`
/// appended if the working tree had uncommitted changes. Baked in by
/// `build.rs`. Falls back to "unknown" when built outside a git checkout.
pub const COLLECTOR_GIT_SHA: &str = env!("FLEETBENCH_GIT_SHA");
pub const CPU_SUITE_VERSION: &str = "cpu-v0";
pub const ADB_SUITE_VERSION: &str = "adb-v0";

#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub schema_version: u32,
    pub collector_version: String,
    pub collector_git_sha: String,
    pub cpu_suite_version: String,
    pub timestamp_utc: String,
    pub status: Status,

    pub host: HostInfo,
    pub cpu: CpuInfo,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Config>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adb_config: Option<AdbConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Environment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adb_env: Option<AdbEnv>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Results>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adb_results: Option<AdbResults>,
    /// Per-sample CPU frequency captured during a `--duration` run. Omitted
    /// for fixed-iteration runs. Used to surface thermal throttling directly
    /// (frequency decay over the run) rather than inferring it from
    /// iteration-time drift alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_series: Option<Vec<FrequencySample>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrequencySample {
    pub t_offset_seconds: f64,
    pub per_core_mhz: Vec<u32>,
    pub mean_mhz: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os_family: String,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub arch: String,
    pub logical_cpus: u32,
    pub physical_cpus: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CpuInfo {
    pub brand: Option<String>,
    pub vendor: Option<String>,
    pub frequency_mhz: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub command: String,
    pub mode: String,
    pub prime_limit: u64,
    pub iterations: u32,
    pub threads: u32,
    pub warmup_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warmup_prime_limit: Option<u64>,
    /// When set, the run is time-bounded: MT sieve loops until this many
    /// seconds elapse, the 1t workload is skipped, and `iterations` reflects
    /// the count actually completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Environment {
    pub load_pre_warmup: LoadSample,
    pub load_pre_timed: LoadSample,
    pub load_post_timed: LoadSample,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadSample {
    pub cpu_counters: Option<CpuCounters>,
    pub load_1: Option<f64>,
    pub load_5: Option<f64>,
    pub load_15: Option<f64>,
    pub processor_queue_length: Option<u32>,
}

/// Raw CPU time counters captured at a single point in time. Differencing two
/// snapshots over a window yields CPU utilization for that window.
///
/// Units differ per platform; `kind` identifies which to use:
///   "linux_proc_stat":         jiffies (typically 1/100 s × logical CPU)
///   "windows_get_system_times": 100-ns intervals × logical CPU
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCounters {
    pub kind: String,
    pub idle_units: u64,
    pub iowait_units: Option<u64>,
    pub total_units: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Results {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prime_sieve_1t: Option<PrimeSieve1t>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prime_sieve_mt: Option<PrimeSieveMt>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrimeSieve1t {
    pub iterations: Vec<PrimeIteration>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrimeSieveMt {
    pub threads: u32,
    pub iterations: Vec<PrimeIteration>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrimeIteration {
    pub seconds: f64,
    pub prime_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdbConfig {
    pub command: String,
    pub adb_path: String,
    pub serial: Option<String>,
    pub remote_path: String,
    pub sizes: Vec<AdbSizeSpec>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdbSizeSpec {
    pub size_bytes: u64,
    pub iterations: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdbEnv {
    pub adb_version: Option<String>,
    /// `lsusb -t` output captured on Linux hosts; None on other platforms or
    /// when lsusb is unavailable.
    pub lsusb_topology: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdbResults {
    pub iterations: Vec<AdbIteration>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdbIteration {
    pub device_serial: String,
    pub device_model: String,
    /// Logical USB hub path (e.g. extracted from `lsusb -t`) when available.
    pub hub_path: Option<String>,
    pub size_bytes: u64,
    /// "push" (host → device) or "pull" (device → host).
    pub direction: String,
    pub bytes_per_sec: f64,
    pub elapsed_ms: f64,
    pub sha256_ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_host() -> HostInfo {
        HostInfo {
            hostname: "linux-perf-123".into(),
            os_family: "linux".into(),
            os_version: Some("Ubuntu 24.04".into()),
            kernel_version: Some("6.8.0-xx".into()),
            arch: "x86_64".into(),
            logical_cpus: 32,
            physical_cpus: Some(16),
        }
    }

    fn sample_cpu() -> CpuInfo {
        CpuInfo {
            brand: Some("AMD EPYC ...".into()),
            vendor: Some("AuthenticAMD".into()),
            frequency_mhz: Some(3200),
        }
    }

    #[test]
    fn success_output_serializes_with_expected_top_level_keys() {
        let out = Output {
            schema_version: SCHEMA_VERSION,
            collector_version: COLLECTOR_VERSION.into(),
            collector_git_sha: COLLECTOR_GIT_SHA.into(),
            cpu_suite_version: CPU_SUITE_VERSION.into(),
            timestamp_utc: "2026-05-20T00:00:00Z".into(),
            status: Status::Ok,
            host: sample_host(),
            cpu: sample_cpu(),
            config: Some(Config {
                command: "cpu".into(),
                mode: "normal".into(),
                prime_limit: 100_000_000,
                iterations: 5,
                threads: 32,
                warmup_enabled: true,
                warmup_prime_limit: Some(1_000_000),
                duration_seconds: None,
            }),
            environment: Some(Environment {
                load_pre_warmup: LoadSample {
                    cpu_counters: Some(CpuCounters {
                        kind: "linux_proc_stat".into(),
                        idle_units: 1_000_000,
                        iowait_units: Some(50),
                        total_units: 1_100_000,
                    }),
                    load_1: Some(0.42),
                    load_5: Some(0.55),
                    load_15: Some(0.61),
                    processor_queue_length: None,
                },
                load_pre_timed: LoadSample {
                    cpu_counters: Some(CpuCounters {
                        kind: "linux_proc_stat".into(),
                        idle_units: 1_000_100,
                        iowait_units: Some(50),
                        total_units: 1_100_300,
                    }),
                    load_1: Some(0.48),
                    load_5: Some(0.56),
                    load_15: Some(0.61),
                    processor_queue_length: None,
                },
                load_post_timed: LoadSample {
                    cpu_counters: Some(CpuCounters {
                        kind: "linux_proc_stat".into(),
                        idle_units: 1_000_100,
                        iowait_units: Some(50),
                        total_units: 1_200_300,
                    }),
                    load_1: Some(8.91),
                    load_5: Some(2.10),
                    load_15: Some(0.95),
                    processor_queue_length: None,
                },
            }),
            results: Some(Results {
                prime_sieve_1t: Some(PrimeSieve1t {
                    iterations: vec![PrimeIteration {
                        seconds: 4.21,
                        prime_count: 5_761_455,
                    }],
                }),
                prime_sieve_mt: Some(PrimeSieveMt {
                    threads: 32,
                    iterations: vec![PrimeIteration {
                        seconds: 0.41,
                        prime_count: 5_761_455,
                    }],
                }),
            }),
            adb_config: None,
            adb_env: None,
            adb_results: None,
            frequency_series: None,
            error: None,
        };

        let v: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(v["schema_version"], 6);
        assert!(v["collector_git_sha"].is_string());
        assert!(v.get("frequency_series").is_none(), "frequency_series must be omitted when unset");
        assert_eq!(v["cpu_suite_version"], "cpu-v0");
        assert_eq!(v["status"], "ok");
        assert!(v.get("error").is_none(), "error must be omitted on success");
        assert_eq!(v["environment"]["load_pre_warmup"]["cpu_counters"]["kind"], "linux_proc_stat");
        assert_eq!(v["environment"]["load_pre_warmup"]["cpu_counters"]["idle_units"], 1_000_000);
        assert_eq!(v["results"]["prime_sieve_mt"]["threads"], 32);
    }

    #[test]
    fn failed_output_omits_results_and_environment() {
        let out = Output {
            schema_version: SCHEMA_VERSION,
            collector_version: COLLECTOR_VERSION.into(),
            collector_git_sha: COLLECTOR_GIT_SHA.into(),
            cpu_suite_version: CPU_SUITE_VERSION.into(),
            timestamp_utc: "2026-05-20T00:00:00Z".into(),
            status: Status::Failed,
            host: sample_host(),
            cpu: sample_cpu(),
            config: None,
            adb_config: None,
            environment: None,
            adb_env: None,
            results: None,
            adb_results: None,
            frequency_series: None,
            error: Some(ErrorInfo {
                kind: "correctness_check_failed".into(),
                message: "prime count mismatch".into(),
            }),
        };

        let v: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(v["status"], "failed");
        assert_eq!(v["error"]["kind"], "correctness_check_failed");
        assert!(v.get("results").is_none());
        assert!(v.get("environment").is_none());
        assert!(v.get("config").is_none());
    }

    #[test]
    fn load_sample_emits_nulls_for_missing_fields() {
        let s = LoadSample {
            cpu_counters: Some(CpuCounters {
                kind: "linux_proc_stat".into(),
                idle_units: 1000,
                iowait_units: Some(5),
                total_units: 1500,
            }),
            load_1: None,
            load_5: None,
            load_15: None,
            processor_queue_length: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["cpu_counters"]["kind"], "linux_proc_stat");
        assert_eq!(v["cpu_counters"]["idle_units"], 1000);
        assert!(v["load_1"].is_null());
        assert!(v["processor_queue_length"].is_null());
    }
}
