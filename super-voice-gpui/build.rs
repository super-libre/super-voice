// SPDX-License-Identifier: GPL-3.0-only
//! The commit the app is built from, for its About page:
//! `SUPER_VOICE_GIT_SHA` and `SUPER_VOICE_GIT_DATE`, empty when unknown.
//!
//! A vendored build (`just vendor`) has no git history and sets
//! `VERGEN_GIT_SHA` and `VERGEN_GIT_COMMIT_DATE` instead, which win.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=VERGEN_GIT_SHA");
    println!("cargo:rerun-if-env-changed=VERGEN_GIT_COMMIT_DATE");
    let sha = std::env::var("VERGEN_GIT_SHA")
        .ok()
        .or_else(|| git(&["rev-parse", "HEAD"]));
    let date = std::env::var("VERGEN_GIT_COMMIT_DATE")
        .ok()
        .or_else(|| git(&["log", "-1", "--format=%cs"]));
    println!(
        "cargo:rustc-env=SUPER_VOICE_GIT_SHA={}",
        sha.unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=SUPER_VOICE_GIT_DATE={}",
        date.unwrap_or_default()
    );

    // Built again when the checked-out commit moves: HEAD, the branch it
    // names, and packed-refs, where a branch can live instead. `--git-path`
    // finds each one in a worktree too, whose branches are in the main
    // checkout's git dir.
    let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(branch);
    }
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--git-path", &name]) {
            let path = Path::new(&path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

/// `git args`'s output, trimmed, or `None` when git can't say.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}
