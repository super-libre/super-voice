// SPDX-License-Identifier: GPL-3.0-only
//! Text to speech › Voices: voices cloned from a recording. What the loaded
//! model can do with them is said once, at the top. Then a new voice: a
//! script to read, Record (or a recording picked from a file), and once there
//! is a recording, its name and transcript and Save voice. Then the saved
//! voices, each to hear, rename or delete.
//!
//! "Recording" is the word for a cloned voice's audio. The library is the
//! daemon's, kept whatever model is loaded, so nothing here waits on the
//! model except Preview.
//!
//! The logic is the core's Voices page (`pages::voices`). The recorder is the
//! core's too, and opens the microphone only while recording.

use std::time::Duration;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Anchor, AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};
use super_voice_app_core::events::{self, Event};
use super_voice_app_core::languages::friendly_name;
use super_voice_app_core::pages::readiness::Feature;
use super_voice_app_core::pages::settings;
use super_voice_app_core::pages::voices::{self as page, Notice};
use super_voice_app_core::state::scripts::{self, Script};
use super_voice_app_core::state::voices::{SampleOrigin, ScriptChoice, VoicesState};
use super_voice_shared::tts::models::voices::VoiceInfo;

use crate::effects::{self, CorePage, Runner, runner};
use crate::icons::Phosphor;
use crate::pages::{Page, PageView, PaletteCommand, SidebarMark};
use crate::shared::Shared;
use crate::ui::page_header::PageHeader;
use crate::ui::settings_row::SettingsRow;
use crate::ui::toast::{self, Toast};
use crate::ui::width;
use crate::{t, t_with};

/// How often the recording's time and level move.
const TICK: Duration = Duration::from_millis(100);

/// What happened on the page.
#[derive(Debug, Clone)]
pub enum In {
    Voices(page::Input),
    /// The primary language was read: the language a script is suggested
    /// in, since a clone is best read in the language it will speak.
    Language(Option<String>),
    /// An answer with nothing in it.
    Nothing,
}

/// One row of the script picker: a script, or none (the user's own words).
#[derive(Clone)]
struct ScriptRow {
    script: Option<&'static Script>,
    /// The script's id: what picking the row sends.
    id: Option<&'static str>,
}

impl ScriptRow {
    fn new(script: Option<&'static Script>) -> Self {
        Self {
            script,
            id: script.map(|s| s.id),
        }
    }
}

impl SelectItem for ScriptRow {
    type Value = Option<&'static str>;

    fn title(&self) -> SharedString {
        match self.script {
            Some(script) => format!("{} · {}", friendly_name(script.language), script.title).into(),
            None => t("voices-no-script"),
        }
    }

    // reason: the trait's signature, which hands out a reference to the value.
    #[allow(clippy::ref_option_ref)]
    fn value(&self) -> &Option<&'static str> {
        &self.id
    }
}

pub struct VoicesPage {
    state: VoicesState,
    shared: Shared,
    /// The primary language, for the script suggested.
    language: Option<String>,
    /// The synthesis model and whether it is up, as last seen.
    synthesis: Option<(Option<String>, bool)>,
    script: Entity<SelectState<SearchableVec<ScriptRow>>>,
    /// The script rows' ids, in the picker's order.
    script_ids: Vec<Option<&'static str>>,
    label: Entity<InputState>,
    transcript: Entity<InputState>,
    rename: Entity<InputState>,
    /// Moves the recording's time and level while it runs.
    ticker: Option<Task<()>>,
    /// The shot draws a recording, which has no microphone open.
    #[cfg(feature = "screenshot")]
    shot_recording: bool,
    _subscriptions: Vec<Subscription>,
}

/// The scripts offered for `language`, after the row for none.
fn script_rows(language: Option<&str>) -> Vec<ScriptRow> {
    std::iter::once(ScriptRow::new(None))
        .chain(
            scripts::offered(language)
                .into_iter()
                .map(|s| ScriptRow::new(Some(s))),
        )
        .collect()
}

