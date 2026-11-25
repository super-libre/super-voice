// SPDX-License-Identifier: GPL-3.0-only
use cosmic::Element;
use cosmic::iced::widget::{column, row};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, text};
use super_voice_shared::models::protocol::{DownloadProgress, LoadProgress, load_progress};

use super_voice_app_core::pages::models::{self as page, Card, Input, Shown};
use super_voice_app_core::state::model_operations::LoadCause;

use crate::core::app::scene::{self, Target};
use crate::core::app::{AppModel, ModelOperationState};
use crate::daemon::backends::BackendInfo;
use crate::state::Stage;
use crate::ui::icons;
use crate::ui::messages::{Message, ModelsMessage, ModelsPageMessage};

use super::chips::{requirement_warning, stage_chips};
use super::fmt::{no_viable_device_warning, vram_warning};

/// `input` for the cards' logic, as the app's message.
pub(super) fn models(input: Input) -> Message {
    Message::Models(ModelsMessage::Page(input))
}
use super::language::language_button;
use super::status::unmet_requirements;
use super::surface::{card_divider, card_surface};

/// The leading glyph tile shared by every backend card, and by the empty state
/// that stands in for one: a soft accent-tinted rounded square holding the
/// "models" brain glyph. It gives each card a consistent anchor on the left
/// that lines up with the two-line name/source block beside it.
pub(super) fn backend_glyph_tile<'a>() -> Element<'a, Message> {
    let accent: cosmic::iced::Color = cosmic::theme::active().cosmic().accent.base.into();
    let mut fill = accent;
    fill.a = 0.16;

    widget::container(icons::phosphor_tinted(icons::BRAIN, 22.0, accent))
        .center_x(Length::Fixed(40.0))
        .center_y(Length::Fixed(40.0))
        .class(cosmic::theme::Container::custom(move |theme| {
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(fill)),
                border: cosmic::iced::Border {
                    radius: theme.cosmic().corner_radii.radius_s.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

/// The selected backend, shown above the tabs: its model picker + Select, an
/// in-card status line (loading / download progress / error), Configure, and
/// Deselect. `width` is the card's: under [`super::surface::NARROW_CARD`] its
/// header stacks (see [`super::surface::card_header`]).
pub(super) fn active_backend_card<'a>(
    backend: &'a BackendInfo,
    app: &'a AppModel,
    width: f32,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let source = backend.source.clone();
    let state = page::card(&app.models, Stage::Transcription);
    let missing = unmet_requirements(&app.configure.secrets_configured, backend);

    // Header: glyph + name/description, then the repo button, Configure, and
    // Deselect. The repo button + description replace the old source caption.
    // Changing backend is ✕ then pick again: one way in, one way out, and no
    // button that silently swaps what a loaded card is about.
    let description = super::surface::backend_description(app, &source);
    let actions = row![
        super::surface::repo_button(&source),
        button::standard(crate::fl!("configure")).on_press(Message::ModelsPage(
            ModelsPageMessage::OpenBackendConfig(source.clone()),
        )),
        super::surface::deselect_button(
            crate::fl!("deselect-this-backend"),
            models(Input::PickBackend(Stage::Transcription, None)),
        ),
    ]
    .spacing(spacing.space_xs)
    .align_y(Alignment::Center);
    let header = super::surface::card_header(
        width < super::surface::NARROW_CARD,
        backend_glyph_tile(),
        backend.name.clone(),
        &backend.version,
        description,
        actions.into(),
    );

    let mut card = widget::column::with_capacity(6)
        .spacing(spacing.space_s)
        .push(header);

    // What the backend runs on and how many models it offers this stage, as
    // its row in the picker showed it, then the update chip.
    let mut chip_row = row![].spacing(spacing.space_xxs).align_y(Alignment::Center);
    if let Some(chips) = stage_chips(app, Stage::Transcription, backend) {
        chip_row = chip_row.push(chips);
    }
    // The update chip is the action as well as the sign, which is what lets it
    // work here: this card has no overflow menu, so without it a waiting
    // version would be invisible on the page the user actually sits on. Once
    // pressed it becomes the progress chip in place — the Library card does the
    // same, and an update started from either is watchable from both.
    let registry_map = app.registry.by_source();
    if let Some(s) = app.registry.installs.get(source.as_str()) {
        chip_row = chip_row.push(super::chips::update_progress_chip(s));
    } else if let Some(v) =
        super::chips::update_offer(registry_map.get(source.as_str()).copied(), false)
    {
        chip_row = chip_row.push(super::chips::update_chip(&source, &v, true));
    }
    card = card.push(chip_row);

    // Loaded or not. When a model is loaded for this backend, or is out of
    // memory until its next use, show a summary and an Unload button;
    // otherwise show the model + device staging row with a Load button.
    // Requirements-unmet skips both — the warnings below are the only path
    // forward. A divider sets the launch controls apart from the header/chips
    // above.
    if missing.is_empty() {
        card = card.push(card_divider());
        card = card.push(match &state.shown {
            Shown::Running { model, .. } => loaded_model_summary(backend, model, app),
            Shown::Staging { .. } | Shown::Empty => staged_model_picker(backend, app, &state),
        });
    }

    // Unmet requirements are surfaced inline so the user fixes them in this
    // same card (Configure) without ever triggering the daemon's safety-net
    // error. Card-scoped: no need to repeat the backend name in each line.
    for label in &missing {
        card = card.push(requirement_warning(label));
    }

    // The model operation status, when it is this stage's.
    if let Some(status) = operation_status(app, Stage::Transcription) {
        card = card.push(status);
    }

    if missing.is_empty() && matches!(state.shown, Shown::Running { idle: true, .. }) {
        card = card.push(super::surface::idle_label());
    }

    card_surface(card, true)
}

/// The device dropdown for one stage's `model`, or `None` when there is
/// nothing to offer.
///
/// Rendered whether or not the model is loaded, and that is the point: a
/// running model's device can be changed in place — the daemon reloads it onto
/// the new one — so hiding the picker once it is up made the only route
/// through unload, re-pick, load.
///
/// `None` covers two cases the caller must not distinguish: the daemon has not
/// answered yet, and it answered with nothing. The answer is already the
/// model's `supported_devices` narrowed to what this install and host can do,
/// so an online model and a GPU the install cannot use both fall out here.
pub(super) fn device_dropdown<'a>(
    stage: Stage,
    source: &str,
    model: &str,
    app: &'a AppModel,
) -> Option<Element<'a, Message>> {
    let devices: Vec<String> = app.models.offers.model(stage, source, model)?.to_vec();
    if devices.is_empty() {
        return None;
    }
    let selected = app
        .models
        .picks
        .device(stage)
        .and_then(|d| devices.iter().position(|x| x == d));
    let labels: Vec<String> = devices.iter().map(|d| super::fmt::accel_name(d)).collect();
    let pick = devices.clone();
    let dropdown = widget::dropdown(labels, selected, move |index| {
        models(Input::PickDevice(stage, pick[index].clone()))
    })
    .placeholder(crate::fl!("select"));
    // The transcription card's is the one `just screenshots` opens.
    Some(if stage == Stage::Transcription {
        scene::tag(Target::DeviceMenu, dropdown)
    } else {
        dropdown.into()
    })
}

/// The line under a running model's Runs on row, when it is not running on
/// the framework picked. Not while a switch or a build is under way: the model
/// is still on the old framework then because it has not moved yet, and the
/// card's progress already says so.
pub(super) fn device_note(card: &Card<'_>) -> Option<String> {
    card.fallback.as_deref().map(super::fmt::running_note)
}

/// Summary shown in the active-backend card when `model` is loaded for this
/// backend, or out of memory until its next use. Reads as e.g. "Active:
/// whisper-1" with its device picker, and an Unload button on the right; the
/// Unload click drops the model but keeps the active backend — and the model —
/// selected.
pub(super) fn loaded_model_summary<'a>(
    backend: &'a BackendInfo,
    model: &str,
    app: &'a AppModel,
) -> Element<'a, Message> {
    let running = text::body(model.to_string()).class(cosmic::theme::Text::Accent);
    let mut rows: Vec<super::surface::ControlRow<'a>> =
        vec![(crate::fl!("model"), None, running.into())];
    let mut buttons = Vec::new();
    // The same picker the unloaded card shows, in the same place. Changing it
    // now reloads the model onto the new device rather than staging a choice
    // for a Load that is not coming.
    if let Some(devices) = device_dropdown(Stage::Transcription, &backend.source, model, app) {
        let note = device_note(&page::card(&app.models, Stage::Transcription));
        rows.push((crate::fl!("runs-on"), note, devices));
    }
    // Per-model language, for a multilingual model.
    if let Some(lang_button) = language_button(Stage::Transcription, backend, model, app) {
        rows.push((crate::fl!("language"), None, lang_button));
    }
    // A leading stop glyph fronts the Unload label, mirroring the Load button's
    // play icon so load/unload read as a play/stop pair.
    buttons.push(
        button::standard(crate::fl!("unload"))
            .leading_icon(icons::phosphor_handle(icons::STOP))
            .on_press(models(Input::Unload(Stage::Transcription)))
            .into(),
    );
    super::surface::control_rows(rows, buttons)
}

