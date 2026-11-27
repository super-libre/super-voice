// SPDX-License-Identifier: GPL-3.0-only
//! The inspector: what a backend is before it is installed, and what can be
//! done with it. Its name and version, what it is for (tags), the facts a
//! person agrees to by installing it (how it runs, what it needs and
//! reaches, what it runs on, its license and source), its models by kind,
//! and its actions: Install, or Update, Configure… and Uninstall….
//!
//! The same parts beside the table, in a sheet, and on a page of their own.

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::link::Link;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, div, px,
};
use super_voice_app_core::pages::add_backend::{Facts, RunsAs};
use super_voice_app_core::pages::library::{Framework, Input, Installing, Row, RowState, Runs};
use super_voice_app_core::pages::readiness::Feature;
use super_voice_app_core::rules::gpu::accel_name;
use super_voice_app_core::rules::load::{Size, size};

use super::{LibraryPage, Msg, kind_words, phase_words};
use crate::icons::Phosphor;
use crate::ui::status::{Dot, StatusPill};
use crate::{t, t_with};

/// The backend's tile, name and version, and its status when it needs
/// saying. `large` beside the table and on a page; smaller as a sheet's
/// title.
pub fn heading(row: &Row, large: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let icon: Icon = match row.runs {
        Runs::Local => Phosphor::Brain.into(),
        Runs::Cloud => Phosphor::Cloud.into(),
    };
    let version = if row.installed {
        t_with("library-version", &[("version", &row.version)])
    } else {
        t_with(
            "backend-version-source",
            &[
                ("value0", &row.version),
                ("where_from", &t("from-its-latest-release")),
            ],
        )
    };
    let status = match &row.state {
        RowState::NeedsKey => Some(StatusPill::new(Dot::Yellow, t("api-key-required"))),
        RowState::Failed(_) => Some(StatusPill::new(Dot::Red, t("failed"))),
        _ => None,
    };
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2p5()
        .items_center()
        .child(
            div()
                .flex_shrink_0()
                .size(px(if large { 32. } else { 28. }))
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .bg(theme.muted)
                .flex()
                .items_center()
                .justify_center()
                .child(icon.size_4().text_color(theme.muted_foreground)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .map(|name| {
                            if large {
                                name.text_base()
                            } else {
                                name.text_sm()
                            }
                        })
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(row.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(version),
                ),
        )
        .children(status)
        .into_any_element()
}

/// What the backend is: why it can't be installed, when it can't; its
/// description, tags, facts and models; an install under way.
pub fn body(row: &Row, narrow: bool, this: &WeakEntity<LibraryPage>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let _ = this;
    v_flex()
        .w_full()
        .gap_3()
        .children(notice(row))
        .when_some(row.description.clone(), |body, description| {
            body.child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(description),
            )
        })
        .child(tags(row))
        .child(facts(row, narrow, cx))
        .children(models(row, cx))
        .children(progress(row, cx))
        .into_any_element()
}

/// Why it can't be installed, or what went wrong with it.
fn notice(row: &Row) -> Option<AnyElement> {
    let id = SharedString::from(format!("notice-{}", row.source));
    let alert = match &row.state {
        RowState::CannotRun(reason) => Alert::warning(
            id,
            reason
                .clone()
                .map_or_else(|| t("this-backend-cannot-run-on-this-computer"), Into::into),
        ),
        RowState::NeedsNewerApp(version) => Alert::warning(
            id,
            match version {
                Some(version) => format!("{} ({version})", t("needs-a-newer-super-voice")).into(),
                None => t("needs-a-newer-super-voice"),
            },
        ),
        RowState::Failed(message) => Alert::error(id, t_with("failed-detail", &[("err", message)])),
        _ => {
            let error = row.uninstall_error.as_ref()?;
            Alert::error(id, t_with("uninstall-failed", &[("err", error)]))
        }
    };
    Some(alert.into_any_element())
}