impl PageView for VoicesPage {
    fn new(_: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let rows = script_rows(None);
        let script_ids = rows.iter().map(|row| row.id).collect();
        let script = cx.new(|cx| {
            SelectState::new(SearchableVec::new(rows), None, window, cx).searchable(true)
        });
        let label = cx.new(|cx| InputState::new(window, cx).placeholder(t("name-this-voice")));
        let transcript =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("what-the-recording-says")));
        let rename = cx.new(|cx| InputState::new(window, cx).placeholder(t("name-this-voice")));
        let subscriptions = vec![
            cx.subscribe_in(
                &script,
                window,
                |this, _, event: &SelectEvent<SearchableVec<ScriptRow>>, window, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.send(In::Voices(page::Input::ScriptSelected(*id)), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &label,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let value = state.read(cx).value().to_string();
                        this.send(In::Voices(page::Input::LabelChanged(value)), window, cx);
                    }
                    InputEvent::PressEnter { .. } if this.state.can_save() => {
                        this.send(In::Voices(page::Input::Save), window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &transcript,
                window,
                |this, state, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = state.read(cx).value().to_string();
                        this.send(
                            In::Voices(page::Input::TranscriptChanged(value)),
                            window,
                            cx,
                        );
                    }
                },
            ),
            cx.subscribe_in(
                &rename,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let value = state.read(cx).value().to_string();
                        this.send(In::Voices(page::Input::RenameChanged(value)), window, cx);
                    }
                    InputEvent::PressEnter { .. } => {
                        this.send(In::Voices(page::Input::CommitRename), window, cx);
                    }
                    _ => {}
                },
            ),
            // Another model came up, or went: what it can do with the voices
            // is read with them.
            cx.observe_in(&shared.readiness, window, |this, readiness, window, cx| {
                let synthesis = readiness
                    .read(cx)
                    .stage(Feature::TextToSpeech)
                    .map(|stage| (stage.model.clone(), stage.loaded));
                if this.state.loaded && synthesis != this.synthesis {
                    this.synthesis = synthesis;
                    this.send(In::Voices(page::Input::Refresh), window, cx);
                }
            }),
        ];
        Self {
            state: VoicesState::default(),
            synthesis: None,
            shared: shared.clone(),
            language: None,
            script,
            script_ids,
            label,
            transcript,
            rename,
            ticker: None,
            #[cfg(feature = "screenshot")]
            shot_recording: false,
            _subscriptions: subscriptions,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(In::Voices(page::Input::Refresh), window, cx);
        Self::read_language(window, cx);
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        for input in page::heard(event) {
            self.send(In::Voices(input), window, cx);
        }
        match events::setting_changed(event) {
            Some("language") => Self::read_language(window, cx),
            // Another model may clone, or not: what it can do is read with
            // the library.
            Some("voice") => self.send(In::Voices(page::Input::Refresh), window, cx),
            _ => {}
        }
    }

    fn commands(&self, _: &App) -> Vec<PaletteCommand> {
        let feature = t("text-to-speech");
        if self.state.is_recording() {
            return vec![
                PaletteCommand::new("stop-recording", t("voices-stop-recording"), Phosphor::Stop)
                    .detail(feature),
            ];
        }
        vec![
            PaletteCommand::new("record-voice", t("record-a-voice"), Phosphor::Microphone)
                .detail(feature.clone()),
            PaletteCommand::new(
                "import-voice",
                t("import-a-wav-file"),
                Phosphor::UploadSimple,
            )
            .detail(feature),
        ]
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = match id {
            "record-voice" => page::Input::StartRecording,
            "import-voice" => page::Input::ImportFile,
            "stop-recording" => page::Input::StopRecording {
                language: self.language.clone(),
            },
            _ => return,
        };
        self.send(In::Voices(input), window, cx);
    }

    fn sidebar_mark(&self, _: &App) -> Option<SidebarMark> {
        self.state
            .loaded
            .then_some(SidebarMark::Count(self.state.voices.len()))
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        pretend::fill(self, scenes, window, cx);
    }
}

impl CorePage for VoicesPage {
    type Input = In;
    type Effect = page::Effect;

