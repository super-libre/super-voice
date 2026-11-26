// SPDX-License-Identifier: GPL-3.0-only
//! The Updates page: this version and the latest, when the daemon last
//! checked and Check now, whether it checks on its own and offers betas, and
//! the update itself: offered, its run with its progress, then installed
//! (with Restart) or failed (with the command that installs it by hand).
//!
//! The logic is the core's (`pages::updates`, `state::update`). The app's
//! own part is the run's stream, which it reads on the core's runtime, and
//! starting itself again after an update. While an update is ready, the
//! sidebar marks the page with a dot, and a toast says so once per version.

use futures_util::StreamExt as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, px,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::updates::{self as page, Effect, Input};
use super_voice_app_core::state::update::{RunPhase, UpdateNotice, UpdateRun, UpdateState};
use super_voice_app_core::updater;

use crate::effects::{self, CorePage, Runner, runner};
use crate::icons::Phosphor;
use crate::pages::{Page, PageView, PaletteCommand, SidebarMark};
use crate::runtime;
use crate::shared::{Navigator, Shared};
use crate::ui::card::Card;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::toast::{self, Toast};
use crate::ui::{code_line, width};
use crate::{t, t_with, version};

pub struct UpdatesPage {
    state: UpdateState,
    nav: Navigator,
    /// The run's events, handed over from the core's runtime.
    run: Option<Task<()>>,
    /// The version a toast last said was ready, so it says so once.
    toasted: Option<String>,
    /// Starting the app again failed.
    relaunch_failed: bool,
}

/// A toast of the page's own, apart from its notices.
struct UpdateReady;

impl PageView for UpdatesPage {
    fn new(_: Page, shared: &Shared, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            state: UpdateState::default(),
            nav: shared.nav.clone(),
            run: None,
            toasted: None,
            relaunch_failed: false,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(Input::Reached, window, cx);
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = page::heard(event) {
            self.send(input, window, cx);
        }
    }

    fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        toast::clear::<UpdateReady>(window, cx);
        if self.state.status.is_some() && self.state.run.is_none() {
            self.send(Input::Reached, window, cx);
        }
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let mut commands = Vec::new();
        if self.state.can_check_now() {
            commands.push(PaletteCommand::new(
                "check",
                t("check-for-updates"),
                Phosphor::ArrowsClockwise,
            ));
        }
        if self.state.can_start_update()
            && let Some(tag) = self.latest()
        {
            commands.push(PaletteCommand::new(
                "update",
                t_with("update-to-tag", &[("tag", &tag)]),
                Phosphor::DownloadSimple,
            ));
        }
        commands
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "check" => self.send(Input::CheckNow, window, cx),
            "update" => self.send(Input::StartUpdate, window, cx),
            _ => {}
        }
    }

    fn sidebar_mark(&self, _: &App) -> Option<SidebarMark> {
        self.state
            .header_badge_visible()
            .then_some(SidebarMark::Update)
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], _: &mut Window, _: &mut Context<Self>) {
        use super_voice_app_core::updater::{InstallerEvent, UpdateRunEvent};
        use super_voice_shared::models::self_update::{InstallerAsset, SelfUpdateStatus};

        self.state.auto_check_enabled = Some(true);
        self.state.apply_status_loaded(Some(SelfUpdateStatus {
            current_version: env!("CARGO_PKG_VERSION").into(),
            latest_version: Some("v0.3.0-beta.8".into()),
            update_available: !scenes.contains(&"up-to-date"),
            checked_at: Some("2026-11-25T09:14:00Z".into()),
            last_check_error: None,
            beta_optin_effective: true,
            installer_asset: Some(InstallerAsset {
                name: "super-voice-install-x86_64-unknown-linux-gnu".into(),
                url: "https://example.invalid/installer".into(),
                size: 9_437_184,
                sha256: "0".repeat(64),
            }),
        }));
        if scenes.contains(&"updating") {
            self.state.begin_run();
            let _ = self
                .state
                .apply_run_event(UpdateRunEvent::Installer(InstallerEvent::Phase {
                    phase: "download".into(),
                    message: String::new(),
                }));
            let _ =
                self.state
                    .apply_run_event(UpdateRunEvent::Installer(InstallerEvent::Progress {
                        phase: "download".into(),
                        bytes_done: 58_720_256,
                        bytes_total: 96_468_992,
                    }));
        } else if scenes.contains(&"update-failed") {
            self.state.begin_run();
            let _ = self
                .state
                .apply_run_event(UpdateRunEvent::Installer(InstallerEvent::Error {
                    code: "checksum_mismatch".into(),
                    message: "The download didn't match its checksum.".into(),
                }));
        } else if scenes.contains(&"update-done") {
            self.state.begin_run();
            let _ =
                self.state
                    .apply_run_event(UpdateRunEvent::Installer(InstallerEvent::Complete {
                        installed_version: "v0.3.0-beta.8".into(),
                        components: vec!["daemon".into(), "app".into()],
                    }));
        }
    }
}