/// The line under a staged model's Model row: how much memory it declares it
/// needs, so the size of what Load is about to download and run is known
/// before the click. `None` for a model that declares no estimate (an online
/// one) or that the backend does not list.
///
/// The memory estimate, not the download: a manifest names each file's URL
/// and checksum but not its size, and the estimate is what decides whether
/// the model runs here at all.
pub(super) fn memory_note(card: &Card<'_>) -> Option<String> {
    card.memory
        .map(|bytes| crate::fl!("model-needs-memory", size = super::fmt::fmt_size(bytes)))
}

/// Model + device pickers and the Load button, shown in the active-backend
/// card when no model is loaded for this backend. Picking a model stages it
/// (and asks the daemon for the model's own device); picking a device stages
/// it too; the Load button commits both, setting the model's device and then
/// loading it via `set_model`. The device dropdown lists what the daemon
/// answered for the staged model (`/stt/pipeline/1/model/{model}/device/list`) and
/// is omitted whenever that list is empty: an online model, or a model with no
/// viable device on this install.
pub(super) fn staged_model_picker<'a>(
    backend: &'a BackendInfo,
    app: &'a AppModel,
    state: &Card<'_>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    // Model dropdown, scoped to this stage's role: a post-processor here would
    // be a pick the daemon then refuses.
    let model_names = state.models.clone();
    let staged_model = match &state.shown {
        Shown::Staging { model } => model.clone(),
        Shown::Running { .. } | Shown::Empty => None,
    };
    let model_index = staged_model
        .as_deref()
        .and_then(|m| model_names.iter().position(|n| n == m));
    let model_names_pick = model_names.clone();
    let model_dropdown = widget::dropdown(model_names, model_index, move |index| {
        models(Input::PickModel(
            Stage::Transcription,
            model_names_pick[index].clone(),
        ))
    })
    .placeholder(crate::fl!("select-model"));

    let mut rows: Vec<super::surface::ControlRow<'a>> = vec![(
        crate::fl!("model"),
        memory_note(state),
        scene::tag(Target::ModelMenu, model_dropdown),
    )];
    let mut buttons = Vec::new();

    // Device dropdown — only once the daemon has answered what the staged
    // model can run on here, and only when that answer offers something. The
    // answer is already the model's `supported_devices` narrowed to what this
    // install and host can do, so an online model (offering nothing) and a GPU
    // the install cannot use both fall out of the same check.
    if let Some(model) = staged_model.as_deref()
        && let Some(devices) = device_dropdown(Stage::Transcription, &backend.source, model, app)
    {
        rows.push((crate::fl!("runs-on"), None, devices));
    }

    // Per-model language, after the device — shown only for a staged
    // multilingual model.
    if let Some(model) = staged_model.as_deref()
        && let Some(lang_button) = language_button(Stage::Transcription, backend, model, app)
    {
        rows.push((crate::fl!("language"), None, lang_button));
    }

    // Load — enabled once a model is staged on a device (or is online, and
    // needs none), while the stage is free.
    let load_button = button::suggested(crate::fl!("load-model"))
        .leading_icon(icons::phosphor_handle(icons::PLAY))
        .on_press_maybe(
            state
                .can_load
                .then_some(models(Input::Load(Stage::Transcription))),
        );
    buttons.push(load_button.into());
    let picker_row = super::surface::control_rows(rows, buttons);

    // Below the picker: either the "can't be loaded here" advisory for a
    // staged model with no viable device (blocking — Load is already
    // disabled above), or a staged GPU load whose conservative VRAM estimate
    // exceeds the GPU's available memory (non-blocking). The two can't both
    // fire.
    if state.no_device {
        column![
            picker_row,
            no_viable_device_warning(staged_model.as_deref().unwrap_or_default())
        ]
        .spacing(spacing.space_xs)
        .into()
    } else if let Some((needed, available)) = state.vram_shortfall {
        column![picker_row, vram_warning(needed, available)]
            .spacing(spacing.space_xs)
            .into()
    } else {
        picker_row
    }
}

