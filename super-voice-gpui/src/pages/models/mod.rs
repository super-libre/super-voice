// SPDX-License-Identifier: GPL-3.0-only
//! Each feature's Models page: its pipeline as a vertical row of steps, from
//! where its input comes from to where its output goes, with a card for each
//! step a model fills. Speech to text: the microphone, transcription, then
//! optional post-processing, then the app the words are typed into. Text to
//! speech: the text, synthesis, then the speakers.
//!
//! A step's card picks its backend (or offers the Library when none is
//! installed), its model, where the model runs, its language and (synthesis)
//! its voice, loads and unloads it, follows its download and its load with
//! Cancel, says how much memory it needs and whether that fits, and opens
//! the backend's Configure sheet (its keys, its options, the dictation
//! context it is sent).
//!
//! The logic is the core's: `pages::models` for the cards,
//! `pages::model_language` and `pages::model_voice` for a model's language
//! and voice, `pages::configure` for the sheet, and `pages::library`'s
//! install for a framework's build the user agreed to download. One view
//! serves both pages; each reads and draws only its own feature's steps.

mod card;
mod configure;
mod pickers;
#[cfg(feature = "screenshot")]
mod pretend;
mod words;

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};
use super_voice_app_core::daemon::backends::BackendInfo;
use super_voice_app_core::daemon::client::v1::contexts::ContextCatalog;
use super_voice_app_core::events::{self, Event};
use super_voice_app_core::pages::configure as sheet;
use super_voice_app_core::pages::library::{self, LibraryState};
use super_voice_app_core::pages::model_language::{self as language, Focus, ModelKey};
use super_voice_app_core::pages::model_voice as voice;
use super_voice_app_core::pages::models::{self as page, ModelsState};
use super_voice_app_core::pages::readiness::{Feature, Readiness};
use super_voice_app_core::rules::gpu::accel_name;
use super_voice_app_core::state::Stage;
use super_voice_app_core::state::language::ModelLanguages;
use super_voice_app_core::state::registry::RegistryState;
use super_voice_app_core::state::voice::VoiceState;

use self::pickers::{PickerKey, Pickers};
use crate::effects::{self, CorePage, Runner, runner};
use crate::pages::{Page, PageView, PaletteCommand};
use crate::shared::Shared;
use crate::ui::page_header::PageHeader;
use crate::ui::toast::{self, Toast};
use crate::ui::width::{self, WindowSize};
use crate::{t, t_with};

/// How often the page checks for a download or load the daemon stopped
/// reporting on.
const TICK: Duration = Duration::from_secs(30);

/// The widest the steps grow.
const STEPS_MAX: Pixels = px(860.);

/// One feature's Models page.
pub struct ModelsPage {
    page: Page,
    feature: Feature,
    shared: Shared,
    models: ModelsState,
    languages: ModelLanguages,
    focus: Focus,
    voice: VoiceState,
    sheet: configure::Sheet,
    /// A framework's build the user agreed to download, installing as the
    /// Library installs.
    registry: RegistryState,
    library: LibraryState,
    pickers: Pickers<Self>,
    /// Checks for a stalled download or load, while the daemon answers.
    ticker: Option<Task<()>>,
}

/// What happened, for any of the core's pages this page runs.
pub enum In {
    /// A runner with nothing to say.
    Nothing,
    Models(page::Input),
    Language(language::Input),
    Voice(voice::Input),
    Configure(sheet::Input),
    /// The build's install, as the Library's page takes it.
    Install(library::Input),
    /// The backend list, read again for the open sheet.
    Backends(Result<Vec<BackendInfo>, String>),
    /// The backend list, read to open the sheet over `source`.
    SheetOpened {
        source: String,
        result: Result<Vec<BackendInfo>, String>,
    },
    /// The dictation contexts, for the sheet's picker.
    Contexts(Option<ContextCatalog>),
}

/// What to do, for any of the core's pages this page runs.
pub enum Out {
    Models(page::Effect),
    Language(language::Effect),
    Voice(voice::Effect),
    Configure(sheet::Effect),
}

/// The toasts of each page, apart: the two pages are one view.
struct SpeechToTextToast;
struct TextToSpeechToast;

