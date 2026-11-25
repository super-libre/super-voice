// SPDX-License-Identifier: GPL-3.0-only
//! How the apps show a GPU: its name, short, how full its memory is, and
//! the frameworks models run on.

/// A GPU's name without the vendor's marketing prefix, for compact display
/// (`"NVIDIA GeForce RTX 3090"` → `"RTX 3090"`). The full name goes in the
/// tooltip.
#[must_use]
pub fn short_name(name: &str) -> &str {
    const PREFIXES: &[&str] = &[
        "NVIDIA GeForce ",
        "NVIDIA ",
        "AMD Radeon ",
        "AMD ",
        "Intel Arc ",
        "Intel ",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest;
        }
    }
    name
}

/// A byte count in GiB with one decimal, as memory is shown: `6.1`. The
/// unit is the words' (`memory-size`, `memory-pair`).
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / f64::from(1u32 << 30))
}

/// A framework as people name it, from the id the daemon uses: `cuda` is
/// CUDA, `vulkan` Vulkan. An id the app doesn't know is shown as it is.
#[must_use]
pub fn accel_name(accel: &str) -> String {
    match accel {
        "cpu" => "CPU",
        "cuda" => "CUDA",
        "rocm" => "ROCm",
        "vulkan" => "Vulkan",
        "metal" => "Metal",
        other => other,
    }
    .to_string()
}

/// How full a GPU's memory is, as its meter's colour says: green, then yellow
/// from three quarters, then red from nine tenths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterLevel {
    Ok,
    High,
    Full,
}

/// How much of the meter is filled, from 0 to 1.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn meter_fraction(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f32 / total as f32).clamp(0.0, 1.0)
    }
}

/// The meter's colour for `used` of `total` bytes.
#[must_use]
pub fn meter_level(used: u64, total: u64) -> MeterLevel {
    let fraction = meter_fraction(used, total);
    if fraction >= 0.9 {
        MeterLevel::Full
    } else if fraction >= 0.75 {
        MeterLevel::High
    } else {
        MeterLevel::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_shows_in_gib_with_one_decimal() {
        assert_eq!(gib(0), "0.0");
        assert_eq!(gib(24 << 30), "24.0");
        assert_eq!(gib(6_550_000_000), "6.1");
    }

    #[test]
    fn frameworks_go_by_their_names() {
        assert_eq!(accel_name("cuda"), "CUDA");
        assert_eq!(accel_name("rocm"), "ROCm");
        assert_eq!(accel_name("vulkan"), "Vulkan");
        assert_eq!(accel_name("cpu"), "CPU");
        assert_eq!(accel_name("tpu"), "tpu");
    }

    #[test]
    fn the_vendor_prefix_goes() {
        assert_eq!(short_name("NVIDIA GeForce RTX 3090"), "RTX 3090");
        assert_eq!(short_name("AMD Radeon RX 7900 XTX"), "RX 7900 XTX");
        assert_eq!(short_name("Apple M2 Pro"), "Apple M2 Pro");
    }

    #[test]
    fn the_meter_turns_yellow_then_red() {
        assert_eq!(meter_level(2, 10), MeterLevel::Ok);
        assert_eq!(meter_level(8, 10), MeterLevel::High);
        assert_eq!(meter_level(95, 100), MeterLevel::Full);
        assert_eq!(meter_level(1, 0), MeterLevel::Ok);
    }
}
