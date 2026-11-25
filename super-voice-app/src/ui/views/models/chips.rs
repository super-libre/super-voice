// SPDX-License-Identifier: GPL-3.0-only
use cosmic::iced::Alignment;
use cosmic::iced::widget::row;
use cosmic::widget::{self, text};
use cosmic::{Apply, Element};
// The rules behind the chips are the core's; the views reach them here.
use super_voice_app_core::rules::backends::{Capabilities, ModelKind, model_groups};
pub(super) use super_voice_app_core::rules::backends::{
    backend_has_user_url, backend_is_online, model_is_online, stage_clones_voices,
    stage_device_support, stage_hears_context, update_offer,
};

use crate::state::registry::{FeatureFilter, RoleFilter};
use crate::ui::icons;
use crate::ui::messages::Message;

use super::surface::muted_text_color;
use super_voice_app_core::pages::library::Input;

/// Accent chip marking a backend with a newer version — and the control that
/// applies it.
///
/// Shaped like the capability chips beside it but accent-colored and clickable,
/// because unlike them it reports something the user can act on. Being the
/// action as well as the sign is what lets it work on the Models page, whose
/// card carries no other route to an update.
///
/// `tooltips` is off while a card's overflow menu is open, for the same reason
/// the capability chips suppress theirs: a tooltip would paint half-behind the
/// menu.
pub(super) fn update_chip(
    source: &str,
    version: &str,
    tooltips: bool,
) -> Element<'static, Message> {
    let spacing = cosmic::theme::spacing();
    let radius = cosmic::theme::active().cosmic().corner_radii.radius_xl;
    let fg: cosmic::iced::Color = cosmic::theme::active().cosmic().accent.base.into();

    let chip = widget::button::custom(
        row![
            icons::phosphor_tinted(icons::ARROWS_CLOCKWISE, 14.0, fg),
            text::caption(crate::fl!("update")).class(cosmic::theme::Text::Color(fg)),
        ]
        .spacing(spacing.space_xxxs)
        .align_y(Alignment::Center),
    )
    .padding([spacing.space_xxxs, spacing.space_xs])
    .class(super::surface::accent_button_class(radius))
    .on_press(Message::ModelsPage(lib(Input::Update(source.to_string()))))
    .into();

    if tooltips {
        super::surface::rounded_tooltip(
            chip,
            text::body(crate::fl!(
                "update-to-version",
                version = (version).to_string()
            )),
            widget::tooltip::Position::Top,
        )
    } else {
        chip
    }
}

/// The update chip's in-flight form: same shape, muted, and inert.
///
/// Reads the `InstallStatus` a Browse install reports on — an update *is* an
/// install — so the phase and percentage mean what they mean there. It replaces
/// the update chip in place on both cards, so the control the user pressed
/// becomes the progress they are waiting on rather than disappearing.
pub(super) fn update_progress_chip(
    s: &crate::state::registry::InstallStatus,
) -> Element<'static, Message> {
    let label = match (&s.error, s.bytes_total) {
        (Some(_), _) => crate::fl!("update-failed"),
        (None, Some(total)) if total > 0 => {
            crate::fl!(
                "update-backend-percent",
                value0 = ((s.bytes_done * 100) / total).to_string()
            )
        }
        _ => crate::fl!(
            "update-backend-progress",
            value0 = super::download::phase_label(s.phase)
        ),
    };
    let fg = muted_text_color();
    let chip = inert_chip(icons::ARROWS_CLOCKWISE, label, fg);
    match s.failure() {
        // The reason is too long for the chip and too important to drop.
        Some(failure) => super::surface::rounded_tooltip(
            chip,
            text::body(failure),
            widget::tooltip::Position::Top,
        ),
        None => chip,
    }
}