impl PageView for ModelsPage {
    fn new(page: Page, shared: &Shared, _: &mut Window, _: &mut Context<Self>) -> Self {
        let feature = page.feature().unwrap_or(Feature::SpeechToText);
        Self {
            page,
            feature,
            shared: shared.clone(),
            models: ModelsState::default(),
            languages: ModelLanguages::default(),
            focus: Focus::default(),
            voice: VoiceState::default(),
            sheet: configure::Sheet::default(),
            registry: RegistryState::default(),
            library: LibraryState::default(),
            pickers: Pickers::new(Self::picked),
            ticker: None,
        }
    }

    fn reached(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // What was under way went with the daemon's last session.
        self.send(In::Models(page::Input::Reconnected), window, cx);
        if self.ticker.is_none() {
            self.ticker = Some(cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    let ticked = this.update_in(cx, |page, window, cx| {
                        if !page.models.operations.pending_stages().is_empty() {
                            page.send(In::Models(page::Input::Tick), window, cx);
                        }
                    });
                    if ticked.is_err() {
                        return;
                    }
                }
            }));
        }
    }

    fn heard(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Connected => {
                self.send(In::Models(page::Input::Reload), window, cx);
                let keys: Vec<ModelKey> = self.focus.keys().cloned().collect();
                for key in keys {
                    self.send(In::Language(language::Input::Focus(key)), window, cx);
                }
                if self.feature == Feature::TextToSpeech {
                    self.send(In::Voice(voice::Input::Reload), window, cx);
                }
                if let Some(source) = self.sheet.state.open.clone() {
                    self.read_sheet(source, window, cx);
                }
            }
            Event::Status(status) => {
                for input in page::heard(status) {
                    if input_stage(&input).is_none_or(|stage| self.has(stage)) {
                        self.send(In::Models(input), window, cx);
                    }
                }
                self.setting_changed(events::setting_changed(event), window, cx);
            }
            Event::Install(install) => {
                let input = library::Input::Event(install.clone());
                self.send(In::Install(input), window, cx);
            }
            _ => {}
        }
    }

    fn shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // How much of the GPU is free decides whether a model fits.
        if !self.shared.connection.read(cx).is_down() {
            self.send(In::Models(page::Input::RefreshGpus), window, cx);
        }
    }

    fn commands(&self, cx: &App) -> Vec<PaletteCommand> {
        let mut commands = Vec::new();
        for stage in self.stages() {
            let detail = words::step_detail(self.feature, stage);
            let card = page::card(&self.models, stage);
            if card.can_load
                && let page::Shown::Staging { model: Some(model) } = &card.shown
            {
                commands.push(
                    PaletteCommand::new(
                        format!("load:{stage}"),
                        t_with("palette-load-model", &[("model", model)]),
                        IconName::Play,
                    )
                    .detail(detail.clone()),
                );
            }
            if let page::Shown::Running { model, idle } = &card.shown
                && !card.busy
            {
                if stage == Stage::Synthesis && !idle {
                    commands.push(
                        PaletteCommand::new(
                            format!("reload:{stage}"),
                            t_with("palette-reload-model", &[("model", model)]),
                            IconName::RotateCw,
                        )
                        .detail(detail.clone()),
                    );
                }
                commands.push(
                    PaletteCommand::new(
                        format!("unload:{stage}"),
                        t_with("palette-unload-model", &[("model", model)]),
                        IconName::Square,
                    )
                    .detail(detail),
                );
            }
        }
        if self.feature == Feature::SpeechToText && self.anything_up(cx) {
            commands.push(PaletteCommand::new(
                "unload-all-models",
                t("unload-all-models"),
                IconName::Square,
            ));
        }
        commands
    }

    fn run(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == "unload-all-models" {
            self.unload_all(window, cx);
            return;
        }
        let Some((verb, stage)) = id.split_once(':') else {
            return;
        };
        let Some(stage) = self.stages().into_iter().find(|s| s.to_string() == stage) else {
            return;
        };
        let input = match verb {
            "load" => page::Input::Load(stage),
            "reload" => page::Input::ReloadModel(stage),
            "unload" => page::Input::Unload(stage),
            _ => return,
        };
        self.send(In::Models(input), window, cx);
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, scenes: &[&str], window: &mut Window, cx: &mut Context<Self>) {
        pretend::pretend(self, scenes, window, cx);
    }
}

impl CorePage for ModelsPage {
    type Input = In;
    type Effect = Out;

