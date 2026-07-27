//! `fleetbench adb` — adb push/pull I/O benchmark.
//!
//! Runs on the Linux Docker host where adb lives. Production unit is one
//! invocation, one device; contention is observed by running many invocations
//! concurrently at the TC layer. The distribution of per-iteration timings is
//! the signal — the analysis layer computes any summary statistics.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use sysinfo::System;

use crate::env::sample_load;
use crate::inspect::{collect_cpu, collect_host, current_timestamp_utc};
use crate::schema::{
    AdbConfig, AdbEnv, AdbIteration, AdbResults, AdbSizeSpec, CpuInfo, Environment, ErrorInfo,
    HostInfo, Output, Status, ADB_SUITE_VERSION, COLLECTOR_GIT_SHA, COLLECTOR_VERSION,
    CPU_SUITE_VERSION, SCHEMA_VERSION,
};

const EXIT_OK: i32 = 0;
const EXIT_RUNTIME_ERROR: i32 = 1;
const EXIT_CORRECTNESS_FAILED: i32 = 2;

const ERR_INVALID_ARGUMENTS: &str = "invalid_arguments";
const ERR_ADB_NOT_FOUND: &str = "adb_not_found";
const ERR_NO_DEVICE: &str = "no_device";
const ERR_MULTIPLE_DEVICES: &str = "multiple_devices";
const ERR_ADB_COMMAND_FAILED: &str = "adb_command_failed";
const ERR_IO: &str = "io_error";
const ERR_CORRECTNESS_CHECK_FAILED: &str = "correctness_check_failed";

#[derive(Clone, Copy)]
struct SizeDefault {
    bytes: u64,
    iterations: u32,
}

const DEFAULT_SIZES: &[SizeDefault] = &[
    SizeDefault {
        bytes: 25,
        iterations: 200,
    },
    SizeDefault {
        bytes: 1024 * 1024,
        iterations: 100,
    },
    SizeDefault {
        bytes: 10 * 1024 * 1024,
        iterations: 30,
    },
    SizeDefault {
        bytes: 100 * 1024 * 1024,
        iterations: 10,
    },
];

