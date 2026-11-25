// SPDX-License-Identifier: GPL-3.0-only
//! The commit the library is built from, for the app's About window: the
//! same variables, and the same way of overriding them, as the settings
//! app's build.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut vergen = vergen::EmitBuilder::builder();

    println!("cargo:rerun-if-env-changed=VERGEN_GIT_COMMIT_DATE");
    if std::env::var_os("VERGEN_GIT_COMMIT_DATE").is_none() {
        vergen.git_commit_date();
    }

    println!("cargo:rerun-if-env-changed=VERGEN_GIT_SHA");
    if std::env::var_os("VERGEN_GIT_SHA").is_none() {
        vergen.git_sha(false);
    }
    vergen.fail_on_error().emit()?;
    Ok(())
}
