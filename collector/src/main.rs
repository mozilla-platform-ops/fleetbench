use clap::{Parser, Subcommand, ValueEnum};

mod adb;
mod cpu;
mod env;
mod freq_sampler;
#[cfg(target_os = "windows")]
mod freq_windows;
mod inspect;
mod schema;
mod sieve;

const VERSION_STR: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("FLEETBENCH_GIT_SHA"),
    ")",
);

/// Distinctive tagged sentinel embedded in every binary so operators can
/// identify a build without running it. Grep recipe:
///
/// ```sh
/// strings -a fleetbench[.exe] | grep FLEETBENCH_BUILD
/// # FLEETBENCH_BUILD=0.1.0+abc123def456
/// ```
///
/// `#[used]` keeps the static (and the string literal it points to) in the
/// final binary regardless of dead-code elimination.
#[used]
static BUILD_INFO: &str = concat!(
    "FLEETBENCH_BUILD=",
    env!("CARGO_PKG_VERSION"),
    "+",
    env!("FLEETBENCH_GIT_SHA"),
);

#[derive(Parser)]
#[command(name = "fleetbench", version = VERSION_STR, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Collect host and CPU metadata without running a benchmark.
    Inspect {
        #[arg(long)]
        json: bool,
    },
    /// Run CPU benchmark workloads.
    ///
    /// Default: fixed-iteration runs sized by --mode (quick/normal/long).
    /// With --duration: time-bounded torture run; --mode picks per-iteration
    /// size only, iteration count is whatever completes before the deadline.
    Cpu {
        /// Workload preset. Picks prime_limit and iteration count for
        /// default runs (quick=pi(10^7)x3, normal=pi(10^8)x5, long=pi(10^9)x3).
        /// With --duration: only prime_limit applies; iteration count is
        /// ignored. Pair --duration with --mode quick for dense timing.
        #[arg(long, value_enum, default_value_t = Mode::Normal)]
        mode: Mode,
        #[arg(long)]
        limit: Option<u64>,
        #[arg(long)]
        iterations: Option<u32>,
        #[arg(long, default_value = "auto")]
        threads: String,
        #[arg(long)]
        json: bool,
        /// Skip the brief warmup run before timed iterations.
        #[arg(long)]
        no_warmup: bool,
        /// Torture/stress mode: loop MT sieve for this long (e.g. 30s, 10m, 1h);
        /// --mode then sets per-iteration size only. Pair with --mode quick.
        ///
        /// Skips the 1t workload so all cores stay hot continuously. A
        /// background sampler captures per-core CPU frequency at ~1Hz into
        /// the envelope as `frequency_series` — the direct signal for thermal
        /// throttling (boost-clock samples decay toward base-clock over the
        /// run). With --mode long each iteration takes ~seconds, so few
        /// complete; rely on frequency_series for fine-grained evidence.
        #[arg(long, value_parser = parse_duration_arg)]
        duration: Option<u64>,
    },
    /// Time adb push/pull to/from an attached Android device.
    ///
    /// Production unit = one invocation, one device. Pre-generates unique
    /// random payloads per size before the timed section (defeats page-cache
    /// reuse), runs push and pull as separate timed loops, and verifies each
    /// transfer with SHA256. Per-iteration timings are emitted raw; the
    /// distribution is the signal, not the mean. Run multiple invocations
    /// concurrently at the orchestrator layer to observe USB contention.
    Adb {
        /// Transfer directions to time. `both` (the default) records push
        /// then pull samples; `push` records only a contiguous push window.
        #[arg(long, value_enum, default_value_t = AdbDirection::Both)]
        direction: AdbDirection,
        /// Device serial to target. Required if more than one device is
        /// attached.
        #[arg(long)]
        serial: Option<String>,
        /// Path to the adb binary. Defaults to "adb" via PATH.
        #[arg(long)]
        adb_path: Option<String>,
        /// Remote directory on the device for the timed transfers. Defaults
        /// to /data/local/tmp/ (avoids the FUSE layer on /sdcard for a
        /// cleaner USB/adb signal). Use /sdcard/Download to reproduce
        /// raptor's path.
        #[arg(long, default_value = "/data/local/tmp/")]
        remote_path: String,
        /// Comma-separated sizes (e.g. "25B,1M,10M,100M"). Defaults to all
        /// four. Suffixes: B (bytes), K (KiB), M (MiB), G (GiB).
        #[arg(long)]
        sizes: Option<String>,
        /// Per-size iteration count override, KEY=VALUE list
        /// (e.g. "25B=50,1M=20"). Defaults baked in per size:
        /// 25B=200, 1M=100, 10M=30, 100M=10.
        #[arg(long)]
        iterations: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

fn parse_duration_arg(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("duration must not be empty".into());
    }
    let (num_str, mult): (&str, u64) = match s.chars().last().unwrap() {
        's' => (&s[..s.len() - 1], 1),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3600),
        c if c.is_ascii_digit() => (s, 1),
        c => return Err(format!("invalid duration suffix {c:?} (expected s, m, h)")),
    };
    let n: u64 = num_str
        .parse()
        .map_err(|_| format!("invalid duration {s:?} (expected e.g. 30s, 10m, 1h)"))?;
    if n == 0 {
        return Err("duration must be greater than zero".into());
    }
    n.checked_mul(mult)
        .ok_or_else(|| format!("duration {s:?} overflows seconds"))
}

#[derive(Copy, Clone, ValueEnum)]
enum Mode {
    Quick,
    Normal,
    Long,
}

#[derive(Copy, Clone, ValueEnum)]
enum AdbDirection {
    Both,
    Push,
}

impl AdbDirection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Both => "both",
            Self::Push => "push",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_duration_arg, AdbDirection, Cli, Command};
    use clap::Parser;

    #[test]
    fn parses_bare_seconds() {
        assert_eq!(parse_duration_arg("30").unwrap(), 30);
    }

    #[test]
    fn parses_human_suffixes() {
        assert_eq!(parse_duration_arg("30s").unwrap(), 30);
        assert_eq!(parse_duration_arg("10m").unwrap(), 600);
        assert_eq!(parse_duration_arg("2h").unwrap(), 7200);
    }

    #[test]
    fn rejects_zero_empty_and_bad_suffix() {
        assert!(parse_duration_arg("").is_err());
        assert!(parse_duration_arg("0").is_err());
        assert!(parse_duration_arg("0s").is_err());
        assert!(parse_duration_arg("10x").is_err());
        assert!(parse_duration_arg("abc").is_err());
    }

    #[test]
    fn adb_direction_defaults_to_both_and_accepts_push() {
        let default = Cli::try_parse_from(["fleetbench", "adb"]).unwrap();
        assert!(matches!(
            default.command,
            Command::Adb {
                direction: AdbDirection::Both,
                ..
            }
        ));

        let push = Cli::try_parse_from(["fleetbench", "adb", "--direction", "push"]).unwrap();
        assert!(matches!(
            push.command,
            Command::Adb {
                direction: AdbDirection::Push,
                ..
            }
        ));
    }
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Inspect { json } => inspect::run(json),
        Command::Cpu { mode, limit, iterations, threads, json, no_warmup, duration } => {
            cpu::run(mode, limit, iterations, &threads, json, !no_warmup, duration)
        }
        Command::Adb { direction, serial, adb_path, remote_path, sizes, iterations, json } => {
            adb::run(adb_path, serial, remote_path, sizes, iterations, direction.as_str(), json)
        }
    };
    std::process::exit(exit_code);
}