/// The facts row a stage shows for one of its backends: what its models run
/// on, then how many of them the stage can run. The same on the stage's card
/// and on that backend's row in the stage's picker, so a backend reads the
/// same before and after it is chosen.
///
/// The count is of the stage's narrowed models, not everything the backend
/// ships: a backend serving two transcription models and a post-processor
/// offers two to transcription. The Library card holds the full inventory.
pub(super) fn stage_chips(
    app: &crate::core::app::AppModel,
    stage: crate::state::Stage,
    backend: &crate::daemon::backends::BackendInfo,
) -> Option<Element<'static, Message>> {
    let spacing = cosmic::theme::spacing();
    let egress = backend_is_online(backend).then(|| CloudEgress {
        hosts: backend.allowed_hosts.as_slice(),
        user_url: backend_has_user_url(backend),
    });
    let compute = stage_device_support(app.device_offers.backend(stage, &backend.source), backend);
    let caps = capability_chips(
        compute.gpu,
        compute.cpu,
        egress,
        stage_hears_context(stage, backend),
        stage_clones_voices(stage, backend),
        true,
    );
    let count = app.stage_catalog.models(stage, &backend.source).len();
    if caps.is_none() && count == 0 {
        return None;
    }
    let mut chips = row![].spacing(spacing.space_xxs).align_y(Alignment::Center);
    if let Some(caps) = caps {
        chips = chips.push(caps);
    }
    if count > 0 {
        chips = chips.push(count_chip(model_count_label(stage, count)));
    }
    // Wraps rather than squeezes: in a picker row beside its Select button a
    // chip that runs out of width would break its label mid-chip.
    Some(chips.wrap().vertical_spacing(spacing.space_xxs).into())
}

/// "1 model", "9 models", "1 post-processor": what a stage calls its models.
fn model_count_label(stage: crate::state::Stage, count: usize) -> String {
    use crate::state::Stage;
    match (stage, count) {
        (Stage::PostProcessing, 1) => crate::fl!("count-1-post-processor"),
        (Stage::PostProcessing, _) => {
            crate::fl!("post-processors-count", count = count.to_string())
        }
        (Stage::Transcription | Stage::Synthesis, 1) => crate::fl!("count-1-model"),
        (Stage::Transcription | Stage::Synthesis, _) => {
            crate::fl!("models-count", model_count = count.to_string())
        }
    }
}

/// A small rounded "pill" advertising one backend capability — a tinted icon
/// and a short label over a soft, same-hue fill. `fg` is the full-strength
/// tone (icon, text, and border); the fill and border are derived from it at a
/// lower alpha so the chip reads as a tag, not a button.
pub(super) fn capability_chip(
    icon: &'static [u8],
    label: impl Into<String>,
    fg: cosmic::iced::Color,
) -> Element<'static, Message> {
    inert_chip(icon, label.into(), fg)
}

/// The chip shape itself, for a label only known at runtime. `capability_chip`
/// is the fixed-label form; both render identically so a chip built from a
/// progress percentage sits in a row of capability chips without looking
/// foreign.
pub(super) fn inert_chip(
    icon: &'static [u8],
    label: String,
    fg: cosmic::iced::Color,
) -> Element<'static, Message> {
    let spacing = cosmic::theme::spacing();
    chip_shell(
        row![
            icons::phosphor_tinted(icon, 14.0, fg),
            text::caption(label).class(cosmic::theme::Text::Color(fg)),
        ]
        .spacing(spacing.space_xxxs)
        .align_y(Alignment::Center),
        fg,
        spacing.space_xs,
    )
}

/// The pill itself: soft same-hue fill, hairline border, pill radius. Shared so
/// a chip with a glyph alone cannot drift from one carrying a label.
fn chip_shell<'a>(
    content: impl Into<Element<'a, Message>>,
    fg: cosmic::iced::Color,
    h_padding: u16,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let radius = cosmic::theme::active().cosmic().corner_radii.radius_xl;
    let mut fill = fg;
    fill.a = 0.14;
    let mut edge = fg;
    edge.a = 0.32;

    content
        .into()
        .apply(widget::container)
        .padding([spacing.space_xxxs, h_padding])
        .class(cosmic::theme::Container::custom(move |_| {
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(fill)),
                border: cosmic::iced::Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: edge,
                },
                ..Default::default()
            }
        }))
        .into()
}

