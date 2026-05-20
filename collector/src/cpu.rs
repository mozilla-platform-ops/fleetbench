use sysinfo::System;

use crate::env::sample_load;
use crate::inspect::{collect_cpu, collect_host, current_timestamp_utc};
use crate::schema::{
    Config, CpuInfo, Environment, ErrorInfo, HostInfo, Output, PrimeSieve1t, PrimeSieveMt,
    Results, Status, COLLECTOR_VERSION, CPU_SUITE_VERSION, SCHEMA_VERSION,
};
use crate::sieve;
use crate::Mode;

const EXIT_OK: i32 = 0;
const EXIT_RUNTIME_ERROR: i32 = 1;
const EXIT_CORRECTNESS_FAILED: i32 = 2;

const ERR_INVALID_ARGUMENTS: &str = "invalid_arguments";
const ERR_CORRECTNESS_CHECK_FAILED: &str = "correctness_check_failed";

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

    let host = collect_host(&sys);
    let cpu = collect_cpu(&sys);
    let logical_cpus = sys.cpus().len() as u32;
    let preset = preset(mode);
    let prime_limit = limit.unwrap_or(preset.prime_limit);
    let iter_count = iterations.unwrap_or(preset.iterations);

    let threads = match parse_threads(threads_arg, logical_cpus) {
        Ok(t) => t,
        Err(msg) => {
            return emit_failure(
                json,
                host,
                cpu,
                None,
                ErrorInfo { kind: ERR_INVALID_ARGUMENTS.into(), message: msg },
                EXIT_RUNTIME_ERROR,
            );
        }
    };

    let config = Config {
        command: "cpu".into(),
        mode: preset.name.into(),
        prime_limit,
        iterations: iter_count,
        threads,
        warmup_enabled: false,
        warmup_prime_limit: None,
    };

    let load_pre_warmup = sample_load();
    // Warmup will go between these two samples once .7 lands.
    let load_pre_timed = sample_load();

    let workload_result = run_workloads(prime_limit, iter_count, threads);

    let load_post_timed = sample_load();
    let environment = Some(Environment { load_pre_warmup, load_pre_timed, load_post_timed });

    match workload_result {
        Ok(results) => {
            let out = build_output(Status::Ok, host, cpu, Some(config), environment, Some(results), None);
            emit(json, &out, EXIT_OK)
        }
        Err(err) => {
            let exit = exit_code_for(&err);
            let out = build_output(
                Status::Failed,
                host,
                cpu,
                Some(config),
                environment,
                None,
                Some(err),
            );
            emit(json, &out, exit)
        }
    }
}

fn build_output(
    status: Status,
    host: HostInfo,
    cpu: CpuInfo,
    config: Option<Config>,
    environment: Option<Environment>,
    results: Option<Results>,
    error: Option<ErrorInfo>,
) -> Output {
    Output {
        schema_version: SCHEMA_VERSION,
        collector_version: COLLECTOR_VERSION.into(),
        cpu_suite_version: CPU_SUITE_VERSION.into(),
        timestamp_utc: current_timestamp_utc(),
        status,
        host,
        cpu,
        config,
        environment,
        results,
        error,
    }
}

fn emit_failure(
    json: bool,
    host: HostInfo,
    cpu: CpuInfo,
    config: Option<Config>,
    error: ErrorInfo,
    exit_code: i32,
) -> i32 {
    let out = build_output(Status::Failed, host, cpu, config, None, None, Some(error));
    emit(json, &out, exit_code)
}

fn emit(json: bool, out: &Output, intended_exit: i32) -> i32 {
    if json {
        match serde_json::to_string_pretty(out) {
            Ok(s) => {
                println!("{s}");
                intended_exit
            }
            Err(e) => {
                eprintln!("cpu: failed to serialize output: {e}");
                EXIT_RUNTIME_ERROR
            }
        }
    } else {
        print_human(out);
        intended_exit
    }
}

fn exit_code_for(err: &ErrorInfo) -> i32 {
    if err.kind == ERR_CORRECTNESS_CHECK_FAILED {
        EXIT_CORRECTNESS_FAILED
    } else {
        EXIT_RUNTIME_ERROR
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
    fn exit_code_for_correctness_failure_is_two() {
        let e = ErrorInfo {
            kind: ERR_CORRECTNESS_CHECK_FAILED.into(),
            message: "x".into(),
        };
        assert_eq!(exit_code_for(&e), EXIT_CORRECTNESS_FAILED);
    }

    #[test]
    fn exit_code_for_other_errors_is_one() {
        let e = ErrorInfo {
            kind: ERR_INVALID_ARGUMENTS.into(),
            message: "x".into(),
        };
        assert_eq!(exit_code_for(&e), EXIT_RUNTIME_ERROR);
        let e2 = ErrorInfo { kind: "anything_else".into(), message: "x".into() };
        assert_eq!(exit_code_for(&e2), EXIT_RUNTIME_ERROR);
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