    fn send(&mut self, input: In, window: &mut Window, cx: &mut Context<Self>) {
        match input {
            In::Voices(input) => {
                let asked = self.state.confirming_delete.clone();
                let renaming = self.state.renaming.as_ref().map(|(id, _)| id.clone());
                let outcome = page::update(&mut self.state, input);
                effects::run_effects(self, outcome.effects, window, cx);
                toast::notice::<Self>(window, cx, outcome.clear_notice, outcome.notice.map(notice));
                if let Some(id) = self.state.confirming_delete.clone()
                    && asked.as_ref() != Some(&id)
                {
                    self.ask_to_delete(id, window, cx);
                }
                let now_renaming = self.state.renaming.as_ref().map(|(id, _)| id.clone());
                if now_renaming.is_some() && now_renaming != renaming {
                    self.rename.update(cx, |input, cx| input.focus(window, cx));
                }
                self.sync_fields(window, cx);
                self.keep_ticking(window, cx);
            }
            In::Language(language) => {
                self.language = language;
                self.sync_fields(window, cx);
            }
            In::Nothing => {}
        }
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: page::Effect,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<In>> {
        use page::Effect;
        Some(match effect {
            Effect::List => runner(async { In::Voices(page::run_list().await) }),
            Effect::Rename { id, label } => {
                runner(async move { In::Voices(page::run_rename(id, label).await) })
            }
            Effect::Delete(id) => runner(async move { In::Voices(page::run_delete(id).await) }),
            Effect::Preview { model, voice_id } => {
                runner(async move { In::Voices(page::run_preview(model, voice_id).await) })
            }
            Effect::Save {
                label,
                transcript,
                wav,
            } => runner(async move { In::Voices(page::run_save(label, transcript, wav).await) }),
            Effect::PickFile => {
                // The picker is the platform's, opened here; the file is read
                // once it answers. It can't filter by type: the daemon is the
                // one that refuses what isn't a WAV.
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(t("choose-a-voice-recording")),
                });
                runner(async move {
                    let path = match picked.await {
                        Ok(Ok(Some(paths))) => paths.into_iter().next(),
                        Ok(Ok(None)) | Err(_) => None,
                        Ok(Err(e)) => return In::Voices(page::Input::ImportFailed(e.to_string())),
                    };
                    let Some(path) = path else {
                        return In::Voices(page::Input::FileImported(None));
                    };
                    let name = path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    In::Voices(match tokio::fs::read(&path).await {
                        Ok(bytes) => page::Input::FileImported(Some((name, bytes))),
                        Err(e) => page::Input::ImportFailed(format!("{}: {e}", path.display())),
                    })
                })
            }
            // The Models page hears the daemon's `voices` setting change and
            // reads its voices again.
            Effect::LibraryChanged => return None,
        })
    }
}

/// What the page tells the user. The daemon's and the device's own words are
/// shown as they come.
fn notice(notice: Notice) -> Toast {
    match notice {
        Notice::CouldNotLoad(e)
        | Notice::CouldNotImport(e)
        | Notice::CouldNotRename(e)
        | Notice::CouldNotDelete(e)
        | Notice::CouldNotPreview(e)
        | Notice::CouldNotRecord(e)
        | Notice::CouldNotSave(e) => Toast::error(e),
        Notice::NeedsTranscript => Toast::warning(t("this-voice-needs-a-transcript")),
    }
}

impl VoicesPage {
    fn read_language(window: &mut Window, cx: &mut Context<Self>) {
        let work = runner(async {
            match settings::run_load_primary_language().await {
                settings::Input::PrimaryLanguageLoaded(tag) => In::Language(tag),
                _ => In::Nothing,
            }
        });
        effects::run(work, window, cx).detach();
    }