/// One chip per device a model runs on, glyph only, in the order the
/// backend-level [`capability_chips`] use.
///
/// Every device the model declares, not the first one that matches: a Whisper
/// model runs on GPU *and* CPU, and collapsing that to one badge told the
/// reader the wrong thing about where it can run.
///
/// The glyph carries it without a word. A model list is read down the names,
/// and nine rows each repeating "GPU" beside a graphics-card icon is nine
/// copies of what the icon already said. The tooltip keeps the word reachable.
pub(super) fn model_device_chips(devices: &[String]) -> Vec<Element<'static, Message>> {
    use super::surface::rounded_tooltip;
    let spacing = cosmic::theme::spacing();
    let fg: cosmic::iced::Color = cosmic::theme::active()
        .current_container()
        .component
        .on
        .into();
    let has = |want: &str| devices.iter().any(|d| d == want);

    [
        (
            has("gpu"),
            icons::GRAPHICS_CARD,
            crate::fl!("accelerated-on-gpu"),
        ),
        (has("cpu"), icons::CPU, crate::fl!("runs-on-the-cpu")),
        (has("none"), icons::CLOUD, crate::fl!("runs-in-the-cloud")),
    ]
    .into_iter()
    .filter(|(present, _, _)| *present)
    .map(|(_, glyph, tip)| {
        rounded_tooltip(
            chip_shell(
                icons::phosphor_tinted(glyph, 14.0, fg),
                fg,
                spacing.space_xxs,
            ),
            text::body(tip),
            widget::tooltip::Position::Top,
        )
    })
    .collect()
}

/// A neutral, text-only pill — same shape/tone as [`capability_chip`] but
/// without a leading glyph. Used for the active card's "N models" count.
pub(super) fn count_chip(label: String) -> Element<'static, Message> {
    let spacing = cosmic::theme::spacing();
    let radius = cosmic::theme::active().cosmic().corner_radii.radius_xl;
    let fg: cosmic::iced::Color = cosmic::theme::active()
        .current_container()
        .component
        .on
        .into();
    let mut fill = fg;
    fill.a = 0.14;
    let mut edge = fg;
    edge.a = 0.32;

    text::caption(label)
        .class(cosmic::theme::Text::Color(fg))
        .apply(widget::container)
        .padding([spacing.space_xxxs, spacing.space_xs])
        .class(cosmic::theme::Container::custom(move |_| {
            cosmic::iced::widget::container::Style {
                background: Some(cosmic::iced::Background::Color(fill)),
                border: cosmic::iced::Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: edge,
                },
                ..Default::default()
            }
        }))
        .into()
}

/// How many model names a card lists individually before the rest collapse
/// into a "+N" summary chip. Three keeps the inventory to a single line for
/// typical model-name lengths.
const MAX_MODEL_TAGS: usize = 3;

/// A quiet outline "tag" for one model name: hairline border, no fill, slightly
/// dimmed text, with the gentle `radius_s` corner so it reads as a catalog item
/// rather than a status pill (which the capability chips own, fully rounded).
pub(super) fn model_tag(name: String) -> Element<'static, Message> {
    let spacing = cosmic::theme::spacing();
    let radius = cosmic::theme::active().cosmic().corner_radii.radius_s;
    let on: cosmic::iced::Color = cosmic::theme::active()
        .current_container()
        .component
        .on
        .into();
    let mut edge = on;
    edge.a = 0.28;
    let mut fg = on;
    fg.a = 0.85;

    text::caption(name)
        .class(cosmic::theme::Text::Color(fg))
        .apply(widget::container)
        .padding([spacing.space_xxxs, spacing.space_xs])
        .class(cosmic::theme::Container::custom(move |_| {
            cosmic::iced::widget::container::Style {
                border: cosmic::iced::Border {
                    radius: radius.into(),
                    width: 1.0,
                    color: edge,
                },
                ..Default::default()
            }
        }))
        .into()
}