/// The in-flight model operation, on the card that owns it.
///
/// The stages provision independently, so each card reads its own stage: a
/// post-processor's download belongs off the transcription card, and a
/// transcription model's off the post-processing one, even while both run.
pub(super) fn operation_status(app: &AppModel, stage: Stage) -> Option<Element<'_, Message>> {
    if let Some(build) = card_build_progress(app, stage) {
        return Some(build);
    }
    match app.models.operations.get(stage) {
        ModelOperationState::Ready => None,
        ModelOperationState::Provisioning {
            target_model,
            progress,
        } => Some(card_provisioning_progress(stage, target_model, progress)),
        ModelOperationState::Loading {
            target_model,
            cause,
            load,
        } => Some(match load {
            Some(load) => card_load_progress(target_model, load),
            None => text::body(crate::fl!(
                "loading-model-status",
                target_model = target_model.as_str(),
                status_message = load_cause(cause)
            ))
            .into(),
        }),
        // A failure the daemon gave no reason for.
        ModelOperationState::Error { message } if message.is_empty() => {
            Some(card_error(crate::fl!("model-switch-failed")))
        }
        ModelOperationState::Error { message } => Some(card_error(message.clone())),
    }
}

/// What a load says while the backend says nothing of its own.
fn load_cause(cause: &LoadCause) -> String {
    match cause {
        LoadCause::Switch => crate::fl!("progress-starting-switch"),
        LoadCause::Reload => crate::fl!("progress-reloading-model"),
        LoadCause::Move { device } => crate::fl!(
            "progress-loading-on",
            device = super::fmt::accel_name(device)
        ),
        LoadCause::Reported => crate::fl!("progress-loading-model"),
    }
}

