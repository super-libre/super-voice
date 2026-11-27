// SPDX-License-Identifier: GPL-3.0-only
//! The status bar: whether the daemon answers, each feature's readiness in
//! words where the sidebar's headings don't show it, and the GPU's memory.
//!
//! - Wide: the daemon, the GPU's name and memory; the version on the right.
//!   The sidebar's headings carry each feature's readiness.
//! - Compact: the daemon, then each feature with its dot and word (the rail
//!   has only the dots); the GPU's memory on the right.
//! - Narrow: each feature's icon, dot and word; the GPU's memory.
//! - While the daemon is down: only that, at every width.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{AnyElement, Context, IntoElement, ParentElement, SharedString, Styled};
use super_voice_app_core::pages::readiness::{Feature, GpuMemory};
use super_voice_app_core::rules::gpu::gib;
use super_voice_app_core::state::connection::DaemonStatus;

use super::{Shell, version};
use crate::ui::status::{Dot, StatusDot};
use crate::ui::width::WindowSize;
use crate::{t, t_with};

pub fn render(shell: &Shell, size: WindowSize, cx: &mut Context<Shell>) -> impl IntoElement {
    let connection = shell.shared.connection.read(cx);
    let mut left: Vec<AnyElement> = Vec::new();
    let mut right: Vec<AnyElement> = Vec::new();
    if let Some(failure) = connection.failure() {
        left.push(item(Some(Dot::Red), None, down_words(failure)));
        if size == WindowSize::Wide {
            right.push(version().into_any_element());
        }
        return bar(left, right);
    }
    if size != WindowSize::Narrow {
        let (dot, words) = daemon(connection.status());
        left.push(item(Some(dot), None, words));
    }
    let readiness = shell.shared.readiness.read(cx);
    if size != WindowSize::Wide {
        for feature in Feature::ALL {
            let status = readiness.of(feature);
            let words = if size == WindowSize::Compact {
                format!("{} ", t(feature.key())).into()
            } else {
                SharedString::default()
            };
            left.push(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(feature_icon(feature).size_3())
                    .when_some((!words.is_empty()).then_some(words), ParentElement::child)
                    .child(StatusDot::new(status.dot()))
                    .child(t(status.key()))
                    .into_any_element(),
            );
        }
    }
    let gpu = readiness
        .gpu()
        .map(|gpu| gpu_words(&gpu, size == WindowSize::Wide));
    match (size, gpu) {
        (WindowSize::Wide, Some(gpu)) => left.push(gpu_item(gpu)),
        (_, Some(gpu)) => right.push(gpu_item(gpu)),
        (_, None) => {}
    }
    if size == WindowSize::Wide {
        right.push(version().into_any_element());
    }
    bar(left, right)
}

fn bar(left: Vec<AnyElement>, right: Vec<AnyElement>) -> StatusBar {
    let separated = |items: Vec<AnyElement>| {
        let mut row = h_flex().gap_2().items_center().min_w_0();
        for (ix, item) in items.into_iter().enumerate() {
            if ix > 0 {
                row = row.child(Separator::vertical().h_3());
            }
            row = row.child(item);
        }
        row
    };
    StatusBar::new()
        .left(separated(left))
        .right(separated(right))
}

/// An item: a dot or an icon, and words.
fn item(dot: Option<Dot>, icon: Option<Icon>, words: SharedString) -> AnyElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .whitespace_nowrap()
        .children(icon.map(Styled::size_3))
        .children(dot.map(StatusDot::new))
        .child(words)
        .into_any_element()
}

fn gpu_item(words: SharedString) -> AnyElement {
    item(None, Some(Icon::from(Lucide::Microchip)), words)
}

/// The GPU's memory: "RTX 3090 · 6.1 / 24.0 GiB", or without the name.
fn gpu_words(gpu: &GpuMemory, with_name: bool) -> SharedString {
    let memory = match gpu.used {
        Some(used) => t_with(
            "memory-pair",
            &[("value0", &gib(used)), ("value1", &gib(gpu.total))],
        ),
        None => t_with("memory-size", &[("gib", &gib(gpu.total))]),
    };
    if with_name {
        format!("{} · {memory}", gpu.name).into()
    } else {
        memory
    }
}

/// The feature's icon: the microphone, or the speaker.
pub fn feature_icon(feature: Feature) -> Icon {
    match feature {
        Feature::SpeechToText => Icon::from(gpui_kit::component::IconName::Mic),
        Feature::TextToSpeech => Icon::from(Lucide::Volume2),
    }
}

/// The daemon's dot and words while it isn't down: green when it answers,
/// grey while the app asks.
fn daemon(status: &DaemonStatus) -> (Dot, SharedString) {
    match status {
        DaemonStatus::Connected => (Dot::Green, t("daemon-connected")),
        DaemonStatus::Error(reason) => (
            Dot::Red,
            t_with("daemon-not-answering", &[("reason", reason)]),
        ),
        DaemonStatus::Blocked(reason) => (
            Dot::Red,
            t_with("daemon-access-refused", &[("reason", reason)]),
        ),
        DaemonStatus::Disconnected | DaemonStatus::Connecting => {
            (Dot::Grey, t("daemon-connecting"))
        }
    }
}

/// What the bar says while the daemon is down, in a word or two: the
/// Connection page says the rest.
fn down_words(failure: &DaemonStatus) -> SharedString {
    match failure {
        DaemonStatus::Blocked(_) => t("status-access-refused"),
        _ => t("status-daemon-not-running"),
    }
}