impl CorePage for UpdatesPage {
    type Input = Input;
    type Effect = Effect;

    fn send(&mut self, input: Input, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.state.notice.clone();
        let effects = page::update(&mut self.state, input);
        effects::run_effects(self, effects, window, cx);
        if self.state.notice != before {
            toast::notice::<Self>(
                window,
                cx,
                true,
                self.state.notice.as_ref().map(|notice| {
                    let (key, error) = match notice {
                        UpdateNotice::StatusUnread(e) => ("update-status-could-not-read", e),
                        UpdateNotice::SettingUnsaved(e) => ("update-setting-could-not-save", e),
                    };
                    Toast::error(t_with(key, &[("error", error)]))
                }),
            );
        }
        self.say_ready(window, cx);
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<Input>> {
        Some(match effect {
            Effect::ReadStatus => runner(page::run_status()),
            Effect::ReadAutoCheck => runner(page::run_load_auto_check()),
            Effect::Check => runner(page::run_check()),
            Effect::SaveAutoCheck(enabled) => runner(page::run_save_auto_check(enabled)),
            Effect::SaveBetaOptin(enabled) => runner(page::run_save_beta_optin(enabled)),
            Effect::StartRun { asset, tag } => {
                self.start_run(asset, tag, window, cx);
                return None;
            }
        })
    }
}

impl UpdatesPage {
    /// Download and start the installer, and take in each event of its run.
    /// Aborting the stream (Cancel) drops the installer, which kills it.
    fn start_run(
        &mut self,
        asset: super_voice_shared::models::self_update::InstallerAsset,
        tag: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (stream, abort) =
            futures_util::stream::abortable(updater::run_update_stream(asset, tag));
        self.state.run_abort = Some(abort);
        self.relaunch_failed = false;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        drop(runtime::spawn(async move {
            let mut stream = std::pin::pin!(stream);
            while let Some(event) = stream.next().await {
                if tx.send(event).is_err() {
                    return;
                }
            }
        }));
        self.run = Some(cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = rx.recv().await {
                if this
                    .update_in(cx, |page, window, cx| {
                        page.send(Input::RunEvent(event), window, cx);
                    })
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    /// Start the app again, from where it is installed, once the update put
    /// a new one there.
    fn restart(&mut self, cx: &mut Context<Self>) {
        // An executable replaced while running reads as "… (deleted)".
        let path = std::env::current_exe().ok().map(|path| {
            let shown = path.to_string_lossy();
            shown
                .strip_suffix(" (deleted)")
                .map_or(path.clone(), std::path::PathBuf::from)
        });
        if let Some(path) = path.filter(|path| path.exists()) {
            cx.set_restart_path(path);
            cx.restart();
        } else {
            log::warn!("couldn't find the app to start it again");
            self.relaunch_failed = true;
            cx.notify();
        }
    }

    /// Say once per version that an update is ready, with a way to this page.
    fn say_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.header_badge_visible() {
            return;
        }
        let Some(tag) = self.latest() else {
            return;
        };
        if self.toasted.as_ref() == Some(&tag) {
            return;
        }
        self.toasted = Some(tag.clone());
        let nav = self.nav.clone();
        toast::show::<UpdateReady>(
            window,
            cx,
            Toast::info(t_with("update-ready-to-install", &[("tag", &tag)]))
                .title(t("update-available"))
                .action(t("open-updates"), move |window, cx| {
                    nav.go(Page::Updates, window, cx);
                }),
        );
    }

    /// The version the daemon offers, as it tags it.
    fn latest(&self) -> Option<String> {
        self.state
            .status
            .as_ref()
            .and_then(|status| status.latest_version.clone())
    }

    /// A press that sends `input`.
    fn press(
        input: Input,
        cx: &mut Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, _, window, cx| this.send(input.clone(), window, cx))
    }

    fn version_card(&self, cx: &mut Context<Self>) -> Card {
        let status = self.state.status.as_ref();
        let latest: SharedString = status
            .and_then(|s| s.latest_version.clone())
            .map_or_else(|| t("about-daemon-unknown"), SharedString::from);
        let checked: SharedString = status
            .and_then(|s| s.checked_at.as_deref())
            .map_or_else(|| t("never"), |at| page::checked_at_label(at).into());
        let error = status.and_then(|s| s.last_check_error.clone());
        let mut checked_row = SettingsRow::new(t("last-checked")).inline();
        if let Some(error) = error {
            checked_row = checked_row.description(error);
        }
        let value = |text: SharedString| div().text_sm().child(text);
        Card::new()
            .title(t("version"))
            .action(
                Button::new("check-now")
                    .small()
                    .icon(Phosphor::ArrowsClockwise)
                    .label(if self.state.checking {
                        t("checking-progress")
                    } else {
                        t("check-now")
                    })
                    .loading(self.state.checking)
                    .disabled(!self.state.can_check_now())
                    .on_click(Self::press(Input::CheckNow, cx)),
            )
            .child(
                v_flex()
                    .child(
                        SettingsRow::new(t("current-version"))
                            .inline()
                            .control(value(version())),
                    )
                    .child(Separator::horizontal())
                    .child(
                        SettingsRow::new(t("latest-version"))
                            .inline()
                            .control(value(latest)),
                    )
                    .child(Separator::horizontal())
                    .child(checked_row.control(value(checked))),
            )
    }

    fn automatic_card(&self, cx: &mut Context<Self>) -> Card {
        let running = self.state.run.is_some();
        Card::new().title(t("automatic-updates")).child(
            v_flex()
                .child(
                    SettingsRow::new(t("check-daily"))
                        .description(t("check-daily-description"))
                        .inline()
                        .control(
                            Switch::new("check-daily")
                                .checked(self.state.auto_check_enabled.unwrap_or(true))
                                .disabled(running || self.state.auto_check_enabled.is_none())
                                .accessibility_label(t("check-daily"))
                                .on_change(cx.listener(|this, on: &bool, window, cx| {
                                    this.send(Input::AutoCheckToggled(*on), window, cx);
                                })),
                        ),
                )
                .child(Separator::horizontal())
                .child(
                    SettingsRow::new(t("include-betas"))
                        .description(t("include-betas-description"))
                        .inline()
                        .control(
                            // Locked while its write and the check after it
                            // settle: the switch has already moved.
                            Switch::new("include-betas")
                                .checked(self.state.beta_optin_shown())
                                .disabled(running || self.state.beta_pending)
                                .accessibility_label(t("include-betas"))
                                .on_change(cx.listener(|this, on: &bool, window, cx| {
                                    this.send(Input::BetaOptinToggled(*on), window, cx);
                                })),
                        ),
                ),
        )
    }

    /// The update: offered, running, installed or failed.
    fn update_card(&self, cx: &mut Context<Self>) -> Option<Card> {
        if !self.state.update_section_visible() {
            return None;
        }
        let tag = self
            .latest()
            .unwrap_or_else(|| t("the-latest-version").to_string());
        let card = Card::new().title(t("update"));
        let Some(run) = self.state.run.as_ref() else {
            return Some(card.child(self.offered(&tag, cx)));
        };
        Some(match run.phase {
            RunPhase::Done => card.child(self.installed(run, cx)),
            RunPhase::Failed => card.child(Self::failed(run, &tag, cx)),
            phase => card.child(Self::running(run, phase, cx)),
        })
    }

    /// The update offered: Update to its version, or, with no installer for
    /// this computer, the command that installs it.
    fn offered(&self, tag: &str, cx: &mut Context<Self>) -> AnyElement {
        let can_start = self.state.can_start_update();
        let row = SettingsRow::new(t_with("update-ready-to-install", &[("tag", tag)])).control(
            Button::new("start-update")
                .primary()
                .icon(Phosphor::DownloadSimple)
                .label(t_with("update-to-tag", &[("tag", tag)]))
                .disabled(!can_start)
                .on_click(Self::press(Input::StartUpdate, cx)),
        );
        v_flex()
            .gap_2()
            .child(row)
            .when(!can_start, |body| {
                body.child(caption(t("update-no-installer-caption"), cx))
                    .child(command_line(page::install_command(tag), cx))
            })
            .into_any_element()
    }

    /// The run going on: which step, its progress, and Cancel while nothing
    /// is touched yet.
    fn running(run: &UpdateRun, phase: RunPhase, cx: &mut Context<Self>) -> AnyElement {
        // reason: a progress bar's fraction; losing precision is harmless.
        #[allow(clippy::cast_precision_loss)]
        let percent = (run.bytes_total > 0)
            .then(|| (run.bytes_done as f32 / run.bytes_total as f32 * 100.).clamp(0., 100.))
            .filter(|_| matches!(phase, RunPhase::FetchingInstaller | RunPhase::Download));
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().flex_1().min_w_0().text_sm().child(phase_words(phase)))
                    .when_some(percent, |row, percent| {
                        row.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{percent:.0}%")),
                        )
                    })
                    .when(run.cancellable(), |row| {
                        row.child(
                            Button::new("cancel-update")
                                .small()
                                .danger()
                                .label(t("cancel"))
                                .on_click(Self::press(Input::CancelUpdate, cx)),
                        )
                    }),
            )
            .child(
                Progress::new("update-progress")
                    .small()
                    .value(percent.unwrap_or(0.))
                    .loading(percent.is_none()),
            )
            .into_any_element()
    }

    /// Installed: Restart when the app itself was updated, and Dismiss.
    fn installed(&self, run: &UpdateRun, cx: &mut Context<Self>) -> AnyElement {
        let restart = run.completed_components.iter().any(|c| c == "app");
        let error = run
            .error
            .clone()
            .map(SharedString::from)
            .or_else(|| self.relaunch_failed.then(|| t("relaunch-failed")));
        v_flex()
            .gap_2()
            .child(caption(t("update-installed"), cx))
            .when(restart, |body| {
                body.child(
                    SettingsRow::new(t("restart-super-voice-to-finish-the-update")).control(
                        Button::new("restart")
                            .primary()
                            .icon(Phosphor::ArrowClockwise)
                            .label(t("restart"))
                            .on_click(cx.listener(|this, _, _, cx| this.restart(cx))),
                    ),
                )
                .when_some(error, |body, error| {
                    body.child(Alert::error("relaunch-error", error))
                })
            })
            .child(dismiss(cx))
            .into_any_element()
    }

    /// Failed: why, the command that installs it by hand (not after the
    /// user declined the password prompt, where Update again is the answer),
    /// and Dismiss.
    fn failed(run: &UpdateRun, tag: &str, cx: &mut Context<Self>) -> AnyElement {
        let error: SharedString = run
            .error
            .clone()
            .map_or_else(|| t("update-failed"), SharedString::from);
        v_flex()
            .gap_2()
            .child(Alert::error("update-error", error).title(t("update-failed")))
            .when(!run.was_authorization_declined(), |body| {
                body.child(caption(t("install-command-caption"), cx))
                    .child(command_line(page::install_command(tag), cx))
            })
            .child(dismiss(cx))
            .into_any_element()
    }
}

