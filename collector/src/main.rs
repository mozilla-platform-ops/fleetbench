use clap::{Parser, Subcommand, ValueEnum};

mod cpu;
mod env;
mod freq_sampler;
mod inspect;
mod schema;
mod sieve;

const VERSION_STR: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("FLEETBENCH_GIT_SHA"),
    ")",
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
    Cpu {
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
        /// Time-bounded torture/stress mode. When set, loops the MT sieve
        /// until the duration elapses (skipping the 1t workload). Accepts
        /// bare seconds or human suffixes: `30s`, `10m`, `1h`.
        ///
        /// Interaction with --mode: --mode picks the per-iteration size
        /// (prime_limit) and nothing else — the preset's iteration count is
        /// ignored. For dense per-iteration timing across the run, use
        /// `--mode quick` (each iteration ~tens of ms). `--mode long` still
        /// works but produces only a handful of multi-second iterations,
        /// which makes iteration-time drift a coarse signal; rely on
        /// frequency_series for fine-grained throttle evidence in that case.
        #[arg(long, value_parser = parse_duration_arg)]
        duration: Option<u64>,
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

#[cfg(test)]
mod tests {
    use super::parse_duration_arg;

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
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Command::Inspect { json } => inspect::run(json),
        Command::Cpu { mode, limit, iterations, threads, json, no_warmup, duration } => {
            cpu::run(mode, limit, iterations, &threads, json, !no_warmup, duration)
        }
    };
    std::process::exit(exit_code);
}