/// The models a backend ships, split by what they are for.
///
/// A backend may serve both kinds from one install — a weights bundle that
/// transcribes *and* cleans up — and the Library cards say so, since which
/// kinds you get is the first thing that decides whether a backend is the one
/// you want.
pub(super) struct RoleGroup {
    /// Human label for the role, e.g. `"Speech to text"`.
    pub label: String,
    /// The model names in this group, in catalog order.
    pub names: Vec<String>,
}

/// How a kind of model reads on a card.
pub(super) fn kind_label(kind: ModelKind) -> String {
    match kind {
        ModelKind::Transcription => crate::fl!("speech-to-text"),
        ModelKind::PostProcessing => crate::fl!("post-processing"),
        ModelKind::Synthesis => crate::fl!("text-to-speech"),
    }
}

/// How a model `role` reads on a card. An unknown role reads as speech to
/// text (see [`ModelKind::of`]).
pub(super) fn role_label(role: &str) -> String {
    kind_label(ModelKind::of(role))
}

/// Group `(name, role)` pairs by kind, in the core's fixed order
/// ([`model_groups`]), each under its label.
pub(super) fn role_groups<'a>(models: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<RoleGroup> {
    model_groups(models)
        .into_iter()
        .map(|group| RoleGroup {
            label: kind_label(group.kind),
            names: group.names,
        })
        .collect()
}

/// A backend's model inventory: one block per kind of model it ships — a muted
/// role label on its own line, then the first [`MAX_MODEL_TAGS`] names as
/// outline [`model_tag`]s and a filled "+N" [`count_chip`] for the rest. `None`
/// when the backend serves no models, so the caller skips the inventory.
///
/// The label names the kind rather than saying "Models", so a card answers
/// "what does this ship?" without the user opening it. It sits above its tags
/// rather than before them, so every kind's tags start at the card's edge
/// instead of after a label of whatever width that kind's name has.
///
/// The tags wrap rather than squeeze. Three long names are wider than a narrow
/// card: a plain row then breaks a tag's name across two lines inside its pill
/// and crushes the "+N" chip to a sliver. Wrapping moves the tags that do not
/// fit to a second line, whole.
pub(super) fn models_inventory(groups: &[RoleGroup]) -> Option<Element<'static, Message>> {
    if groups.is_empty() {
        return None;
    }
    let spacing = cosmic::theme::spacing();
    let muted = muted_text_color();
    let blocks: Vec<Element<'static, Message>> = groups
        .iter()
        .map(|group| {
            let mut tags = widget::row::with_capacity(MAX_MODEL_TAGS + 1)
                .spacing(spacing.space_xxs)
                .align_y(Alignment::Center);
            for name in group.names.iter().take(MAX_MODEL_TAGS) {
                tags = tags.push(model_tag(name.clone()));
            }
            let rest = group.names.len().saturating_sub(MAX_MODEL_TAGS);
            if rest > 0 {
                tags = tags.push(count_chip(format!("+{rest}")));
            }
            widget::column::with_capacity(2)
                .spacing(spacing.space_xxs)
                .push(text::caption(group.label.clone()).class(cosmic::theme::Text::Color(muted)))
                .push(tags.wrap().vertical_spacing(spacing.space_xxs))
                .into()
        })
        .collect();
    Some(
        cosmic::iced::widget::column(blocks)
            .spacing(spacing.space_s)
            .into(),
    )
}

/// Where an online backend sends audio, as a card can describe it: the hosts
/// its `backend.toml` declares, plus whether the user pointed it at an endpoint
/// of their own.
#[derive(Clone, Copy)]
pub(super) struct CloudEgress<'a> {
    /// The backend's declared `[network].allowed_hosts`.
    pub hosts: &'a [String],
    /// Whether a `base_url` the user set adds an endpoint beyond `hosts`.
    pub user_url: bool,
}