impl Render for UpdatesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        let unsupported = self.state.unsupported;
        v_flex()
            .size_full()
            .child(PageHeader::new(Page::Updates))
            .child(
                div()
                    .id("updates")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(820.))
                            .px(padding)
                            .pt_2()
                            .pb_6()
                            .gap_4()
                            .children(self.update_card(cx))
                            .child(self.version_card(cx))
                            .map(|page| {
                                if unsupported {
                                    page.child(caption(
                                        t("the-connected-daemon-predates-update-support"),
                                        cx,
                                    ))
                                } else {
                                    page.child(self.automatic_card(cx))
                                }
                            }),
                    ),
            )
    }
}

/// What a step of the run is doing.
fn phase_words(phase: RunPhase) -> SharedString {
    t(match phase {
        RunPhase::FetchingInstaller => "downloading-installer-progress",
        RunPhase::Resolve => "resolving-progress",
        RunPhase::Download => "downloading-update-progress",
        RunPhase::Verify => "verifying-progress",
        RunPhase::Stage => "staging-progress",
        RunPhase::WaitingAuth => "waiting-for-authorization-enter-your-password-in",
        RunPhase::Install => "installing-progress",
        RunPhase::PostInstall | RunPhase::Done | RunPhase::Failed => "finishing-progress",
    })
}