    /// Put the core's fields in the page's inputs and picker, where they
    /// differ: an imported file names itself, a script read fills the
    /// transcript, a save empties both.
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rename = self
            .state
            .renaming
            .as_ref()
            .map_or_else(String::new, |(_, label)| label.clone());
        for (input, value) in [
            (&self.label, self.state.label_input.clone()),
            (&self.transcript, self.state.transcript_input.clone()),
            (&self.rename, rename),
        ] {
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
        // The picker lists the scripts in the language's order, and selects
        // the one the page reads.
        let rows = script_rows(self.language.as_deref());
        let ids: Vec<Option<&'static str>> = rows.iter().map(|row| row.id).collect();
        if ids != self.script_ids {
            self.script_ids = ids;
            self.script.update(cx, |select, cx| {
                select.set_items(SearchableVec::new(rows), window, cx);
            });
        }
        let shown = match self.state.script {
            ScriptChoice::Free => None,
            _ => self.state.script(self.language.as_deref()).map(|s| s.id),
        };
        if let Some(row) = self.script_ids.iter().position(|id| *id == shown)
            && self.script.read(cx).selected_index(cx) != Some(IndexPath::new(row))
        {
            self.script.update(cx, |select, cx| {
                select.set_selected_index(Some(IndexPath::new(row)), window, cx);
            });
        }
    }