/// The Cloud capability chip: a [`capability_chip`] with a hover tooltip
/// listing the hosts the backend transmits audio to. Shares the GPU/CPU
/// chips' neutral tone so "runs in the cloud" reads as a plain capability,
/// not a golden/premium value judgment.
///
/// A user-set `base_url` is named as a line, never as the address: it is the
/// user's own value, and printing it would put a configured endpoint on a card
/// they may be showing someone.
pub(super) fn cloud_chip(
    fg: cosmic::iced::Color,
    egress: CloudEgress<'_>,
) -> Element<'static, Message> {
    use super::surface::rounded_tooltip;
    let chip = capability_chip(icons::CLOUD, crate::fl!("cloud"), fg);
    if egress.hosts.is_empty() && !egress.user_url {
        return chip;
    }
    let mut popup = widget::column::with_capacity(egress.hosts.len() + 2)
        .push(text::body(crate::fl!("transmits-audio-to")))
        .spacing(cosmic::theme::spacing().space_xxxs);
    for host in egress.hosts {
        popup = popup.push(text::body(format!("• {host}")));
    }
    if egress.user_url {
        popup = popup.push(text::body(crate::fl!("another-url-you-set")));
    }
    rounded_tooltip(chip, popup, widget::tooltip::Position::Top)
}

/// [`capability_chips`] for a backend's capabilities as the core reads them
/// ([`backend_capabilities`](super_voice_app_core::rules::backends::backend_capabilities),
/// [`entry_capabilities`](super_voice_app_core::rules::backends::entry_capabilities)).
pub(super) fn backend_chips(
    caps: &Capabilities,
    tooltips: bool,
) -> Option<Element<'static, Message>> {
    capability_chips(
        caps.compute.gpu,
        caps.compute.cpu,
        caps.cloud.as_ref().map(|egress| CloudEgress {
            hosts: egress.hosts.as_slice(),
            user_url: egress.user_url,
        }),
        caps.contexts,
        caps.voice_cloning,
        tooltips,
    )
}

/// The capability-chip row for a backend: GPU / CPU advertise local compute,
/// Cloud (when `online` is `Some`) flags an online backend, Contexts (when
/// `context`) one that is handed the user's dictation context, and Voice
/// Cloning (when `clones`) one that speaks in voices the user cloned. Returns
/// `None` when there's nothing to advertise, so callers skip the row rather
/// than render an empty band.
///
/// `tooltips` gates the hover popovers (GPU/CPU detail, Cloud host list). The
/// Library installed card passes `!menu_open`: while that card's "⋯" overflow
/// menu is open, its chips drop their tooltips so the menu renders cleanly on
/// top (libcosmic draws the open menu above a tooltip, so a tooltip showing at
/// the same time would paint half-behind it). With the menu closed — and on
/// the active-backend / Browse cards, which have no overflow menu — tooltips
/// show as normal.
// reason: "supports_gpu" / "supports_cpu" are the clearest names, and each
// bool says whether one chip, or the tooltips, show; a struct would only
// rename them at every call
#[allow(clippy::similar_names, clippy::fn_params_excessive_bools)]
pub(super) fn capability_chips(
    supports_gpu: bool,
    supports_cpu: bool,
    online: Option<CloudEgress<'_>>,
    context: bool,
    clones: bool,
    tooltips: bool,
) -> Option<Element<'static, Message>> {
    use super::surface::rounded_tooltip;
    let theme = cosmic::theme::active();
    let neutral: cosmic::iced::Color = theme.current_container().component.on.into();

    let mut chips: Vec<Element<'static, Message>> = Vec::new();
    if supports_gpu {
        let chip = capability_chip(icons::GRAPHICS_CARD, "GPU", neutral);
        chips.push(if tooltips {
            rounded_tooltip(
                chip,
                text::body(crate::fl!("accelerated-on-gpu")),
                widget::tooltip::Position::Top,
            )
        } else {
            chip
        });
    }
    if supports_cpu {
        let chip = capability_chip(icons::CPU, "CPU", neutral);
        chips.push(if tooltips {
            rounded_tooltip(
                chip,
                text::body(crate::fl!("runs-on-the-cpu")),
                widget::tooltip::Position::Top,
            )
        } else {
            chip
        });
    }
    if let Some(egress) = online {
        chips.push(if tooltips {
            cloud_chip(neutral, egress)
        } else {
            capability_chip(icons::CLOUD, crate::fl!("cloud"), neutral)
        });
    }
    if context {
        let chip = capability_chip(icons::CHAT_TEXT, crate::fl!("contexts"), neutral);
        chips.push(if tooltips {
            rounded_tooltip(
                chip,
                text::body(crate::fl!("uses-your-dictation-context")),
                widget::tooltip::Position::Top,
            )
        } else {
            chip
        });
    }
    if clones {
        let chip = capability_chip(icons::USER_SOUND, crate::fl!("voice-cloning"), neutral);
        chips.push(if tooltips {
            rounded_tooltip(
                chip,
                text::body(crate::fl!("speaks-in-voices-you-clone")),
                widget::tooltip::Position::Top,
            )
        } else {
            chip
        });
    }
    if chips.is_empty() {
        return None;
    }
    let spacing = cosmic::theme::spacing();
    Some(
        row(chips)
            .spacing(spacing.space_xxs)
            .align_y(Alignment::Center)
            .wrap()
            .vertical_spacing(spacing.space_xxs)
            .into(),
    )
}