/// How the card names the phase the daemon reports. `verifying` is checking
/// files already on disk against their checksums; everything else the daemon
/// routes to this card is bytes coming off the network, and an unrecognised
/// status reads as a download because that is what every status other than
/// `verifying` has ever meant here.
pub(super) fn provisioning_verb(status: &str) -> String {
    if status == "verifying" {
        crate::fl!("verifying")
    } else {
        crate::fl!("downloading")
    }
}

/// In-card provisioning progress (bar + text + Cancel), shown at the bottom of
/// the active-backend card while the model's files are being verified or
/// downloaded.
///
/// The verb comes from the daemon's phase, and that distinction is the whole
/// point: loading a model whose files are all present still walks every file to
/// check it, and for one whose verification stamp is missing that means hashing
/// it end to end — seconds of real work for multi-GB weights. Calling that
/// "Downloading" told the user their files were being fetched again. Both
/// phases are byte-tracked the same way, so they share one bar.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
pub(super) fn card_provisioning_progress<'a>(
    stage: Stage,
    target_model: &'a str,
    progress: &'a DownloadProgress,
) -> Element<'a, Message> {
    let fraction = if progress.percentage < 0.0 || progress.percentage > 100.0 {
        0.0
    } else {
        (progress.percentage / 100.0).clamp(0.0, 1.0)
    };
    let line = crate::fl!(
        "provision-model-progress",
        value0 = provisioning_verb(&progress.status),
        value1 = (target_model).to_string(),
        value2 = (progress.file_index + 1).to_string(),
        value3 = (progress.total_files).to_string(),
        value4 = format!("{:.1}", progress.percentage)
    );
    let bytes = if progress.total_bytes > 0 {
        let mb = progress.bytes_downloaded as f64 / (1024.0 * 1024.0);
        let total = progress.total_bytes as f64 / (1024.0 * 1024.0);
        crate::fl!(
            "download-size",
            mb = format!("{:.1}", mb),
            total = format!("{:.1}", total)
        )
    } else {
        String::new()
    };
    column![
        text::body(line),
        widget::determinate_linear(fraction.max(0.05)).width(Length::Fill),
        row![
            text::caption(bytes).width(Length::Fill),
            widget::button::destructive(crate::fl!("cancel"))
                // The card's own stage, so the cancel reaches the download
                // this card is showing.
                .on_press(models(Input::CancelDownload(stage))),
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(cosmic::theme::spacing().space_xs)
    .into()
}

/// The bar for a build the user agreed to download for `stage`, in the place
/// the card shows a model's download, so the build and the restart that
/// follows it read as one wait. No Cancel: an install runs to its end.
// reason: display-only; the imprecision is cosmetic
#[allow(clippy::cast_precision_loss)]
fn card_build_progress(app: &AppModel, stage: Stage) -> Option<Element<'_, Message>> {
    use super_voice_shared::registry::events::InstallPhase;
    let pending = app.models.build_downloading(stage)?;
    let name = super::fmt::accel_name(&pending.framework);
    let status = app.registry.installs.get(&pending.source);
    let (phase, done, total) = status.map_or((InstallPhase::Resolving, 0, None), |s| {
        (s.phase, s.bytes_done, s.bytes_total.filter(|t| *t > 0))
    });
    let downloading = matches!(phase, InstallPhase::Resolving | InstallPhase::Downloading);
    let (line, fraction) = match (downloading, total) {
        (true, Some(total)) => {
            let fraction = (done as f32 / total as f32).clamp(0.0, 1.0);
            (
                crate::fl!(
                    "downloading-the-build-percent",
                    framework = name.as_str(),
                    percent = format!("{:.0}", fraction * 100.0)
                ),
                fraction,
            )
        }
        (true, None) => (
            crate::fl!("downloading-the-build", framework = name.as_str()),
            0.0,
        ),
        (false, _) => (
            crate::fl!("installing-the-build", framework = name.as_str()),
            1.0,
        ),
    };
    let bytes = match total {
        Some(total) if downloading => {
            let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
            crate::fl!(
                "download-size",
                mb = format!("{:.1}", mb(done)),
                total = format!("{:.1}", mb(total))
            )
        }
        _ => String::new(),
    };
    Some(
        column![
            text::body(line),
            widget::determinate_linear(fraction.max(0.05)).width(Length::Fill),
            text::caption(bytes),
        ]
        .spacing(cosmic::theme::spacing().space_xs)
        .into(),
    )
}

