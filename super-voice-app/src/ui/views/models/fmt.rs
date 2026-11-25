// SPDX-License-Identifier: GPL-3.0-only
use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;

use crate::ui::messages::Message;

pub(crate) use super_voice_app_core::rules::gpu::accel_name;
use super_voice_app_core::rules::load::Size;

/// Format a byte count as a one-decimal GiB string (e.g. `"24.0 GiB"`).
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
pub(super) fn fmt_gib(bytes: u64) -> String {
    let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    crate::fl!("memory-size", gib = format!("{:.1}", gib))
}

/// Two byte counts as a shared-unit GiB pair, e.g. `"3.2 / 24.0 GiB"` — the
/// header's live "used / total" GPU memory readout, refreshed by polling.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
pub(super) fn fmt_gib_pair(used: u64, total: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    crate::fl!(
        "memory-pair",
        value0 = format!("{:.1}", used as f64 / GIB),
        value1 = format!("{:.1}", total as f64 / GIB)
    )
}

/// A GPU's name without the vendor's prefix; the rule is the core's.
pub(super) use super_voice_app_core::rules::gpu::short_name as short_gpu_name;

/// A compact fixed-width used/total memory meter for the header GPU readout: a
/// neutral rounded track with an accent fill whose width tracks utilization.
/// The fill shifts success → warning → destructive as the card fills up, so
/// memory pressure reads at a glance without needing the tooltip.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
pub(super) fn vram_meter<'a>(used: u64, total: u64) -> Element<'a, Message> {
    use super_voice_app_core::rules::gpu::{MeterLevel, meter_fraction, meter_level};
    let frac = meter_fraction(used, total);
    let level = meter_level(used, total);
    let track_w = 72.0_f32;
    let height = 6.0_f32;
    let fill_w = (track_w * frac).clamp(2.0, track_w);

    let fill = widget::container(widget::text(""))
        .width(Length::Fixed(fill_w))
        .height(Length::Fixed(height))
        .class(cosmic::theme::Container::custom(move |theme| {
            let cosmic = theme.cosmic();
            let color: cosmic::iced::Color = match level {
                MeterLevel::Full => cosmic.destructive.base.into(),
                MeterLevel::High => cosmic.warning.base.into(),
                MeterLevel::Ok => cosmic.success.base.into(),
            };
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(color)),
                border: cosmic::iced::Border {
                    radius: (height / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }));

    widget::container(fill)
        .width(Length::Fixed(track_w))
        .height(Length::Fixed(height))
        .class(cosmic::theme::Container::custom(move |theme| {
            let mut bg: cosmic::iced::Color = theme.cosmic().palette.neutral_5.into();
            bg.a = 0.25;
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(bg)),
                border: cosmic::iced::Border {
                    radius: (height / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// The advisory "may not fit" row shown under the staged picker: a yellow
/// warning glyph plus the model's estimated VRAM against what the GPU has.
/// Non-blocking — the Load button stays enabled; this only flags the risk.
pub(super) fn vram_warning<'a>(needed: u64, available: u64) -> Element<'a, Message> {
    use crate::ui::icons;
    use cosmic::iced::Alignment;
    use cosmic::iced::widget::row;
    use cosmic::widget::text;

    row![
        icons::phosphor_warning(icons::WARNING, 16.0),
        text::body(crate::fl!(
            "model-memory-warning",
            value0 = fmt_gib(needed),
            value1 = fmt_gib(available)
        )),
    ]
    .spacing(cosmic::theme::spacing().space_xs)
    .align_y(Alignment::Center)
    .into()
}

/// The advisory shown under the staged picker in place of a viable device: a
/// local model whose `offered_devices` came back empty because this specific
/// install cannot run it on any device (e.g. a GPU-only model with only a
/// CPU asset installed), not because it needs none. Unlike [`vram_warning`]
/// this one is blocking — the Load button is disabled while it shows, so a
/// silently inert button doesn't ship with no explanation.
pub(super) fn no_viable_device_warning<'a>(model: &str) -> Element<'a, Message> {
    use crate::ui::icons;
    use cosmic::iced::Alignment;
    use cosmic::iced::widget::row;
    use cosmic::widget::text;

    row![
        icons::phosphor_warning(icons::WARNING, 16.0),
        text::body(crate::fl!(
            "model-device-unavailable",
            model = (model).to_string()
        )),
    ]
    .spacing(cosmic::theme::spacing().space_xs)
    .align_y(Alignment::Center)
    .into()
}

/// A download's size, e.g. `"640 MB"` or `"1.4 GB"`.
pub(super) fn fmt_size(bytes: u64) -> String {
    match super_voice_app_core::rules::load::size(bytes) {
        Size::Mb(size) => crate::fl!("size-mb", size = size),
        Size::Gb(size) => crate::fl!("size-gb", size = size),
    }
}

/// The line under a Runs on row when the model is not running on the
/// framework picked, which is one that fell back to the CPU.
pub(super) fn running_note(chosen: Option<&str>, running: &str) -> Option<String> {
    super_voice_app_core::rules::load::fallback(chosen, running)
        .map(|framework| crate::fl!("running-on-framework", framework = accel_name(framework)))
}

#[cfg(test)]
mod device_name_tests {
    use super::*;

    #[test]
    fn a_download_reads_in_mb_then_gb() {
        let size = |bytes| crate::i18n::without_isolation(&fmt_size(bytes));
        assert_eq!(size(640 * 1024 * 1024), "640 MB");
        assert_eq!(size(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(size(1), "1 MB");
    }

    /// Only a model running on another framework than picked says so.
    #[test]
    fn a_fallback_is_noted() {
        assert_eq!(running_note(Some("vulkan"), "vulkan"), None);
        assert_eq!(running_note(Some("cpu"), "cpu"), None);
        assert_eq!(
            running_note(Some("cuda"), "cpu").map(|note| crate::i18n::without_isolation(&note)),
            Some("Running on CPU".to_string())
        );
        assert_eq!(running_note(Some("cpu"), "remote"), None);
        assert_eq!(running_note(None, "cpu"), None);
    }
}