/// A segmented control of mutually-exclusive filter options: a caption label
/// followed by chips butted together inside a single rounded "track", so the
/// group reads as one unified toggle rather than separate buttons. The active
/// chip is filled — accent by default, or a neutral surface when `neutral` is
/// set (used for the secondary "Format" filter); inactive chips are transparent
/// so the track shows through.
/// The "Feature" segmented filter: which feature's models a backend must
/// serve.
///
/// Both Library tabs render it from here, so the wording and the order of the
/// choices cannot drift between Installed and Browse.
pub(super) fn feature_filter_chips(
    current: FeatureFilter,
    on_pick: fn(FeatureFilter) -> Message,
) -> Element<'static, Message> {
    chip_group(
        &crate::fl!("feature"),
        false,
        vec![
            (
                crate::fl!("all"),
                current == FeatureFilter::All,
                on_pick(FeatureFilter::All),
            ),
            (
                crate::fl!("speech-to-text"),
                current == FeatureFilter::SpeechToText,
                on_pick(FeatureFilter::SpeechToText),
            ),
            (
                crate::fl!("text-to-speech"),
                current == FeatureFilter::TextToSpeech,
                on_pick(FeatureFilter::TextToSpeech),
            ),
        ],
    )
}

/// The "Kind" segmented filter: which of speech to text's stages a backend's
/// models must serve. Shown only under [`FeatureFilter::SpeechToText`].
///
/// Both Library tabs render it from here, for the same reason as
/// [`feature_filter_chips`].
pub(super) fn role_filter_chips(
    current: RoleFilter,
    on_pick: fn(RoleFilter) -> Message,
) -> Element<'static, Message> {
    chip_group(
        &crate::fl!("kind"),
        false,
        vec![
            (
                crate::fl!("all"),
                current == RoleFilter::All,
                on_pick(RoleFilter::All),
            ),
            (
                crate::fl!("transcription"),
                current == RoleFilter::Transcription,
                on_pick(RoleFilter::Transcription),
            ),
            (
                crate::fl!("post-processing"),
                current == RoleFilter::PostProcessing,
                on_pick(RoleFilter::PostProcessing),
            ),
        ],
    )
}

/// The "Runs on" segmented filter: local models, cloud models, or both.
///
/// Shared for the same reason [`role_filter_chips`] is — the Installed tab
/// gained this filter after Browse had it, and two copies would drift.
pub(super) fn runs_on_chips(
    current: Option<bool>,
    on_pick: fn(Option<bool>) -> Message,
) -> Element<'static, Message> {
    chip_group(
        &crate::fl!("runs-on"),
        false,
        vec![
            (crate::fl!("all"), current.is_none(), on_pick(None)),
            (
                crate::fl!("local"),
                current == Some(false),
                on_pick(Some(false)),
            ),
            (
                crate::fl!("cloud"),
                current == Some(true),
                on_pick(Some(true)),
            ),
        ],
    )
}