pub fn run(
    adb_path: Option<String>,
    serial: Option<String>,
    remote_path: String,
    sizes_arg: Option<String>,
    iterations_arg: Option<String>,
    json: bool,
) -> i32 {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    let host = collect_host(&sys);
    let cpu = collect_cpu(&sys);

    let adb_bin = adb_path.unwrap_or_else(|| "adb".to_string());

    // Resolve specs from --sizes / --iterations.
    let specs = match resolve_specs(sizes_arg.as_deref(), iterations_arg.as_deref()) {
        Ok(s) => s,
        Err(msg) => {
            return emit_failure(
                json,
                host,
                cpu,
                None,
                None,
                ErrorInfo {
                    kind: ERR_INVALID_ARGUMENTS.into(),
                    message: msg,
                },
                EXIT_RUNTIME_ERROR,
            );
        }
    };

    // Normalize remote path (ensure trailing slash for directory semantics).
    let remote_dir = if remote_path.ends_with('/') {
        remote_path.clone()
    } else {
        format!("{remote_path}/")
    };

    let adb_config = AdbConfig {
        command: "adb".into(),
        adb_path: adb_bin.clone(),
        serial: serial.clone(),
        remote_path: remote_dir.clone(),
        sizes: specs
            .iter()
            .map(|s| AdbSizeSpec {
                size_bytes: s.bytes,
                iterations: s.iterations,
            })
            .collect(),
    };

    // Capture adb --version and lsusb topology up front.
    let adb_version = capture_adb_version(&adb_bin);
    let lsusb_topology = capture_lsusb_topology();
    let adb_env = AdbEnv {
        adb_version: adb_version.clone(),
        lsusb_topology: lsusb_topology.clone(),
    };

    if adb_version.is_none() {
        return emit_failure(
            json,
            host,
            cpu,
            Some(adb_config),
            Some(adb_env),
            ErrorInfo {
                kind: ERR_ADB_NOT_FOUND.into(),
                message: format!("could not execute {adb_bin:?} --version"),
            },
            EXIT_RUNTIME_ERROR,
        );
    }

    // Resolve target device.
    let device = match resolve_device(&adb_bin, serial.as_deref()) {
        Ok(d) => d,
        Err(err) => {
            let exit = exit_code_for(&err);
            return emit_failure(json, host, cpu, Some(adb_config), Some(adb_env), err, exit);
        }
    };

    let load_pre_warmup = sample_load();
    let load_pre_timed = sample_load();

    let run_result = run_iterations(&adb_bin, &device, &remote_dir, &specs);

    let load_post_timed = sample_load();
    let environment = Some(Environment {
        load_pre_warmup,
        load_pre_timed,
        load_post_timed,
    });

    match run_result {
        Ok(iterations) => {
            let any_bad = iterations.iter().any(|i| !i.sha256_ok);
            let status = if any_bad { Status::Failed } else { Status::Ok };
            let error = any_bad.then(|| ErrorInfo {
                kind: ERR_CORRECTNESS_CHECK_FAILED.into(),
                message: "one or more iterations failed sha256 verification".into(),
            });
            let exit_code = if any_bad {
                EXIT_CORRECTNESS_FAILED
            } else {
                EXIT_OK
            };
            let out = build_output(
                status,
                host,
                cpu,
                Some(adb_config),
                Some(adb_env),
                environment,
                Some(AdbResults { iterations }),
                error,
            );
            emit(json, &out, exit_code)
        }
        Err(err) => {
            let exit = exit_code_for(&err);
            let out = build_output(
                Status::Failed,
                host,
                cpu,
                Some(adb_config),
                Some(adb_env),
                environment,
                None,
                Some(err),
            );
            emit(json, &out, exit)
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedSize {
    bytes: u64,
    iterations: u32,
}

fn resolve_specs(
    sizes_arg: Option<&str>,
    iterations_arg: Option<&str>,
) -> Result<Vec<ResolvedSize>, String> {
    let chosen: Vec<SizeDefault> = match sizes_arg {
        None => DEFAULT_SIZES.to_vec(),
        Some(s) => {
            let mut out = Vec::new();
            for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                let bytes = parse_size_token(tok)?;
                // Find matching default for iteration count, else require iterations override.
                let default = DEFAULT_SIZES.iter().find(|d| d.bytes == bytes).copied();
                out.push(SizeDefault {
                    bytes,
                    iterations: default.map(|d| d.iterations).unwrap_or(0),
                });
            }
            if out.is_empty() {
                return Err("--sizes was empty".into());
            }
            out
        }
    };

    let overrides = parse_iterations_arg(iterations_arg)?;

    let mut resolved = Vec::with_capacity(chosen.len());
    for size in &chosen {
        let mut iters = size.iterations;
        if let Some((_, n)) = overrides.iter().find(|(b, _)| *b == size.bytes) {
            iters = *n;
        }
        if iters == 0 {
            return Err(format!(
                "no iteration count for size {} bytes (no default; specify via --iterations)",
                size.bytes
            ));
        }
        resolved.push(ResolvedSize {
            bytes: size.bytes,
            iterations: iters,
        });
    }
    Ok(resolved)
}

fn parse_size_token(tok: &str) -> Result<u64, String> {
    let t = tok.trim();
    if t.is_empty() {
        return Err("empty size token".into());
    }
    let (num, mult): (&str, u64) = match t.chars().last().unwrap() {
        'B' | 'b' => (&t[..t.len() - 1], 1),
        'K' | 'k' => (&t[..t.len() - 1], 1024),
        'M' | 'm' => (&t[..t.len() - 1], 1024 * 1024),
        'G' | 'g' => (&t[..t.len() - 1], 1024 * 1024 * 1024),
        c if c.is_ascii_digit() => (t, 1),
        c => {
            return Err(format!(
                "invalid size suffix {c:?} in {tok:?} (expected B, K, M, G)"
            ))
        }
    };
    let n: u64 = num.parse().map_err(|_| format!("invalid size {tok:?}"))?;
    if n == 0 {
        return Err(format!("size {tok:?} must be greater than zero"));
    }
    n.checked_mul(mult)
        .ok_or_else(|| format!("size {tok:?} overflows"))
}

fn parse_iterations_arg(arg: Option<&str>) -> Result<Vec<(u64, u32)>, String> {
    let Some(s) = arg else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let (k, v) = tok
            .split_once('=')
            .ok_or_else(|| format!("--iterations entry {tok:?} must be KEY=VALUE"))?;
        let bytes = parse_size_token(k.trim())?;
        let n: u32 = v
            .trim()
            .parse()
            .map_err(|_| format!("invalid iteration count in {tok:?}"))?;
        if n == 0 {
            return Err(format!(
                "iteration count in {tok:?} must be greater than zero"
            ));
        }
        out.push((bytes, n));
    }
    Ok(out)
}

struct Device {
    serial: String,
    model: String,
    hub_path: Option<String>,
}

fn resolve_device(adb: &str, requested: Option<&str>) -> Result<Device, ErrorInfo> {
    let out = Command::new(adb)
        .args(["devices", "-l"])
        .output()
        .map_err(|e| ErrorInfo {
            kind: ERR_ADB_COMMAND_FAILED.into(),
            message: format!("failed to spawn `{adb} devices -l`: {e}"),
        })?;
    if !out.status.success() {
        return Err(ErrorInfo {
            kind: ERR_ADB_COMMAND_FAILED.into(),
            message: format!("`adb devices -l` exited with {}", out.status),
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut serials: Vec<String> = Vec::new();
    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };
        let Some(state) = parts.next() else { continue };
        if state == "device" {
            serials.push(serial.to_string());
        }
    }
    let serial = match requested {
        Some(req) => {
            if !serials.iter().any(|s| s == req) {
                return Err(ErrorInfo {
                    kind: ERR_NO_DEVICE.into(),
                    message: format!("requested serial {req:?} not present in `adb devices`"),
                });
            }
            req.to_string()
        }
        None => match serials.len() {
            0 => {
                return Err(ErrorInfo {
                    kind: ERR_NO_DEVICE.into(),
                    message: "no devices attached".into(),
                })
            }
            1 => serials.into_iter().next().unwrap(),
            _ => {
                return Err(ErrorInfo {
                    kind: ERR_MULTIPLE_DEVICES.into(),
                    message: format!(
                        "multiple devices attached ({}); pass --serial",
                        serials.join(",")
                    ),
                })
            }
        },
    };

    let model = capture_device_model(adb, &serial).unwrap_or_else(|| "unknown".into());
    let hub_path = capture_hub_path(&serial);
    Ok(Device {
        serial,
        model,
        hub_path,
    })
}

fn capture_adb_version(adb: &str) -> Option<String> {
    let out = Command::new(adb).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "linux")]
fn capture_lsusb_topology() -> Option<String> {
    let out = Command::new("lsusb").arg("-t").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "linux"))]