    fn send(&mut self, input: In, window: &mut Window, cx: &mut Context<Self>) {
        match input {
            In::Nothing => {}
            In::Models(input) => {
                let outcome = page::update(&mut self.models, input);
                // A notice about the other feature's step is that page's.
                let notice = outcome
                    .notice
                    .filter(|notice| self.has(words::notice_stage(notice)))
                    .map(|notice| Toast::error(words::notice(notice)));
                self.toast(outcome.clear_notice, notice, window, cx);
                let effects = outcome.effects.into_iter().map(Out::Models);
                effects::run_effects(self, effects, window, cx);
            }
            In::Language(input) => {
                let outcome = language::update(&mut self.languages, Some(&mut self.focus), input);
                let notice = outcome
                    .notice
                    .map(|language::Notice::CouldNotSaveLanguage(e)| {
                        Toast::error(t_with("language-could-not-save", &[("error", &e)]))
                    });
                self.toast(outcome.clear_notice, notice, window, cx);
                let effects = outcome.effects.into_iter().map(Out::Language);
                effects::run_effects(self, effects, window, cx);
            }
            In::Voice(input) => {
                let outcome = voice::update(&mut self.voice, input);
                let notice = outcome.notice.map(|voice::Notice::CouldNotSaveVoice(e)| {
                    Toast::error(t_with("voice-could-not-save", &[("error", &e)]))
                });
                self.toast(outcome.clear_notice, notice, window, cx);
                let effects = outcome.effects.into_iter().map(Out::Voice);
                effects::run_effects(self, effects, window, cx);
            }
            In::Configure(input) => {
                let outcome = sheet::update(&mut self.sheet.state, &self.sheet.backends, input);
                let notice = outcome.notice.map(|notice| {
                    Toast::error(match notice {
                        sheet::Notice::CouldNotSave(e) => {
                            t_with("backend-setting-could-not-save", &[("error", &e)])
                        }
                        sheet::Notice::ContextNotChanged(e) => {
                            t_with("backend-context-change-failed", &[("error", &e)])
                        }
                    })
                });
                self.toast(outcome.clear_notice, notice, window, cx);
                let effects = outcome.effects.into_iter().map(Out::Configure);
                effects::run_effects(self, effects, window, cx);
            }
            In::Install(input) => self.install(input, window, cx),
            In::Backends(result) => {
                match result {
                    Ok(backends) => self.sheet.backends = backends,
                    Err(e) => self.toast(
                        false,
                        Some(Toast::error(t_with(
                            "backends-could-not-load",
                            &[("error", &e)],
                        ))),
                        window,
                        cx,
                    ),
                }
                self.send(In::Configure(sheet::Input::BackendsLoaded), window, cx);
            }
            In::SheetOpened { source, result } => self.sheet_opened(source, result, window, cx),
            In::Contexts(catalog) => {
                // A failed read keeps what the picker had: following the
                // active context and no context are offered either way.
                if let Some(catalog) = catalog {
                    self.sheet.contexts = catalog;
                }
            }
        }
        cx.notify();
    }

    fn effect(
        &mut self,
        effect: Out,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<In>> {
        match effect {
            Out::Models(effect) => self.models_effect(effect, window, cx),
            Out::Language(effect) => Some(match effect {
                language::Effect::Read(key) => {
                    runner(async move { In::Language(language::run_read(key).await) })
                }
                language::Effect::ReadOffer(key) => {
                    runner(async move { In::Language(language::run_read_offer(key).await) })
                }
                language::Effect::Save(key, tag) => {
                    runner(async move { In::Language(language::run_save(key, tag).await) })
                }
            }),
            Out::Voice(effect) => Some(match effect {
                voice::Effect::Read { source, model } => {
                    runner(async move { In::Voice(voice::run_read(source, model).await) })
                }
                voice::Effect::List { source, model } => {
                    runner(async move { In::Voice(voice::run_list(source, model).await) })
                }
                voice::Effect::Save {
                    source,
                    model,
                    voice: picked,
                } => runner(async move { In::Voice(voice::run_save(source, model, picked).await) }),
            }),
            Out::Configure(effect) => self.sheet_effect(effect, window, cx),
        }
    }
}

impl ModelsPage {
    /// The steps this page draws, in order.
    fn stages(&self) -> Vec<Stage> {
        match self.feature {
            Feature::SpeechToText => vec![Stage::Transcription, Stage::PostProcessing],
            Feature::TextToSpeech => vec![Stage::Synthesis],
        }
    }