/// The card's title while a backend loads: "Initial setup" when the backend
/// says this load pays a one-time cost, so the user knows the wait will not
/// repeat, and the model being loaded otherwise.
fn load_title(load: &LoadProgress, target_model: &str) -> String {
    if load.phase.as_deref() == Some(load_progress::phase::INITIAL_SETUP) {
        crate::fl!("initial-setup")
    } else {
        crate::fl!("loading-model", target_model = (target_model).to_string())
    }
}

/// What the backend says it is doing, in words. An id this app does not know
/// is still a load in progress, so it reads as one rather than as an error.
pub(super) fn load_step(step: Option<&str>) -> String {
    match step {
        Some(load_progress::step::LOADING_WEIGHTS) => crate::fl!("loading-weights"),
        Some(load_progress::step::BUILDING_KERNELS) => crate::fl!("building-kernels"),
        Some(load_progress::step::WARMING_UP) => crate::fl!("warming-up"),
        _ => crate::fl!("loading"),
    }
}

/// In-card load progress, as the backend reports it: a title, the step, and a
/// bar when the backend can say how far through the step it is.
///
/// A backend that compiles its GPU kernels on the first load spends minutes
/// there. Before backends reported it, this card said "Loading model into
/// memory" for all of it, which reads as stuck.
fn card_load_progress<'a>(target_model: &str, load: &LoadProgress) -> Element<'a, Message> {
    let title = text::body(load_title(load, target_model));
    let step = load_step(load.step.as_deref());
    match load.progress {
        Some(fraction) => column![
            title,
            row![
                text::caption(step).width(Length::Fill),
                text::caption(format!("{:.0}%", fraction * 100.0)),
            ],
            widget::determinate_linear(fraction.clamp(0.0, 1.0).max(0.02)).width(Length::Fill),
        ]
        .spacing(cosmic::theme::spacing().space_xs)
        .into(),
        None => column![title, text::caption(step)]
            .spacing(cosmic::theme::spacing().space_xs)
            .into(),
    }
}

