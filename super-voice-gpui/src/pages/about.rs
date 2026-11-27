// SPDX-License-Identifier: GPL-3.0-only
//! The About page: which Super Voice this is (its version and commit), which
//! daemon it talks to, the system, its license, and where its source is and
//! problems go. Copy details puts it all on the clipboard for a report.
//!
//! The example of a whole page over a core page (`pages::about`): the view
//! keeps the core's state, sends each press through the core's `update`, and
//! runs what it returns with [`effects`]: the daemon's version is read on the
//! core's runtime, while copying and opening a link happen at once.

use gpui_kit::component::button::Button;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::link::Link;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, img, px,
};
use super_voice_app_core::pages::about::{
    self as page, AboutState, Build, DaemonVersion, Effect, Input, Link as Address, Notice,
};

use crate::assets::APP_ICON;
use crate::effects::{self, CorePage, Runner, runner};
use crate::icons::Phosphor;
use crate::pages::{Page, PageView};
use crate::shared::Shared;
use crate::ui::card::Card;
use crate::ui::page_header::PageHeader;
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, version};

pub struct AboutPage {
    state: AboutState,
}

/// This build, as the core's About page takes it.
fn build() -> Build {
    let known = |value: &str| (!value.is_empty()).then(|| value.to_string());
    Build {
        version: env!("CARGO_PKG_VERSION").to_string(),
        commit: known(env!("SUPER_VOICE_GIT_SHA")),
        commit_date: known(env!("SUPER_VOICE_GIT_DATE")),
        repository: env!("CARGO_PKG_REPOSITORY").to_string(),
    }
}

impl PageView for AboutPage {
    fn new(_: Page, _: &Shared, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            state: AboutState::new(build()),
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(Input::Reached, window, cx);
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, _: &[&str], _: &mut Window, _: &mut Context<Self>) {
        self.state.daemon = DaemonVersion::Known(env!("CARGO_PKG_VERSION").to_string());
    }
}

impl CorePage for AboutPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = page::update(&mut self.state, input);
        effects::run_effects(self, outcome.effects, window, cx);
        toast::notice::<Self>(
            window,
            cx,
            outcome.clear_notice,
            outcome.notice.map(|notice| match notice {
                Notice::Copied => Toast::success(t("about-copied")),
            }),
        );
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        match effect {
            Effect::ReadDaemonVersion => Some(runner(page::run_read_daemon_version())),
            Effect::Copy(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                None
            }
            Effect::OpenUrl(url) => {
                cx.open_url(&url);
                None
            }
        }
    }
}

impl AboutPage {
    /// A press that sends `input`.
    fn press(
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _, window, cx| this.send(input.clone(), window, cx))
    }

    fn hero(cx: &App) -> impl IntoElement {
        h_flex()
            .gap_4()
            .items_center()
            .child(img(APP_ICON).size(px(56.)).flex_shrink_0())
            .child(
                v_flex()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t("app-title")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t("about-tagline")),
                    ),
            )
    }

    fn versions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let build = &self.state.build;
        let daemon: SharedString = match &self.state.daemon {
            DaemonVersion::Known(v) => format!("v{v}").into(),
            DaemonVersion::Reading => t("about-daemon-reading"),
            DaemonVersion::NotRead | DaemonVersion::Unknown => t("about-daemon-unknown"),
        };
        let commit: Option<AnyElement> = build.short_commit().map(|sha| {
            let label = match &build.commit_date {
                Some(date) => format!("{sha} ({date})"),
                None => sha.to_string(),
            };
            Link::new("commit")
                .on_click(Self::press(Input::Open(Address::Commit), cx))
                .child(label)
                .into_any_element()
        });
        let system = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        let mut list = DescriptionList::new()
            .small()
            .bordered(true)
            .columns(1)
            .label_width(px(120.))
            .item(t("about-app"), version(), 1);
        if let Some(commit) = commit {
            list = list.item(t("about-commit"), commit, 1);
        }
        Card::new()
            .title(t("version"))
            .action(
                Button::new("copy-details")
                    .small()
                    .icon(Phosphor::Copy)
                    .label(t("about-copy-details"))
                    .on_click(Self::press(Input::CopyDetails, cx)),
            )
            // The values in the labels' size, which the list leaves to its
            // parent.
            .child(
                div()
                    .text_sm()
                    .child(list.item(t("about-daemon"), daemon, 1).item(
                        t("about-system"),
                        system,
                        1,
                    )),
            )
    }

    fn license(cx: &mut Context<Self>) -> impl IntoElement {
        Card::new()
            .title(t("license"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t("about-license-body")),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(Self::address(
                        "license",
                        Address::License,
                        "about-read-the-license",
                        cx,
                    ))
                    .child(Self::address(
                        "notices",
                        Address::Notices,
                        "about-notices",
                        cx,
                    )),
            )
    }

    fn elsewhere(cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .flex_wrap()
            .gap_2()
            .child(
                Self::address("source", Address::Source, "about-source-code", cx)
                    .icon(Phosphor::GithubLogo),
            )
            .child(
                Self::address("issues", Address::Issues, "about-report-a-problem", cx)
                    .icon(Phosphor::Bug),
            )
    }

    /// A button that opens `address` in the browser.
    fn address(id: &'static str, address: Address, key: &str, cx: &mut Context<Self>) -> Button {
        Button::new(id)
            .outline()
            .label(t(key))
            .icon(Phosphor::ArrowSquareOut)
            .on_click(Self::press(Input::Open(address), cx))
    }
}

impl Render for AboutPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::About))
            .child(
                div()
                    .id("about")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(720.))
                            .px(padding)
                            .pt_2()
                            .pb_6()
                            .gap_4()
                            .child(Self::hero(cx))
                            .child(self.versions(cx))
                            .child(Self::license(cx))
                            .child(Self::elsewhere(cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "about-app",
            "about-commit",
            "about-copied",
            "about-copy-details",
            "about-daemon",
            "about-daemon-reading",
            "about-daemon-unknown",
            "about-license-body",
            "about-notices",
            "about-read-the-license",
            "about-report-a-problem",
            "about-source-code",
            "about-system",
            "about-tagline",
            "license",
            "version",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