    /// Run the ticker while the microphone is open, and only then.
    fn keep_ticking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.is_recording() {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                let going = this.update_in(cx, |page, window, cx| {
                    let language = page.language.clone();
                    page.send(
                        In::Voices(page::Input::RecordingTick { language }),
                        window,
                        cx,
                    );
                    page.ticker.is_some()
                });
                if !matches!(going, Ok(true)) {
                    return;
                }
            }
        }));
    }

    /// The model text to speech speaks in: what a preview speaks through.
    fn synthesis_model(&self, cx: &App) -> Option<String> {
        self.shared
            .readiness
            .read(cx)
            .stage(Feature::TextToSpeech)
            .and_then(|stage| stage.model.clone())
    }

    /// Ask before deleting the voice `id`, naming it.
    fn ask_to_delete(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(voice) = self.state.voices.iter().find(|v| v.id == id) else {
            return;
        };
        let title = t_with("delete-voice-named", &[("name", &voice.label)]);
        let this = cx.weak_entity();
        let cancel = this.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let this = this.clone();
            let cancel = cancel.clone();
            let id = id.clone();
            alert
                .title(title.clone())
                .description(t("delete-voice-description"))
                .confirm()
                .ok_text(t("delete"))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t("cancel"))
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    _ = this.update(cx, |page, cx| {
                        page.send(In::Voices(page::Input::Delete(id)), window, cx);
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    _ = cancel.update(cx, |page, cx| {
                        page.send(In::Voices(page::Input::DeleteCancelled), window, cx);
                    });
                    true
                })
        });
    }

    /// What the loaded model can do with these voices, said once.
    fn capability(&self) -> Alert {
        match self.state.model.as_ref() {
            None => {
                Alert::info("capability", t("voices-you-add-are-kept")).title(t("no-model-loaded"))
            }
            Some(model) if !model.clones => {
                Alert::warning("capability", t("voices-you-add-are-kept"))
                    .title(t_with("model-does-not-clone", &[("model", &model.name)]))
            }
            Some(model) => {
                let budget = model.clone_ref_seconds.map_or_else(
                    || t("any-length-of-reference-audio"),
                    |s| {
                        t_with(
                            "up-to-seconds-of-reference-audio",
                            &[("seconds", &format!("{s:.0}"))],
                        )
                    },
                );
                let mut detail =
                    t_with("it-uses-budget-from-each-recording", &[("budget", &budget)])
                        .to_string();
                if model.needs_transcript {
                    detail = format!("{detail} {}", t("it-also-needs-the-transcript"));
                }
                Alert::info("capability", detail)
                    .title(t_with("model-clones-voices", &[("model", &model.name)]))
            }
        }
    }

    /// A script, as a block to read from.
    fn script_block(script: &Script, large: bool, cx: &App) -> impl IntoElement {
        div()
            .w_full()
            .px_4()
            .py_3()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .when(large, |block| block.text_lg().line_height(px(30.)))
            .when(!large, |block| block.text_base().line_height(px(26.)))
            .child(script.text)
    }

    /// How long a script takes to read, and a warning when the loaded model
    /// hears only part of it.
    fn script_meta(&self, script: &Script) -> SharedString {
        let pace = t_with(
            "about-seconds-at-an-unhurried-pace",
            &[("seconds", &format!("{:.0}", script.estimated_seconds()))],
        );
        match self.state.clone_ref_seconds() {
            Some(budget) if !script.fits(Some(budget)) => t_with(
                "voices-script-too-long",
                &[("pace", &pace), ("budget", &format!("{budget:.0}"))],
            ),
            _ => pace,
        }
    }

    /// The line under the picker: what the script is for, and whether it is
    /// in the language the voice will speak.
    fn script_about(&self, script: Option<&Script>) -> SharedString {
        let Some(script) = script else {
            return t("nothing-to-read");
        };
        match self
            .language
            .as_deref()
            .filter(|tag| !scripts::same_language(script, tag))
        {
            Some(tag) => t_with(
                "voices-script-language",
                &[
                    ("blurb", script.blurb),
                    ("script_language", &friendly_name(script.language)),
                    ("speak_language", &friendly_name(tag)),
                ],
            ),
            None => script.blurb.into(),
        }
    }

    /// The new voice: the script and Record; the recording; or the recording
    /// to name and save.
    fn new_voice(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.state.is_recording() || self.shot_recording() {
            return self.recording(cx).into_any_element();
        }
        if self.state.pending.is_some() {
            return self.pending(cx).into_any_element();
        }
        let script = self.state.script(self.language.as_deref());
        let muted = cx.theme().muted_foreground;
        GroupBox::new()
            .outline()
            .title(t("new-voice"))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        div().w(px(320.)).max_w_full().child(
                            Select::new(&self.script)
                                .title_prefix(format!("{}: ", t("script")))
                                .accessibility_label(t("script")),
                        ),
                    )
                    .when_some(script, |row, script| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(self.script_meta(script)),
                        )
                    }),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.script_about(script)),
            )
            .when_some(script, |card, script| {
                card.child(Self::script_block(script, false, cx))
            })
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .flex_wrap()
                    .child(
                        Button::new("record-voice")
                            .primary()
                            .icon(Phosphor::Microphone)
                            .label(t("record"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(In::Voices(page::Input::StartRecording), window, cx);
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.))
                            .text_sm()
                            .text_color(muted)
                            .child(if script.is_some() {
                                t("voices-read-the-script")
                            } else {
                                t("read-a-couple-of-sentences")
                            }),
                    ),
            )
            .into_any_element()
    }

    /// Whether the shot draws a recording (`voice-recording`).
    #[allow(clippy::unused_self)]
    fn shot_recording(&self) -> bool {
        #[cfg(feature = "screenshot")]
        return self.shot_recording;
        #[cfg(not(feature = "screenshot"))]
        false
    }

    /// While the microphone is open: the script, large, to read from; the
    /// time, the input level, and Stop.
    fn recording(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let max = self.state.recorder.as_ref().map_or(
            page::MAX_RECORDING_SECONDS,
            super_voice_app_core::audio::recorder::Recorder::max_seconds,
        );
        let elapsed = t_with(
            "seconds-of-seconds",
            &[
                ("elapsed", &format!("{:.0}", self.state.recording_seconds)),
                ("max", &format!("{max:.0}")),
            ],
        );
        // On the daemon's 60 dB scale, as the meter beside a dictation.
        let level = self.state.recording_level;
        GroupBox::new()
            .outline()
            .title(t("recording"))
            .when_some(
                self.state.script(self.language.as_deref()),
                |card, script| card.child(Self::script_block(script, true, cx)),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(elapsed),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Progress::new("input-level")
                                .small()
                                .value(level * 100.)
                                .accessibility_label(t("input-level")),
                        ),
                    )
                    .child(
                        Button::new("stop-recording")
                            .danger()
                            .icon(Phosphor::Stop)
                            .label(t("stop"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let language = this.language.clone();
                                this.send(
                                    In::Voices(page::Input::StopRecording { language }),
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
    }

    /// A recording, waiting to be named and saved.
    fn pending(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(pending) = self.state.pending.as_ref() else {
            return GroupBox::new().outline().title(t("new-voice"));
        };
        let muted = cx.theme().muted_foreground;
        let origin = match &pending.origin {
            SampleOrigin::Recorded => t("recorded-just-now"),
            SampleOrigin::Imported(name) => t_with("imported-from", &[("name", name)]),
        };
        let summary: SharedString = if pending.seconds > 0. {
            format!("{origin} · {:.1} s", pending.seconds).into()
        } else {
            origin
        };
        // A built-in script is shown, not offered for editing: its words are
        // the ones read, and an edit would describe audio that doesn't exist.
        let transcript =
            match pending.script {
                Some(script) => v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(t("from-the-built-in-script")),
                    )
                    .child(Self::script_block(script, false, cx))
                    .into_any_element(),
                None => v_flex()
                    .gap_2()
                    .child(div().text_xs().text_color(muted).child(
                        if self.state.needs_transcript() {
                            t("required-by-the-loaded-model")
                        } else {
                            t("voices-transcript-optional")
                        },
                    ))
                    .child(Input::new(&self.transcript).aria_label(t("transcript")))
                    .into_any_element(),
            };
        GroupBox::new()
            .outline()
            .title(t("new-voice"))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_kit::component::Icon::new(Phosphor::Waveform)
                            .small()
                            .text_color(muted),
                    )
                    .child(div().text_sm().child(summary)),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("name")),
                    )
                    .child(Input::new(&self.label).aria_label(t("name"))),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("transcript")),
                    )
                    .child(transcript),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-voice")
                            .primary()
                            .label(t("save-voice"))
                            .loading(self.state.saving)
                            .disabled(!self.state.can_save())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(In::Voices(page::Input::Save), window, cx);
                            })),
                    )
                    .child(
                        Button::new("discard-voice")
                            .outline()
                            .label(t("discard"))
                            .disabled(self.state.saving)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(In::Voices(page::Input::DiscardPending), window, cx);
                            })),
                    ),
            )
    }

    /// The saved voices.
    fn library(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let card = GroupBox::new().outline().title(t("saved-voices"));
        if !self.state.loaded {
            return card.child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(t("reading-the-library")),
            );
        }
        if self.state.voices.is_empty() {
            return card.child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("no-voices-yet")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(t("record-a-voice-or-import-a-recording")),
                    ),
            );
        }
        // The row's width: the card's, inside its padding.
        let row =
            (width::content(window) - width::page_padding(window) * 2.).min(px(720.)) - px(34.);
        card.child(
            v_flex().children(
                self.state
                    .voices
                    .iter()
                    .enumerate()
                    .map(|(ix, voice)| self.voice_row(ix, voice, row, cx)),
            ),
        )
    }

    /// One saved voice: its name and facts; Preview, Rename and a menu with
    /// Delete…. While renamed, a field and Save.
    fn voice_row(
        &self,
        ix: usize,
        voice: &VoiceInfo,
        row: gpui_kit::Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = cx.theme().border;
        let separated = |el: AnyElement| {
            div()
                .when(ix > 0, |d| d.border_t_1().border_color(border))
                .child(el)
                .into_any_element()
        };
        if self
            .state
            .renaming
            .as_ref()
            .is_some_and(|(id, _)| *id == voice.id)
        {
            return separated(
                h_flex()
                    .py_3()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.rename).aria_label(t("name"))),
                    )
                    .child(
                        Button::new("save-rename")
                            .primary()
                            .label(t("save"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(In::Voices(page::Input::CommitRename), window, cx);
                            })),
                    )
                    .child(
                        Button::new("cancel-rename")
                            .label(t("cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.send(In::Voices(page::Input::CancelRename), window, cx);
                            })),
                    )
                    .into_any_element(),
            );
        }
        let deleting = self.state.deleting.contains(&voice.id);
        let busy = self.state.is_busy(&voice.voice_id);
        let description: SharedString = if deleting {
            t("deleting-progress")
        } else if busy {
            t("getting-the-model-ready")
        } else {
            let transcript = if voice.transcript.is_some() {
                t("with-transcript")
            } else {
                t("no-transcript")
            };
            t_with(
                "voice-meta",
                &[
                    ("seconds", &format!("{:.1}", voice.duration_seconds)),
                    ("transcript", &transcript),
                    ("id", &voice.voice_id),
                ],
            )
        };
        let actions = if deleting {
            h_flex()
        } else {
            self.voice_actions(voice, busy, cx)
        };
        separated(
            SettingsRow::new(voice.label.clone())
                .description(description)
                .width(row)
                .control(actions)
                .into_any_element(),
        )
    }

    /// A saved voice's actions: Preview, Rename, and a menu with Delete….
    fn voice_actions(
        &self,
        voice: &VoiceInfo,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let mut actions = h_flex().gap_1().items_center();
        // Offered only when the loaded model can speak in it, and live
        // while nothing is being got ready: a second speak request
        // cancels the first.
        if self.state.model_clones() {
            let voice_id = voice.voice_id.clone();
            actions = actions.child(
                Button::new(SharedString::from(format!("preview-{}", voice.id)))
                    .small()
                    .outline()
                    .icon(Phosphor::Play)
                    .label(if busy {
                        t("preparing-progress")
                    } else {
                        t("preview")
                    })
                    .loading(busy)
                    .disabled(self.state.any_busy())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let model = this.synthesis_model(cx);
                        this.send(
                            In::Voices(page::Input::Preview {
                                voice_id: voice_id.clone(),
                                model,
                            }),
                            window,
                            cx,
                        );
                    })),
            );
        }
        let (id, label) = (voice.id.clone(), voice.label.clone());
        actions = actions.child(
            Button::new(SharedString::from(format!("rename-{}", voice.id)))
                .small()
                .outline()
                .label(t("rename"))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.send(
                        In::Voices(page::Input::BeginRename {
                            id: id.clone(),
                            label: label.clone(),
                        }),
                        window,
                        cx,
                    );
                })),
        );
        let weak = cx.weak_entity();
        let id = voice.id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("voice-more-{}", voice.id)))
                .small()
                .ghost()
                .icon(Phosphor::DotsThreeVertical)
                .accessibility_label(t("voice-actions"))
                .tooltip(t("voice-actions"))
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    let weak = weak.clone();
                    let id = id.clone();
                    menu.item(
                        PopupMenuItem::new(t("delete-voice-ellipsis"))
                            .icon(Phosphor::Trash)
                            .on_click(move |_, window, cx| {
                                let id = id.clone();
                                _ = weak.update(cx, |this, cx| {
                                    this.send(
                                        In::Voices(page::Input::DeleteRequested(id)),
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
                }),
        );
        actions
    }
}

impl Render for VoicesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        let import = Button::new("import-voice")
            .icon(Phosphor::UploadSimple)
            .label(t("import-a-wav-file"))
            .disabled(self.state.is_recording() || self.state.saving)
            .on_click(cx.listener(|this, _, window, cx| {
                this.send(In::Voices(page::Input::ImportFile), window, cx);
            }));
        v_flex()
            .size_full()
            .child(
                PageHeader::new(Page::Voices)
                    .description(t("voices-description"))
                    .action(import),
            )
            .child(
                div()
                    .id("voices")
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
                            .child(self.capability())
                            .child(self.new_voice(cx))
                            .child(self.library(window, cx)),
                    ),
            )
    }
}

