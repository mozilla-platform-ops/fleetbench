//! Bake the git SHA into the binary so `--version` and the JSON envelope
//! identify exactly which commit produced any artifact. Without this, there's
//! no way to tell two near-identical builds apart after they leave the dev's
//! machine — a recurring source of "is this the new binary?" confusion.

use std::process::Command;

fn main() {
    let sha = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());

    // Dirty = uncommitted changes to tracked files. Untracked files don't
    // count (target/ etc would always flag it otherwise).
    let dirty = Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false);

    let full = if dirty { format!("{sha}-dirty") } else { sha };
    println!("cargo:rustc-env=FLEETBENCH_GIT_SHA={full}");

    // Rebuild when HEAD moves, the index changes, or a ref updates (branch
    // switch). Paths are relative to this crate's directory.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/index");
    println!("cargo:rerun-if-changed=../.git/refs");
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
