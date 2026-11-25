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

    // Built again when the checked-out commit moves.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        let dir = Path::new(&dir);
        for watched in ["HEAD", "refs/heads"] {
            let path = dir.join(watched);
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