    /// Whether `stage` is one of this page's.
    fn has(&self, stage: Stage) -> bool {
        self.stages().contains(&stage)
    }

    /// Show `toast` as this page's, taking its last one down first when
    /// `clear`.
    fn toast(&self, clear: bool, toast: Option<Toast>, window: &mut Window, cx: &mut App) {
        match self.feature {
            Feature::SpeechToText => toast::notice::<SpeechToTextToast>(window, cx, clear, toast),
            Feature::TextToSpeech => toast::notice::<TextToSpeechToast>(window, cx, clear, toast),
        }
    }

    /// The build's install moved on, as the Library's page takes it.
    fn install(&mut self, input: library::Input, window: &mut Window, cx: &mut Context<Self>) {
        for effect in library::update(&mut self.registry, &mut self.library, input) {
            let next = match effect {
                library::Effect::Installed { source } => page::Input::BuildInstalled { source },
                library::Effect::InstallFailed { source, message } => page::Input::BuildFailed {
                    source,
                    error: message,
                },
                // A backend installed, updated or removed: what can
                // fill each step changed.
                library::Effect::ReloadBackends | library::Effect::ReloadCatalogs => {
                    page::Input::ReloadBackends
                }
                _ => continue,
            };
            self.send(In::Models(next), window, cx);
        }
    }

    /// The backend list was read to open the sheet over `source`.
    fn sheet_opened(
        &mut self,
        source: String,
        result: Result<Vec<BackendInfo>, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The sheet was closed, or opened over another, meanwhile.
        if self.sheet.reading.as_deref() != Some(source.as_str()) {
            return;
        }
        self.sheet.reading = None;
        self.sheet.loaded = true;
        match result {
            Ok(backends) => self.sheet.backends = backends,
            Err(e) => self.toast(
                false,
                Some(Toast::error(t_with(
                    "backends-could-not-load",
                    &[("error", &e)],
                ))),
                window,
                cx,
            ),
        }
        self.send(In::Configure(sheet::Input::Open(source)), window, cx);
        self.send(In::Configure(sheet::Input::BackendsLoaded), window, cx);
    }

