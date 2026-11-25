// SPDX-License-Identifier: GPL-3.0-only
//! Loading a stage's model: what a Load click sends, what a card says about
//! where the model runs, and how a failure reads.

use crate::daemon::backends::BackendInfo;

/// What a Load click resolves to.
#[derive(Debug, PartialEq, Eq)]
pub enum StagedLoad {
    /// The staged pair is no longer in the installed-backend catalog: the
    /// click is stale and must not reach the daemon.
    NotInCatalog,
    /// Load the staged model, first setting the device when `Some`.
    Load { device_to_set: Option<String> },
}

/// Resolve a Load click against the installed-backend catalog.
///
/// For online models (the `none` sentinel in `supported_devices`) there is no
/// device to set; otherwise the staged device is sent as the model's own. The
/// daemon compares it with where the model already is, so resending an
/// unchanged device costs nothing.
#[must_use]
pub fn staged_load_device(
    backends: &[BackendInfo],
    source: &str,
    model: &str,
    staged_device: Option<&str>,
) -> StagedLoad {
    let Some(online) = backends
        .iter()
        .find(|b| b.source == source)
        .and_then(|b| b.models.iter().find(|m| m.name == model))
        .map(|m| m.supported_devices.iter().any(|d| d == "none"))
    else {
        return StagedLoad::NotInCatalog;
    };
    StagedLoad::Load {
        device_to_set: if online {
            None
        } else {
            staged_device
                .filter(|d| *d != "none")
                .map(ToString::to_string)
        },
    }
}

/// The framework a running model fell back to, when it is not the one
/// picked: what a card notes under its Runs on row. `None` when they agree, as
/// they nearly always do, and for a model that runs nowhere local.
#[must_use]
pub fn fallback<'a>(chosen: Option<&str>, running: &'a str) -> Option<&'a str> {
    match (chosen, running) {
        (None, _) | (_, "" | "none" | "remote") => None,
        (Some(chosen), running) if chosen == running => None,
        (Some(_), running) => Some(running),
    }
}

/// An error as a card shows it: the user's home directory collapsed back to
/// `$HOME`, at most 200 characters. Skips the substitution when `home` is
/// empty: an empty `from` makes `str::replace` insert the replacement at every
/// char boundary, shredding the message.
#[must_use]
pub fn sanitize_home(err: &str, home: &str) -> String {
    if home.is_empty() {
        err.chars().take(200).collect()
    } else {
        err.replace(home, "$HOME").chars().take(200).collect()
    }
}

/// A size as a card says it: whole megabytes below a gigabyte, gigabytes to
/// one decimal above.
#[derive(Debug, Clone, PartialEq)]
pub enum Size {
    /// Megabytes, rounded up.
    Mb(String),
    /// Gigabytes, to one decimal.
    Gb(String),
}

/// `bytes` as a [`Size`].
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn size(bytes: u64) -> Size {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb < 1024.0 {
        Size::Mb(mb.ceil().to_string())
    } else {
        Size::Gb(format!("{:.1}", mb / 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::backends::BackendModel;

    fn backend(source: &str, model: &str, devices: &[&str]) -> BackendInfo {
        BackendInfo {
            source: source.to_string(),
            name: String::new(),
            description: String::new(),
            version: String::new(),
            kind: String::new(),
            allowed_hosts: Vec::new(),
            installed_accel: Vec::new(),
            models: vec![BackendModel {
                name: model.to_string(),
                provider: String::new(),
                supported_devices: devices.iter().map(ToString::to_string).collect(),
                estimated_vram_bytes: 0,
                multilingual: false,
                supported_languages: Vec::new(),
                primary_language: String::new(),
                realtime: false,
                role: "transcription".into(),
                accepts_context: false,
                clones_voices: false,
            }],
            secrets: Vec::new(),
            options: Vec::new(),
            supports: Vec::new(),
        }
    }

    #[test]
    fn a_stale_pair_sends_nothing() {
        let backends = [backend("src/a", "base", &["cpu"])];
        assert_eq!(
            staged_load_device(&backends, "src/b", "base", Some("cpu")),
            StagedLoad::NotInCatalog
        );
    }

    #[test]
    fn an_online_model_sets_no_device() {
        let backends = [backend("src/a", "cloud", &["none"])];
        assert_eq!(
            staged_load_device(&backends, "src/a", "cloud", Some("gpu")),
            StagedLoad::Load {
                device_to_set: None
            }
        );
    }

    #[test]
    fn a_local_model_sets_the_staged_device() {
        let backends = [backend("src/a", "base", &["cpu", "gpu"])];
        assert_eq!(
            staged_load_device(&backends, "src/a", "base", Some("gpu")),
            StagedLoad::Load {
                device_to_set: Some("gpu".into())
            }
        );
    }

    /// Only a model running on another framework than picked says so.
    #[test]
    fn a_fallback_is_noted() {
        assert_eq!(fallback(Some("vulkan"), "vulkan"), None);
        assert_eq!(fallback(Some("cuda"), "cpu"), Some("cpu"));
        assert_eq!(fallback(Some("cpu"), "remote"), None);
        assert_eq!(fallback(None, "cpu"), None);
    }

    #[test]
    fn home_is_collapsed_and_the_message_capped() {
        assert_eq!(
            sanitize_home("load failed at /a/b", ""),
            "load failed at /a/b"
        );
        assert_eq!(
            sanitize_home("no file /home/jo/models/x", "/home/jo"),
            "no file $HOME/models/x"
        );
        assert_eq!(sanitize_home(&"x".repeat(300), "").chars().count(), 200);
    }

    #[test]
    fn a_size_reads_in_mb_then_gb() {
        assert_eq!(size(640 * 1024 * 1024), Size::Mb("640".into()));
        assert_eq!(size(1536 * 1024 * 1024), Size::Gb("1.5".into()));
        assert_eq!(size(1), Size::Mb("1".into()));
    }
}