/// What it is for and what it can do, as tags: its features, then what it
/// runs on, where it sends audio, and whether it hears a dictation context
/// or speaks in cloned voices.
fn tags(row: &Row) -> AnyElement {
    let tag = |icon: Icon, words: SharedString| {
        Tag::secondary().small().child(
            h_flex()
                .gap_1()
                .items_center()
                .child(icon.size_3())
                .child(words),
        )
    };
    let with_tip = |id: &str, tag: Tag, tip: SharedString| {
        div()
            .id(SharedString::from(format!("tag-{id}")))
            .child(tag)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .into_any_element()
    };
    let mut tags: Vec<AnyElement> = row
        .features
        .iter()
        .map(|feature| {
            let icon: Icon = match feature {
                Feature::SpeechToText => Phosphor::Microphone.into(),
                Feature::TextToSpeech => Phosphor::SpeakerHigh.into(),
            };
            tag(icon, t(feature.key())).into_any_element()
        })
        .collect();
    let caps = &row.capabilities;
    if caps.compute.gpu {
        tags.push(with_tip(
            "gpu",
            tag(Phosphor::GraphicsCard.into(), "GPU".into()),
            t("accelerated-on-gpu"),
        ));
    }
    if caps.compute.cpu {
        tags.push(with_tip(
            "cpu",
            tag(Phosphor::Cpu.into(), "CPU".into()),
            t("runs-on-the-cpu"),
        ));
    }
    if let Some(egress) = &caps.cloud {
        let mut tip = t("transmits-audio-to").to_string();
        for host in &egress.hosts {
            tip.push_str("\n• ");
            tip.push_str(host);
        }
        if egress.user_url {
            tip.push('\n');
            tip.push_str(&t("another-url-you-set"));
        }
        tags.push(with_tip(
            "cloud",
            tag(Phosphor::Cloud.into(), t("cloud")),
            tip.into(),
        ));
    }
    if caps.contexts {
        tags.push(with_tip(
            "contexts",
            tag(Phosphor::ChatText.into(), t("contexts")),
            t("uses-your-dictation-context"),
        ));
    }
    if caps.voice_cloning {
        tags.push(with_tip(
            "cloning",
            tag(Phosphor::UserSound.into(), t("voice-cloning")),
            t("speaks-in-voices-you-clone"),
        ));
    }
    h_flex()
        .flex_wrap()
        .gap_1p5()
        .children(tags)
        .into_any_element()
}

/// A size in words: "38 MB", "1.6 GB".
fn size_words(bytes: u64) -> SharedString {
    match size(bytes) {
        Size::Mb(n) => t_with("size-mb", &[("size", &n)]),
        Size::Gb(n) => t_with("size-gb", &[("size", &n)]),
    }
}