/// In-card model error: a destructive-colored warning glyph plus the daemon's
/// message. Not dismissible — the error is tied to the backend's state, and
/// clears the moment the user fixes the underlying issue (or picks another
/// model from the dropdown above). The Configure button up in the card header
/// is the user-actionable path.
pub(super) fn card_error<'a>(message: String) -> Element<'a, Message> {
    row![
        icons::phosphor_destructive(icons::WARNING, 18.0),
        text::body(message),
    ]
    .spacing(cosmic::theme::spacing().space_xs)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod provisioning_verb_tests {
    //! The card takes its verb from the daemon's phase. A load whose files are
    //! all on disk spends its whole life in `verifying` — calling that
    //! "Downloading" is what had every cached load showing a download bar for
    //! files the user already had.
    use super::provisioning_verb;

    #[test]
    fn verifying_says_so_rather_than_claiming_a_download() {
        assert_eq!(provisioning_verb("verifying"), "Verifying");
    }

    #[test]
    fn downloading_is_still_a_download() {
        assert_eq!(provisioning_verb("downloading"), "Downloading");
    }

    /// An unrecognised phase reads as a download: every status the daemon has
    /// ever routed to this card other than `verifying` is one.
    #[test]
    fn an_unknown_phase_falls_back_to_downloading() {
        assert_eq!(provisioning_verb("something_new"), "Downloading");
    }
}

#[cfg(test)]
mod load_progress_tests {
    //! The words the load card uses for what a backend reports.
    use super::{LoadProgress, load_step, load_title};

    /// A first load says it is setup, so the user knows the wait is a one-off;
    /// any other load names the model.
    #[test]
    fn a_first_load_is_titled_as_setup() {
        let setup = LoadProgress {
            phase: Some("initial_setup".to_string()),
            ..LoadProgress::default()
        };
        assert_eq!(load_title(&setup, "voxtral-mini"), "Initial setup");
        let ordinary = LoadProgress {
            phase: Some("loading".to_string()),
            ..LoadProgress::default()
        };
        assert_eq!(
            load_title(&ordinary, "voxtral-mini"),
            "Loading \u{2068}voxtral-mini\u{2069}"
        );
        assert_eq!(
            load_title(&LoadProgress::default(), "voxtral-mini"),
            "Loading \u{2068}voxtral-mini\u{2069}"
        );
    }

    /// Every id in the contract's vocabulary has its words, and one this
    /// build does not know still reads as a load.
    #[test]
    fn each_step_reads_as_words() {
        assert_eq!(load_step(Some("loading_weights")), "Loading weights");
        assert_eq!(load_step(Some("building_kernels")), "Building kernels");
        assert_eq!(load_step(Some("warming_up")), "Warming up");
        assert_eq!(load_step(Some("defragmenting_vram")), "Loading");
        assert_eq!(load_step(None), "Loading");
    }
}
