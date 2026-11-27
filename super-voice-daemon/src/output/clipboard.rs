// SPDX-License-Identifier: GPL-3.0-only
//! The clipboard, where a dictation goes when it can't be typed.
//!
//! A take whose words could not be typed (no keyboard, the permission to
//! type switched off, another app holding secure keyboard entry) is not
//! thrown away: its words go to the clipboard, and the user pastes them.
//!
//! On Linux through Wayland's data-control protocol, which `wl-clipboard-rs`
//! speaks for the read path already (`tts/input/screen_text.rs`): it serves
//! the text from a thread of its own until something else is copied. On
//! macOS through `pbcopy`, which writes the general pasteboard of the user's
//! session, where the daemon runs as a `LaunchAgent`.
//!
//! Tests never reach the real clipboard, which would be the developer's:
//! under `cfg(test)` the words land in [`copied`] instead.

use anyhow::Result;

/// Put `text` on the clipboard.
///
/// # Errors
/// When the session has no clipboard the daemon can write: no Wayland
/// compositor speaking data-control, or `pbcopy` failing.
#[cfg(not(test))]
pub async fn copy(text: String) -> Result<()> {
    tokio::task::spawn_blocking(move || platform::copy(&text))
        .await
        .map_err(|e| anyhow::anyhow!("the clipboard task panicked: {e}"))?
}

#[cfg(all(not(test), target_os = "linux"))]
mod platform {
    use anyhow::{Context as _, Result};
    use wl_clipboard_rs::copy::{MimeType, Options, Source};

    /// Hand the text to a thread that serves it until something else is
    /// copied; returns once the compositor has the offer.
    pub fn copy(text: &str) -> Result<()> {
        Options::new()
            .copy(Source::Bytes(text.as_bytes().into()), MimeType::Text)
            .context("could not put the words on the clipboard")
    }
}

#[cfg(all(not(test), target_os = "macos"))]
mod platform {
    use anyhow::{Context as _, Result, bail};
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    /// `pbcopy`, told the text is UTF-8: it reads its input in the locale's
    /// encoding, and launchd starts the daemon with no locale at all, which
    /// would turn every accented letter into two.
    pub fn copy(text: &str) -> Result<()> {
        let mut child = Command::new("/usr/bin/pbcopy")
            .env("LANG", "en_US.UTF-8")
            .env("LC_CTYPE", "UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not run pbcopy")?;
        child
            .stdin
            .take()
            .context("pbcopy has no input")?
            .write_all(text.as_bytes())
            .context("could not hand pbcopy the words")?;
        let out = child.wait_with_output().context("pbcopy did not finish")?;
        if !out.status.success() {
            bail!(
                "pbcopy failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }
}

/// What tests copied, in order.
#[cfg(test)]
static COPIED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The test copy: the words land in [`copied`], never on the clipboard.
#[cfg(test)]
#[allow(
    clippy::unused_async,
    reason = "shares a signature with the real copy, which awaits its thread"
)]
pub async fn copy(text: String) -> Result<()> {
    COPIED.lock().expect("copied words").push(text);
    Ok(())
}

/// Everything tests copied so far.
#[cfg(test)]
pub fn copied() -> Vec<String> {
    COPIED.lock().expect("copied words").clone()
}