/// Made-up voices for `just app-shot`: two cloned voices, Grace and Narrator.
/// The default has Kokoro loaded, which doesn't clone. `voice-adding`: Qwen
/// TTS loaded, a recording read from a script waiting to be saved, and Grace
/// being got ready to speak.
#[cfg(feature = "screenshot")]
mod pretend {
    use super::*;
    use super_voice_app_core::state::voices::PendingSample;
    use super_voice_shared::tts::models::voices::VoiceModelSupport;

    fn voice(id: &str, label: &str, seconds: f32, transcript: Option<&str>) -> VoiceInfo {
        VoiceInfo {
            id: id.into(),
            voice_id: format!("voice:{id}"),
            label: label.into(),
            transcript: transcript.map(Into::into),
            created_at: "2026-11-20T10:00:00Z".into(),
            duration_seconds: seconds,
            sample_rate: 24_000,
            channels: 1,
        }
    }

    pub fn fill(
        page: &mut VoicesPage,
        scenes: &[&str],
        window: &mut Window,
        cx: &mut Context<VoicesPage>,
    ) {
        page.language = Some("en".into());
        page.state.loaded = true;
        page.state.voices = vec![
            voice(
                "2f8a2d0e",
                "Grace",
                18.0,
                Some("The North Wind and the Sun…"),
            ),
            voice("9c41b7aa", "Narrator", 12.4, None),
        ];
        // Someone reading the script at an ordinary microphone gain, six
        // seconds in.
        if scenes.contains(&"voice-recording") {
            page.shot_recording = true;
            page.state.script =
                ScriptChoice::Chosen(scripts::find("en-north-wind").expect("a script"));
            page.state.recording_seconds = 6.4;
            page.state.recording_level = super_voice_app_core::events::level_percent(0.03);
        }
        let adding = scenes.contains(&"voice-adding");
        page.state.model = Some(if adding {
            VoiceModelSupport {
                name: "Qwen TTS".into(),
                source: "github.com/super-libre/super-tts-qwen".into(),
                clones: true,
                clone_ref_seconds: Some(15.),
                needs_transcript: false,
            }
        } else {
            VoiceModelSupport {
                name: "Kokoro".into(),
                source: "github.com/super-libre/super-tts-kokoro".into(),
                clones: false,
                clone_ref_seconds: None,
                needs_transcript: false,
            }
        });
        if adding {
            let script = scripts::find("en-north-wind");
            page.state.pending = Some(PendingSample {
                wav: Vec::new(),
                seconds: 14.6,
                origin: SampleOrigin::Recorded,
                script,
            });
            page.state.label_input = "Ada".into();
            page.state.transcript_input = script.map_or_else(String::new, |s| s.text.to_string());
            page.state.preparing = Some("voice:2f8a2d0e".into());
        }
        page.sync_fields(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    /// Every word the page shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_word_is_in_the_catalog() {
        for key in [
            "about-seconds-at-an-unhurried-pace",
            "any-length-of-reference-audio",
            "cancel",
            "choose-a-voice-recording",
            "delete",
            "delete-voice-description",
            "delete-voice-ellipsis",
            "delete-voice-named",
            "deleting-progress",
            "discard",
            "from-the-built-in-script",
            "getting-the-model-ready",
            "import-a-wav-file",
            "imported-from",
            "input-level",
            "it-also-needs-the-transcript",
            "it-uses-budget-from-each-recording",
            "model-clones-voices",
            "model-does-not-clone",
            "name",
            "name-this-voice",
            "new-voice",
            "no-model-loaded",
            "no-transcript",
            "no-voices-yet",
            "nothing-to-read",
            "preparing-progress",
            "preview",
            "read-a-couple-of-sentences",
            "reading-the-library",
            "record",
            "record-a-voice",
            "record-a-voice-or-import-a-recording",
            "recorded-just-now",
            "recording",
            "rename",
            "required-by-the-loaded-model",
            "save",
            "save-voice",
            "saved-voices",
            "script",
            "seconds-of-seconds",
            "stop",
            "voices-stop-recording",
            "text-to-speech",
            "this-voice-needs-a-transcript",
            "transcript",
            "up-to-seconds-of-reference-audio",
            "voice-actions",
            "voice-meta",
            "voices-description",
            "voices-no-script",
            "voices-read-the-script",
            "voices-script-language",
            "voices-script-too-long",
            "voices-transcript-optional",
            "voices-you-add-are-kept",
            "what-the-recording-says",
            "with-transcript",
        ] {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