/// The frameworks it runs on, best first, each with the download its build
/// takes when it isn't on this computer.
fn frameworks_words(frameworks: &[Framework]) -> String {
    frameworks
        .iter()
        .map(|f| match f.download {
            Some(bytes) => format!("{} ({})", accel_name(&f.accel), size_words(bytes)),
            None => accel_name(&f.accel),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The facts a person agrees to by installing it.
fn facts(row: &Row, narrow: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let Facts {
        runs_as,
        needs,
        reaches,
        built_for,
        license,
    } = &row.facts;
    let mono = |text: String| {
        div()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .child(text)
            .into_any_element()
    };
    let runs_as: SharedString = match runs_as {
        RunsAs::Wasm => t("wasm-component-sandboxed"),
        RunsAs::Subprocess => t("native-subprocess"),
        RunsAs::Other(kind) => kind.clone().into(),
    };
    let mut list = DescriptionList::new()
        .small()
        .bordered(true)
        .columns(1)
        .label_width(px(if narrow { 88. } else { 96. }))
        .item(t("runs-as"), runs_as, 1);
    if !needs.is_empty() {
        list = list.item(t("needs"), needs.join(", "), 1);
    }
    // Always, empty too: "nothing, runs offline" is what someone installing
    // a microphone reader looks for.
    list = if reaches.is_empty() {
        list.item(
            t("reaches"),
            div()
                .text_color(theme.success)
                .child(t("nothing-runs-offline"))
                .into_any_element(),
            1,
        )
    } else {
        list.item(t("reaches"), mono(reaches.join(", ")), 1)
    };
    if !row.frameworks.is_empty() {
        list = list.item(t("runs-on"), frameworks_words(&row.frameworks), 1);
    }
    if let Some(built_for) = built_for {
        list = list.item(t("built-for"), mono(built_for.clone()), 1);
    }
    if !license.is_empty() {
        list = list.item(t("license"), license.clone(), 1);
    }
    let address = row.repository.clone();
    list = list.item(
        t("source"),
        Link::new(SharedString::from(format!("source-{}", row.source)))
            .href(address)
            .child(div().truncate().child(row.source.clone()))
            .into_any_element(),
        1,
    );
    div().text_sm().child(list).into_any_element()
}

/// Its models, by kind: each kind's name, then its models.
fn models(row: &Row, cx: &App) -> Option<AnyElement> {
    if row.models.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let groups = row.models.iter().map(|group| {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(kind_words(group.kind)),
            )
            .child(
                v_flex()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .children(group.names.iter().enumerate().map(|(i, name)| {
                        div()
                            .px_2p5()
                            .py_1p5()
                            .when(i > 0, |line| line.border_t_1().border_color(theme.border))
                            .font_family(theme.mono_font_family.clone())
                            .text_xs()
                            .truncate()
                            .child(name.clone())
                    })),
            )
    });
    Some(
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(t("models")),
            )
            .children(groups)
            .into_any_element(),
    )
}

/// An install or update under way: its step, and how far.
fn progress(row: &Row, cx: &App) -> Option<AnyElement> {
    let Installing::Running { phase, fraction } = &row.installing else {
        return None;
    };
    let theme = cx.theme();
    let words = if row.installed {
        t_with(
            "update-backend-progress",
            &[("value0", &phase_words(*phase))],
        )
    } else {
        t_with(
            "install-backend-progress",
            &[("value0", &phase_words(*phase))],
        )
    };
    Some(
        v_flex()
            .gap_2()
            .p_3()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .justify_between()
                    .text_sm()
                    .child(words)
                    .children(fraction.map(|f| {
                        div()
                            .text_color(theme.muted_foreground)
                            .child(format!("{}%", (f * 100.).round()))
                    })),
            )
            .child(
                Progress::new(SharedString::from(format!(
                    "inspector-progress-{}",
                    row.source
                )))
                .small()
                .loading(fraction.is_none())
                .value(fraction.unwrap_or(0.) * 100.),
            )
            .into_any_element(),
    )
}

/// What can be done with it: Install, or Update, Configure… and
/// Uninstall….
pub fn actions(row: &Row, this: &WeakEntity<LibraryPage>, cx: &App) -> AnyElement {
    let _ = cx;
    let busy = matches!(row.installing, Installing::Running { .. });
    let source = row.source.clone();
    let bar = h_flex().w_full().flex_wrap().gap_2().items_center();
    if !row.installed {
        let label = if busy {
            t("installing-progress")
        } else {
            t_with("install-backend", &[("value0", &row.name)])
        };
        return bar
            .child(
                Button::new("install")
                    .primary()
                    .icon(Phosphor::DownloadSimple)
                    .label(label)
                    .loading(busy)
                    .disabled(!row.installable())
                    .on_click(LibraryPage::press(
                        this,
                        Msg::Library(Input::Install(source)),
                    )),
            )
            .into_any_element();
    }
    let configure = {
        let this = this.clone();
        let source = source.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
            let source = source.clone();
            _ = this.update(cx, |page, cx| page.configure(&source, window, cx));
        }
    };
    // Configure… and Uninstall… share a row, Uninstall… at its far end;
    // an update offered goes above them on a row of its own, the width of
    // the panel, so three buttons never wrap one onto a line alone.
    let others = h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(
            Button::new("configure")
                .outline()
                .label(t("library-configure"))
                .disabled(busy)
                .on_click(configure),
        )
        .child(div().flex_1())
        .child(
            Button::new("uninstall")
                .ghost()
                .label(t("library-uninstall"))
                .disabled(busy)
                .on_click(LibraryPage::press(
                    this,
                    Msg::Library(Input::AskUninstall(source.clone())),
                )),
        );
    v_flex()
        .w_full()
        .gap_2()
        .when_some(row.update_to.clone(), |actions, version| {
            actions.child(
                Button::new("update")
                    .primary()
                    .w_full()
                    .icon(Phosphor::DownloadSimple)
                    .label(t_with("update-to-version", &[("version", &version)]))
                    .on_click(LibraryPage::press(
                        this,
                        Msg::Library(Input::Update(source.clone())),
                    )),
            )
        })
        .child(others)
        .into_any_element()
}