/// Muted words under a card's rows.
fn caption(words: SharedString, cx: &App) -> impl IntoElement + use<> {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(words)
}

/// The command that installs an update by hand, with Copy.
fn command_line(command: String, cx: &App) -> impl IntoElement + use<> {
    let copied = command.clone();
    h_flex()
        .gap_2()
        .items_center()
        .child(code_line(command.into(), cx))
        .child(
            Button::new("copy-install-command")
                .icon(Phosphor::Copy)
                .tooltip(t("copy-command"))
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                }),
        )
}

fn dismiss(cx: &mut Context<UpdatesPage>) -> impl IntoElement + use<> {
    h_flex().child(
        Button::new("dismiss-run")
            .label(t("dismiss"))
            .on_click(UpdatesPage::press(Input::DismissRun, cx)),
    )
}

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "about-daemon-unknown",
            "automatic-updates",
            "cancel",
            "check-daily",
            "check-daily-description",
            "check-for-updates",
            "check-now",
            "checking-progress",
            "copy-command",
            "current-version",
            "dismiss",
            "downloading-installer-progress",
            "downloading-update-progress",
            "finishing-progress",
            "include-betas",
            "include-betas-description",
            "install-command-caption",
            "installing-progress",
            "last-checked",
            "latest-version",
            "never",
            "open-updates",
            "relaunch-failed",
            "resolving-progress",
            "restart",
            "restart-super-voice-to-finish-the-update",
            "staging-progress",
            "the-connected-daemon-predates-update-support",
            "the-latest-version",
            "update",
            "update-available",
            "update-failed",
            "update-installed",
            "update-no-installer-caption",
            "update-ready-to-install",
            "update-setting-could-not-save",
            "update-status-could-not-read",
            "update-to-tag",
            "verifying-progress",
            "version",
            "waiting-for-authorization-enter-your-password-in",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