pub(super) fn chip_group(
    label: &str,
    neutral: bool,
    chips: Vec<(String, bool, Message)>,
) -> Element<'static, Message> {
    use cosmic::widget::button;
    let spacing = cosmic::theme::spacing();
    let muted = muted_text_color();

    // Chips with no gap between them; the surrounding track supplies the inset.
    let mut segments = row![].spacing(0).align_y(Alignment::Center);
    for (chip_label, active, msg) in chips {
        let chip = if active {
            if neutral {
                button::standard(chip_label)
            } else {
                button::suggested(chip_label)
            }
        } else {
            button::text(chip_label)
        }
        // Match the font size (14) so the label's line box hugs the glyph and
        // sits vertically centered, rather than floating high in the stock 20px
        // line box. Same centering technique `pill_label` uses (`line_height(1.0)`
        // = 1.0 × 14px); these stay regular buttons, not pills.
        .line_height(14)
        .padding([spacing.space_xxs, spacing.space_s])
        .on_press(msg);
        segments = segments.push(chip);
    }

    // The track: a surface-filled, hairline-bordered, pill-rounded container
    // with a small inset so the active chip visually sits within it.
    let track = widget::container(segments)
        .padding(3)
        .class(cosmic::theme::Container::custom(
            super::surface::pill_surface,
        ));

    row![
        text::caption(label.to_uppercase()).class(cosmic::theme::Text::Color(muted)),
        track
    ]
    .spacing(spacing.space_xs)
    .align_y(Alignment::Center)
    .into()
}

/// The "{shown} backends found" result-count caption above the filter chips,
/// singular at one.
pub(super) fn result_count<'a>(shown: usize) -> Element<'a, Message> {
    let muted = muted_text_color();
    let label = if shown == 1 {
        crate::fl!("count-1-backend-found")
    } else {
        crate::fl!("backends-count", shown = (shown).to_string())
    };
    text::caption(label)
        .class(cosmic::theme::Text::Color(muted))
        .into()
}

/// One unmet-requirement row: a destructive-colored warning glyph and the
/// message `"{label} must be set."`. The active-backend card is the obvious
/// source of the constraint, so the message stays short — no backend name,
/// no internal identifier. Only the icon is tinted red; the text uses the
/// default body color so the row reads cleanly. Non-dismissible: this row
/// disappears the moment the requirement is satisfied (no click required).
pub(super) fn requirement_warning(label: &str) -> Element<'_, Message> {
    row![
        icons::phosphor_destructive(icons::WARNING, 16.0),
        text::body(crate::fl!("required-setting", label = (label).to_string())),
    ]
    .spacing(cosmic::theme::spacing().space_xs)
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod role_group_tests {
    //! A card's model inventory names each kind of model it ships, in a fixed
    //! order, so two backends with the same kinds read the same way round.
    use super::role_groups;

    /// Each group's label and names. The labels are the English catalog's,
    /// which is what the tests run with.
    fn labels(models: &[(&str, &str)]) -> Vec<(String, Vec<String>)> {
        role_groups(models.iter().copied())
            .into_iter()
            .map(|g| (g.label, g.names))
            .collect()
    }

    fn expected(groups: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        groups
            .iter()
            .map(|(label, names)| {
                (
                    (*label).to_string(),
                    names.iter().map(ToString::to_string).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn each_kind_is_a_group_in_pipeline_order() {
        assert_eq!(
            labels(&[
                ("kokoro-82m", "synthesis"),
                ("tidy", "post_processor"),
                ("whisper-small", "transcription"),
            ]),
            expected(&[
                ("Speech to text", &["whisper-small"]),
                ("Post-processing", &["tidy"]),
                ("Text to speech", &["kokoro-82m"]),
            ])
        );
    }

    /// The bug this fixed: a synthesis model was listed as speech to text.
    #[test]
    fn a_synthesis_model_is_text_to_speech() {
        assert_eq!(
            labels(&[("kokoro-82m", "synthesis")]),
            expected(&[("Text to speech", &["kokoro-82m"])])
        );
    }
}

/// A Library input, as the page's message.
fn lib(input: Input) -> crate::ui::messages::ModelsPageMessage {
    crate::ui::messages::ModelsPageMessage::Library(input)
}
