use sysinfo::System;

use crate::inspect::{collect_cpu, collect_host, current_timestamp_utc};
use crate::schema::{
    Config, ErrorInfo, Output, PrimeSieve1t, PrimeSieveMt, Results, Status, COLLECTOR_VERSION,
    CPU_SUITE_VERSION, SCHEMA_VERSION,
};
use crate::sieve;
use crate::Mode;

struct ModePreset {
    name: &'static str,
    prime_limit: u64,
    iterations: u32,
}

fn preset(mode: Mode) -> ModePreset {
    match mode {
        Mode::Quick => ModePreset { name: "quick", prime_limit: 10_000_000, iterations: 3 },
        Mode::Normal => ModePreset { name: "normal", prime_limit: 100_000_000, iterations: 5 },
        Mode::Long => ModePreset { name: "long", prime_limit: 1_000_000_000, iterations: 3 },
    }
}

pub fn run(
    mode: Mode,
    limit: Option<u64>,
    iterations: Option<u32>,
    threads_arg: &str,
    json: bool,
) -> i32 {
    let mut sys = System::new();
    sys.refresh_cpu_all();

    let logical_cpus = sys.cpus().len() as u32;
    let preset = preset(mode);
    let prime_limit = limit.unwrap_or(preset.prime_limit);
    let iter_count = iterations.unwrap_or(preset.iterations);

    let threads = match parse_threads(threads_arg, logical_cpus) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cpu: {e}");
            return 1;
        }
    };

    let host = collect_host(&sys);
    let cpu = collect_cpu(&sys);
    let config = Config {
        command: "cpu".into(),
        mode: preset.name.into(),
        prime_limit,
        iterations: iter_count,
        threads,
        warmup_enabled: false,
        warmup_prime_limit: None,
    };

    let (status, results, error) = match run_workloads(prime_limit, iter_count, threads) {
        Ok(r) => (Status::Ok, Some(r), None),
        Err(e) => (Status::Failed, None, Some(e)),
    };

    let out = Output {
        schema_version: SCHEMA_VERSION,
        collector_version: COLLECTOR_VERSION.into(),
        cpu_suite_version: CPU_SUITE_VERSION.into(),
        timestamp_utc: current_timestamp_utc(),
        status,
        host,
        cpu,
        config: Some(config),
        environment: None,
        results,
        error: error.clone(),
    };

    let emit_exit = if json {
        match serde_json::to_string_pretty(&out) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("cpu: failed to serialize output: {e}");
                1
            }
        }
    } else {
        print_human(&out);
        0
    };

    if emit_exit != 0 {
        return emit_exit;
    }

    match (out.status, error) {
        (Status::Ok, _) => 0,
        (Status::Failed, Some(e)) if e.kind == "correctness_check_failed" => 2,
        (Status::Failed, _) => 1,
    }
}

fn run_workloads(limit: u64, iterations: u32, threads: u32) -> Result<Results, ErrorInfo> {
    let st = sieve::run_1t(limit, iterations)?;
    let mt = sieve::run_mt(limit, iterations, threads)?;
    Ok(Results {
        prime_sieve_1t: Some(PrimeSieve1t { iterations: st }),
        prime_sieve_mt: Some(PrimeSieveMt { threads, iterations: mt }),
    })
}

fn parse_threads(arg: &str, logical_cpus: u32) -> Result<u32, String> {
    if arg.eq_ignore_ascii_case("auto") {
        return Ok(logical_cpus.max(1));
    }
    let n: u32 = arg
        .parse()
        .map_err(|_| format!("invalid --threads value: {arg:?} (expected 'auto' or a positive integer)"))?;
    if n == 0 {
        return Err("--threads must be at least 1".into());
    }
    Ok(n)
}

fn print_human(out: &Output) {
    println!("status:         {:?}", out.status);
    if let Some(cfg) = &out.config {
        println!("mode:           {}", cfg.mode);
        println!("prime_limit:    {}", cfg.prime_limit);
        println!("iterations:     {}", cfg.iterations);
        println!("threads:        {}", cfg.threads);
    }
    if let Some(err) = &out.error {
        println!("error.kind:     {}", err.kind);
        println!("error.message:  {}", err.message);
        return;
    }
    if let Some(r) = &out.results {
        if let Some(st) = &r.prime_sieve_1t {
            let times: Vec<String> =
                st.iterations.iter().map(|i| format!("{:.3}", i.seconds)).collect();
            println!("prime_sieve_1t: [{}] s", times.join(", "));
        }
        if let Some(mt) = &r.prime_sieve_mt {
            let times: Vec<String> =
                mt.iterations.iter().map(|i| format!("{:.3}", i.seconds)).collect();
            println!("prime_sieve_mt: [{}] s ({} threads)", times.join(", "), mt.threads);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_threads_auto_uses_logical_cpus() {
        assert_eq!(parse_threads("auto", 8).unwrap(), 8);
        assert_eq!(parse_threads("AUTO", 8).unwrap(), 8);
    }

    #[test]
    fn parse_threads_numeric() {
        assert_eq!(parse_threads("4", 16).unwrap(), 4);
    }

    #[test]
    fn parse_threads_rejects_zero_and_garbage() {
        assert!(parse_threads("0", 8).is_err());
        assert!(parse_threads("abc", 8).is_err());
    }

    #[test]
    fn presets_match_design_doc() {
        let q = preset(Mode::Quick);
        assert_eq!(q.prime_limit, 10_000_000);
        assert_eq!(q.iterations, 3);
        let n = preset(Mode::Normal);
        assert_eq!(n.prime_limit, 100_000_000);
        assert_eq!(n.iterations, 5);
        let l = preset(Mode::Long);
        assert_eq!(l.prime_limit, 1_000_000_000);
        assert_eq!(l.iterations, 3);
    }
}