fn capture_lsusb_topology() -> Option<String> {
    None
}

fn capture_device_model(adb: &str, serial: &str) -> Option<String> {
    let out = Command::new(adb)
        .args(["-s", serial, "shell", "getprop", "ro.product.model"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Best-effort: look the device's serial up in `lsusb -v` is overkill; the
/// caller has the full `lsusb -t` already in env. We just return None here and
/// rely on the topology field for correlation. Reserved for future enrichment.
fn capture_hub_path(_serial: &str) -> Option<String> {
    None
}

fn run_iterations(
    adb: &str,
    device: &Device,
    remote_dir: &str,
    specs: &[ResolvedSize],
) -> Result<Vec<AdbIteration>, ErrorInfo> {
    let mut all = Vec::new();
    let tty = std::io::stderr().is_terminal();

    progress_line(
        tty,
        &format!(
            "adb: device serial={} model={} sizes={}",
            device.serial,
            device.model,
            specs
                .iter()
                .map(|s| format!("{}B×{}", s.bytes, s.iterations))
                .collect::<Vec<_>>()
                .join(",")
        ),
    );

    // Use a unique workspace under the system temp dir so concurrent
    // invocations don't collide on local paths.
    let work_dir = unique_workspace().map_err(|e| ErrorInfo {
        kind: ERR_IO.into(),
        message: format!("failed to create work dir: {e}"),
    })?;
    let _cleanup = WorkDir(work_dir.clone());

    for spec in specs {
        let n = spec.iterations as usize;
        let size_label = format_size(spec.bytes);
        progress_line(tty, &format!("adb: [{size_label}] generating {n} payloads"));
        // Pre-generate N unique local payloads to defeat page-cache reuse.
        let mut local_files: Vec<(PathBuf, [u8; 32])> = Vec::with_capacity(n);
        for i in 0..n {
            let path = work_dir.join(format!("payload_{}_{i}.bin", spec.bytes));
            let hash = generate_random_file(&path, spec.bytes).map_err(|e| ErrorInfo {
                kind: ERR_IO.into(),
                message: format!("failed to create payload {}: {e}", path.display()),
            })?;
            local_files.push((path, hash));
        }

        // PUSH timed loop. Keep timed pushes back-to-back: performing the
        // remote SHA256 check after every push inserts an extra adb shell
        // round-trip between samples and changes the contention pattern. In
        // particular, Raptor's original 25-byte adb-latency probe ran its 200
        // pushes consecutively and deferred all cleanup. Verification remains
        // mandatory, but runs after the complete push loop.
        progress_line(tty, &format!("adb: [{size_label}] pushing"));
        let mut pushed_for_verification = Vec::with_capacity(n);
        for (i, (path, expected_hash)) in local_files.iter().enumerate() {
            progress_inplace(tty, &format!("adb: [{size_label}] push {}/{n}", i + 1));
            let remote = format!("{remote_dir}fleetbench_{}_{i}.bin", spec.bytes);
            let transfer_started_at_utc = transfer_timestamp_utc();
            let t0 = Instant::now();
            let status = Command::new(adb)
                .args([
                    "-s",
                    &device.serial,
                    "push",
                    path.to_str().unwrap(),
                    &remote,
                ])
                .output()
                .map_err(|e| ErrorInfo {
                    kind: ERR_ADB_COMMAND_FAILED.into(),
                    message: format!("adb push spawn failed: {e}"),
                })?;
            let elapsed = t0.elapsed();
            let transfer_finished_at_utc = transfer_timestamp_utc();
            if !status.status.success() {
                return Err(ErrorInfo {
                    kind: ERR_ADB_COMMAND_FAILED.into(),
                    message: format!(
                        "adb push failed (size={} iter={i}): {}",
                        spec.bytes,
                        String::from_utf8_lossy(&status.stderr).trim()
                    ),
                });
            }
            let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
            let bytes_per_sec = if elapsed.as_secs_f64() > 0.0 {
                spec.bytes as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let result_index = all.len();
            all.push(AdbIteration {
                device_serial: device.serial.clone(),
                device_model: device.model.clone(),
                hub_path: device.hub_path.clone(),
                size_bytes: spec.bytes,
                direction: "push".into(),
                transfer_started_at_utc: Some(transfer_started_at_utc),
                transfer_finished_at_utc: Some(transfer_finished_at_utc),
                bytes_per_sec,
                elapsed_ms,
                // Updated after the contiguous push loop completes.
                sha256_ok: true,
            });
            pushed_for_verification.push((result_index, remote, *expected_hash));
        }

        progress_inplace_done(tty);
        progress_line(tty, &format!("adb: [{size_label}] verifying pushes"));
        for (result_index, remote, expected_hash) in pushed_for_verification {
            all[result_index].sha256_ok =
                verify_remote_sha256(adb, &device.serial, &remote, &expected_hash);
        }

        // PULL timed loop — pulls each previously-pushed remote file to a
        // distinct local path, verifies via local sha256.
        progress_line(tty, &format!("adb: [{size_label}] pulling"));
        for (i, (_path, expected_hash)) in local_files.iter().enumerate() {
            progress_inplace(tty, &format!("adb: [{size_label}] pull {}/{n}", i + 1));
            let remote = format!("{remote_dir}fleetbench_{}_{i}.bin", spec.bytes);
            let local_pulled = work_dir.join(format!("pulled_{}_{i}.bin", spec.bytes));
            let transfer_started_at_utc = transfer_timestamp_utc();
            let t0 = Instant::now();
            let status = Command::new(adb)
                .args([
                    "-s",
                    &device.serial,
                    "pull",
                    &remote,
                    local_pulled.to_str().unwrap(),
                ])
                .output()
                .map_err(|e| ErrorInfo {
                    kind: ERR_ADB_COMMAND_FAILED.into(),
                    message: format!("adb pull spawn failed: {e}"),
                })?;
            let elapsed = t0.elapsed();
            let transfer_finished_at_utc = transfer_timestamp_utc();
            if !status.status.success() {
                return Err(ErrorInfo {
                    kind: ERR_ADB_COMMAND_FAILED.into(),
                    message: format!(
                        "adb pull failed (size={} iter={i}): {}",
                        spec.bytes,
                        String::from_utf8_lossy(&status.stderr).trim()
                    ),
                });
            }
            let sha_ok = match sha256_file(&local_pulled) {
                Ok(h) => h == *expected_hash,
                Err(_) => false,
            };
            let _ = fs::remove_file(&local_pulled);
            let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
            let bytes_per_sec = if elapsed.as_secs_f64() > 0.0 {
                spec.bytes as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            };
            all.push(AdbIteration {
                device_serial: device.serial.clone(),
                device_model: device.model.clone(),
                hub_path: device.hub_path.clone(),
                size_bytes: spec.bytes,
                direction: "pull".into(),
                transfer_started_at_utc: Some(transfer_started_at_utc),
                transfer_finished_at_utc: Some(transfer_finished_at_utc),
                bytes_per_sec,
                elapsed_ms,
                sha256_ok: sha_ok,
            });
        }

        progress_inplace_done(tty);
        // Cleanup remote files for this size.
        progress_line(tty, &format!("adb: [{size_label}] cleanup"));
        for i in 0..n {
            let remote = format!("{remote_dir}fleetbench_{}_{i}.bin", spec.bytes);
            let _ = Command::new(adb)
                .args(["-s", &device.serial, "shell", "rm", "-f", &remote])
                .output();
        }
    }
    progress_line(tty, "adb: done");

    Ok(all)
}

/// RFC 3339 UTC timestamp with microsecond precision. This is intentionally
/// finer-grained than the envelope timestamp so separately scheduled workers
/// can be correlated by transfer overlap.
fn transfer_timestamp_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Writes a progress line to stderr. Always emitted, regardless of --json,
/// because --json output goes to stdout. When stderr is a TTY, finishes any
/// in-progress carriage-return line first so the new line doesn't get
/// overwritten.
fn progress_line(tty: bool, msg: &str) {
    let mut err = std::io::stderr().lock();
    if tty {
        let _ = write!(err, "\r\x1b[2K");
    }
    let _ = writeln!(err, "{msg}");
    let _ = err.flush();
}

/// In-place per-iteration counter. On a TTY uses CR + clear-line for live
/// updates; on a pipe (e.g. captured logs) prints one line per call so the
/// output is still useful, just chattier.
fn progress_inplace(tty: bool, msg: &str) {
    let mut err = std::io::stderr().lock();
    if tty {
        let _ = write!(err, "\r\x1b[2K{msg}");
    } else {
        let _ = writeln!(err, "{msg}");
    }
    let _ = err.flush();
}

/// Finalize an `progress_inplace` line by emitting a newline (TTY only).
fn progress_inplace_done(tty: bool) {
    if tty {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err);
        let _ = err.flush();
    }
}

fn format_size(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 && n % (1024 * 1024 * 1024) == 0 {
        format!("{}G", n / (1024 * 1024 * 1024))
    } else if n >= 1024 * 1024 && n % (1024 * 1024) == 0 {
        format!("{}M", n / (1024 * 1024))
    } else if n >= 1024 && n % 1024 == 0 {
        format!("{}K", n / 1024)
    } else {
        format!("{n}B")
    }
}

fn unique_workspace() -> std::io::Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("fleetbench-adb-{pid}-{ts}"));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

struct WorkDir(PathBuf);
impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Fast xorshift64-filled random payload. Deterministic per call (seeded by
/// SystemTime+address), good enough to prevent page-cache reuse across the
/// pre-generated files within a run. Returns the SHA256 of the written bytes.
fn generate_random_file(path: &Path, size: u64) -> std::io::Result<[u8; 32]> {
    let seed_a = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let seed_b = path.as_os_str().len() as u64 ^ size ^ 0x9E37_79B9_7F4A_7C15;
    let mut state = seed_a ^ seed_b.wrapping_mul(0x100000001B3);
    if state == 0 {
        state = 1;
    }

    let mut f = fs::File::create(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let chunk = remaining.min(buf.len() as u64) as usize;
        for word in buf[..chunk].chunks_mut(8) {
            // xorshift64
            let mut x = state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            state = x;
            let bytes = x.to_le_bytes();
            for (i, b) in word.iter_mut().enumerate() {
                *b = bytes[i];
            }
        }
        f.write_all(&buf[..chunk])?;
        hasher.update(&buf[..chunk]);
        remaining -= chunk as u64;
    }
    f.flush()?;
    Ok(hasher.finalize().into())
}

fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

fn verify_remote_sha256(adb: &str, serial: &str, remote: &str, expected: &[u8; 32]) -> bool {
    let out = match Command::new(adb)
        .args(["-s", serial, "shell", "sha256sum", remote])
        .output()
    {
        Ok(o) => o,
        Err(_) => return false,
    };
    if !out.status.success() {
        return false;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let hex = line.split_whitespace().next().unwrap_or("");
    if hex.len() != 64 {
        return false;
    }
    let expected_hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
    hex.eq_ignore_ascii_case(&expected_hex)
}

fn build_output(
    status: Status,
    host: HostInfo,
    cpu: CpuInfo,
    adb_config: Option<AdbConfig>,
    adb_env: Option<AdbEnv>,
    environment: Option<Environment>,
    adb_results: Option<AdbResults>,
    error: Option<ErrorInfo>,
) -> Output {
    Output {
        schema_version: SCHEMA_VERSION,
        collector_version: COLLECTOR_VERSION.into(),
        collector_git_sha: COLLECTOR_GIT_SHA.into(),
        cpu_suite_version: CPU_SUITE_VERSION.into(),
        timestamp_utc: current_timestamp_utc(),
        status,
        host,
        cpu,
        config: None,
        adb_config,
        environment,
        adb_env,
        results: None,
        adb_results,
        frequency_series: None,
        error,
    }
}

fn emit_failure(
    json: bool,
    host: HostInfo,
    cpu: CpuInfo,
    adb_config: Option<AdbConfig>,
    adb_env: Option<AdbEnv>,
    error: ErrorInfo,
    exit_code: i32,
) -> i32 {
    let out = build_output(
        Status::Failed,
        host,
        cpu,
        adb_config,
        adb_env,
        None,
        None,
        Some(error),
    );
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
                eprintln!("adb: failed to serialize output: {e}");
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

fn print_human(out: &Output) {
    println!("status:         {:?}", out.status);
    println!("adb_suite:      {ADB_SUITE_VERSION}");
    if let Some(env) = &out.adb_env {
        if let Some(v) = &env.adb_version {
            let first = v.lines().next().unwrap_or(v);
            println!("adb_version:    {first}");
        }
    }
    if let Some(cfg) = &out.adb_config {
        if let Some(s) = &cfg.serial {
            println!("serial:         {s}");
        }
        println!("remote_path:    {}", cfg.remote_path);
        let sizes: Vec<String> = cfg
            .sizes
            .iter()
            .map(|s| format!("{}B×{}", s.size_bytes, s.iterations))
            .collect();
        println!("sizes:          {}", sizes.join(", "));
    }
    if let Some(err) = &out.error {
        println!("error.kind:     {}", err.kind);
        println!("error.message:  {}", err.message);
    }
    if let Some(r) = &out.adb_results {
        let n_push = r
            .iterations
            .iter()
            .filter(|i| i.direction == "push")
            .count();
        let n_pull = r
            .iterations
            .iter()
            .filter(|i| i.direction == "pull")
            .count();
        let bad = r.iterations.iter().filter(|i| !i.sha256_ok).count();
        println!("iterations:     push={n_push} pull={n_pull} sha_failed={bad}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_token_handles_suffixes() {
        assert_eq!(parse_size_token("25B").unwrap(), 25);
        assert_eq!(parse_size_token("25").unwrap(), 25);
        assert_eq!(parse_size_token("1K").unwrap(), 1024);
        assert_eq!(parse_size_token("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size_token("10m").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_size_token("1G").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_token_rejects_bad_input() {
        assert!(parse_size_token("").is_err());
        assert!(parse_size_token("0").is_err());
        assert!(parse_size_token("0M").is_err());
        assert!(parse_size_token("10x").is_err());
        assert!(parse_size_token("abc").is_err());
    }

    #[test]
    fn parse_iterations_arg_parses_map() {
        let v = parse_iterations_arg(Some("25B=50,1M=20")).unwrap();
        assert_eq!(v, vec![(25, 50), (1024 * 1024, 20)]);
    }

    #[test]
    fn parse_iterations_arg_rejects_zero_and_bad() {
        assert!(parse_iterations_arg(Some("25B=0")).is_err());
        assert!(parse_iterations_arg(Some("25B")).is_err());
        assert!(parse_iterations_arg(Some("25B=abc")).is_err());
    }

    #[test]
    fn resolve_specs_uses_defaults() {
        let specs = resolve_specs(None, None).unwrap();
        assert_eq!(specs.len(), DEFAULT_SIZES.len());
        let s25 = &specs[0];
        assert_eq!(s25.bytes, 25);
        assert_eq!(s25.iterations, 200);
    }

    #[test]
    fn resolve_specs_applies_overrides() {
        let specs = resolve_specs(None, Some("25B=10,1M=5")).unwrap();
        let s25 = specs.iter().find(|s| s.bytes == 25).unwrap();
        let s1m = specs.iter().find(|s| s.bytes == 1024 * 1024).unwrap();
        assert_eq!(s25.iterations, 10);
        assert_eq!(s1m.iterations, 5);
    }

    #[test]
    fn resolve_specs_requires_iterations_for_custom_size() {
        let err = resolve_specs(Some("7B"), None).unwrap_err();
        assert!(err.contains("no iteration count"));
        let ok = resolve_specs(Some("7B"), Some("7B=3")).unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].bytes, 7);
        assert_eq!(ok[0].iterations, 3);
    }

    #[test]
    fn generate_random_file_is_deterministic_size_and_hash_changes_per_path() {
        let dir = std::env::temp_dir().join(format!("fb-adb-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let p1 = dir.join("a.bin");
        let p2 = dir.join("b.bin");
        let h1 = generate_random_file(&p1, 1024).unwrap();
        let h2 = generate_random_file(&p2, 1024).unwrap();
        let meta = fs::metadata(&p1).unwrap();
        assert_eq!(meta.len(), 1024);
        // Different paths and different seed-time should yield different hashes
        // with overwhelming probability. We accept a tiny collision risk here.
        assert_ne!(h1, h2);
        let local_h = sha256_file(&p1).unwrap();
        assert_eq!(local_h, h1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn transfer_timestamp_is_precise_rfc3339_utc() {
        let timestamp = transfer_timestamp_utc();
        assert!(timestamp.ends_with('Z'));
        assert!(timestamp.contains('.'));
        let parsed = chrono::DateTime::parse_from_rfc3339(&timestamp).unwrap();
        assert_eq!(parsed.offset().local_minus_utc(), 0);
    }

    #[test]
    fn adb_iteration_serializes_transfer_timestamps() {
        let iteration = AdbIteration {
            device_serial: "serial".into(),
            device_model: "model".into(),
            hub_path: None,
            size_bytes: 25,
            direction: "push".into(),
            transfer_started_at_utc: Some("2026-07-16T12:00:00.123456Z".into()),
            transfer_finished_at_utc: Some("2026-07-16T12:00:00.123789Z".into()),
            bytes_per_sec: 1.0,
            elapsed_ms: 1.0,
            sha256_ok: true,
        };
        let value = serde_json::to_value(iteration).unwrap();
        assert_eq!(
            value["transfer_started_at_utc"],
            "2026-07-16T12:00:00.123456Z"
        );
        assert_eq!(
            value["transfer_finished_at_utc"],
            "2026-07-16T12:00:00.123789Z"
        );
    }
}