    fn models_effect(
        &mut self,
        effect: page::Effect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Runner<In>> {
        use page::Effect;
        // The other feature's steps are its own page's to read.
        if effect_stage(&effect).is_some_and(|stage| !self.has(stage)) {
            return None;
        }
        let models = |work: Runner<page::Input>| -> Option<Runner<In>> {
            Some(runner(async move { In::Models(work.await) }))
        };
        match effect {
            Effect::ReadView(stage) => models(runner(page::run_view(stage))),
            Effect::ReadBackends(stage) => models(runner(page::run_backends(stage))),
            Effect::ReadSecrets(source) => models(runner(page::run_secrets(source))),
            Effect::ReadGpus => models(runner(page::run_gpus())),
            Effect::ReadBackendDevices { stage, source } => {
                models(runner(page::run_backend_devices(stage, source)))
            }
            Effect::ReadDevices {
                stage,
                source,
                model,
            } => models(runner(page::run_devices(stage, source, model))),
            Effect::Poll(stage) => models(runner(page::run_poll(stage))),
            Effect::Focus {
                stage,
                source,
                model,
            } => {
                if stage == Stage::Synthesis {
                    let focus = voice::Input::Focus {
                        source: source.clone(),
                        model: model.clone(),
                    };
                    self.send(In::Voice(focus), window, cx);
                }
                let key = ModelKey::new(stage, source, model);
                self.send(In::Language(language::Input::Focus(key)), window, cx);
                None
            }
            Effect::VoicesChanged => {
                self.send(In::Voice(voice::Input::Reload), window, cx);
                None
            }
            Effect::OfferBuild {
                stage,
                source,
                framework,
            } => {
                self.ask_build(stage, &source, &framework, window, cx);
                None
            }
            Effect::InstallBuild { source, framework } => Some(runner(async move {
                In::Install(page::run_install_build(source, framework).await)
            })),
            change => Some(runner(async move {
                page::run_change(change)
                    .await
                    .map_or(In::Nothing, In::Models)
            })),
        }
    }

    /// Ask whether to download a framework's build, as the core asks.
    fn ask_build(
        &mut self,
        stage: Stage,
        source: &str,
        framework: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let backend = self
            .models
            .catalog
            .backend(stage, source)
            .map_or_else(|| source.to_string(), |b| b.name.clone());
        let framework = accel_name(framework);
        let title = t_with("download-the-build-question", &[("framework", &framework)]);
        let body: SharedString = format!(
            "{} {}",
            t_with(
                "backend-needs-its-build",
                &[("backend", &backend), ("framework", &framework)]
            ),
            t_with("build-moves-the-model", &[("framework", &framework)])
        )
        .into();
        let on_ok = cx.weak_entity();
        let on_cancel = cx.weak_entity();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let on_ok = on_ok.clone();
            let on_cancel = on_cancel.clone();
            alert
                .icon(Icon::new(Lucide::Download).text_color(cx.theme().muted_foreground))
                .title(title.clone())
                .description(body.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t("download"))
                        .cancel_text(t("cancel"))
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    _ = on_ok.update(cx, |page, cx| {
                        page.send(In::Models(page::Input::ConfirmBuild), window, cx);
                    });
                    true
                })
                .on_cancel(move |_, window, cx| {
                    _ = on_cancel.update(cx, |page, cx| {
                        page.send(In::Models(page::Input::CancelBuild), window, cx);
                    });
                    true
                })
        });
    }

    /// A shared setting changed, here or in another app.
    fn setting_changed(
        &mut self,
        setting: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match setting {
            // A model that follows the primary language reads differently.
            Some("language") => {
                self.send(In::Language(language::Input::PrimaryChanged), window, cx);
            }
            // The model's voice was set, or a voice it can be set to was
            // added, renamed or deleted.
            Some("voice" | "voices") if self.feature == Feature::TextToSpeech => {
                self.send(In::Voice(voice::Input::Reload), window, cx);
            }
            // A key or option stored or cleared: whether a step is set up,
            // and what the open sheet shows.
            Some("options" | "secrets") => {
                self.send(In::Models(page::Input::ReloadBackends), window, cx);
                if self.sheet.state.open.is_some() {
                    self.run_sheet_effect(sheet::Effect::ReloadBackends, window, cx);
                }
            }
            Some("contexts") if self.sheet.state.open.is_some() => {
                self.run_sheet_effect(sheet::Effect::ReloadContexts, window, cx);
                self.send(In::Configure(sheet::Input::ContextsChanged), window, cx);
            }
            _ => {}
        }
    }

    /// A pick in one of the page's Selects.
    fn picked(
        &mut self,
        key: &PickerKey,
        id: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let own = |id: &SharedString| (!id.is_empty()).then(|| id.to_string());
        let input = match key {
            PickerKey::Model(stage) => In::Models(page::Input::PickModel(*stage, id.to_string())),
            PickerKey::Device(stage) => In::Models(page::Input::PickDevice(*stage, id.to_string())),
            PickerKey::Language(stage) => {
                let Some(key) = self.focus.get(*stage).cloned() else {
                    return;
                };
                In::Language(language::Input::Picked(key, own(id)))
            }
            PickerKey::Voice => In::Voice(voice::Input::Picked(own(id))),
            PickerKey::Context => {
                let Some(source) = self.sheet.state.open.clone() else {
                    return;
                };
                In::Configure(sheet::Input::PickContext {
                    source,
                    pick: configure::context_pick(id),
                })
            }
            PickerKey::Option(source, name) => In::Configure(sheet::Input::ChooseOption {
                source: source.clone(),
                name: name.clone(),
                value: id.to_string(),
            }),
        };
        self.send(input, window, cx);
    }

    /// Whether any model is up, on either feature: what Unload all models
    /// unloads.
    fn anything_up(&self, cx: &App) -> bool {
        let mine = self.stages().into_iter().any(|stage| {
            page::running_model(&self.models, stage).is_some()
                || page::idle_model(&self.models, stage).is_some()
        });
        mine || self.other_feature_up(cx)
    }

    /// Whether the other feature's model is up, as the window's readiness
    /// says: that page holds its step.
    fn other_feature_up(&self, cx: &App) -> bool {
        let other = match self.feature {
            Feature::SpeechToText => Feature::TextToSpeech,
            Feature::TextToSpeech => Feature::SpeechToText,
        };
        matches!(
            self.shared.readiness.read(cx).of(other),
            Readiness::Ready | Readiness::Idle
        )
    }

    /// Unload every model that is up: this page's through its cards, the
    /// other feature's straight from the daemon, whose events tell that page.
    fn unload_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for stage in self.stages() {
            if page::running_model(&self.models, stage).is_some()
                || page::idle_model(&self.models, stage).is_some()
            {
                self.send(In::Models(page::Input::Unload(stage)), window, cx);
            }
        }
        if self.other_feature_up(cx) {
            let other = match self.feature {
                Feature::SpeechToText => Feature::TextToSpeech,
                Feature::TextToSpeech => Feature::SpeechToText,
            }
            .stage();
            let work = runner(async move {
                page::run_change(page::Effect::Unload(other))
                    .await
                    .map_or(In::Nothing, In::Models)
            });
            effects::run(work, window, cx).detach();
        }
    }
}

/// The step an effect reads for, when it only reads: the other feature's are
/// left to its page.
const fn effect_stage(effect: &page::Effect) -> Option<Stage> {
    use page::Effect;
    match effect {
        Effect::ReadView(stage)
        | Effect::ReadBackends(stage)
        | Effect::Poll(stage)
        | Effect::ReadBackendDevices { stage, .. }
        | Effect::ReadDevices { stage, .. }
        | Effect::Focus { stage, .. } => Some(*stage),
        Effect::VoicesChanged => Some(Stage::Synthesis),
        _ => None,
    }
}

/// The step an event's input is about.
const fn input_stage(input: &page::Input) -> Option<Stage> {
    use page::Input;
    match input {
        Input::Progress(stage, _)
        | Input::Polled(stage, _)
        | Input::Changed(stage)
        | Input::DeviceSwitchFailed(stage, _)
        | Input::Ready { stage, .. }
        | Input::LoadStarted { stage, .. } => Some(*stage),
        _ => None,
    }
}

impl Render for ModelsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let padding = width::page_padding(window);
        let narrow = WindowSize::of(window) == WindowSize::Narrow;
        let column = (width::content(window) - padding * 2.).min(STEPS_MAX);
        let rail = card::Rail::new(narrow);
        let card_width = column - rail.mark - rail.gap;
        let (description, source, sink) = match self.feature {
            Feature::SpeechToText => (
                "models-stt-description",
                (IconName::Mic.into(), t("microphone"), t("your-voice")),
                (
                    Icon::from(Lucide::Type),
                    t("typed"),
                    t("typed-with-the-write-method"),
                ),
            ),
            Feature::TextToSpeech => (
                "models-tts-description",
                (
                    Icon::from(Lucide::AppWindow),
                    t("text"),
                    t("text-from-your-app-or-assistant"),
                ),
                (Icon::from(Lucide::Speaker), t("speakers"), t("read-aloud")),
            ),
        };
        let mut steps: Vec<AnyElement> = vec![rail.edge(source.0, source.1, source.2, false, cx)];
        for stage in self.stages() {
            let filled = self.models.view(stage).source.is_some();
            let body = self.step_card(stage, card_width, window, cx);
            steps.push(rail.step(card::stage_icon(stage), filled, body, cx));
        }
        steps.push(rail.edge(sink.0, sink.1, sink.2, true, cx));
        v_flex()
            .size_full()
            .child(PageHeader::new(self.page).description(t(description)))
            .child(
                div()
                    .id(match self.feature {
                        Feature::SpeechToText => "stt-models",
                        Feature::TextToSpeech => "tts-models",
                    })
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(STEPS_MAX + padding * 2.)
                            .px(padding)
                            .map(|steps| if narrow { steps.pt_4() } else { steps.pt_1() })
                            .pb_6()
                            .children(steps),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each page's palette commands name a stage the way `run` reads it.
    #[test]
    fn a_command_names_its_stage() {
        for stage in Stage::ALL {
            let id = format!("load:{stage}");
            let (_, named) = id.split_once(':').expect("a verb and a stage");
            assert!(Stage::ALL.iter().any(|s| s.to_string() == named));
        }
    }

    #[test]
    fn only_reads_are_left_to_the_other_page() {
        assert_eq!(
            effect_stage(&page::Effect::ReadView(Stage::Synthesis)),
            Some(Stage::Synthesis)
        );
        assert_eq!(effect_stage(&page::Effect::Unload(Stage::Synthesis)), None);
    }
}
