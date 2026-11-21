// SPDX-License-Identifier: GPL-3.0-only
//! The Assistants page: each AI assistant on this computer, connected to Super
//! Voice or not, and keys for any other assistant that supports MCP. The
//! daemon writes each assistant's config and holds the keys; the page shows
//! what it answers.

use chrono::{DateTime, NaiveDate, Utc};
use cosmic::Element;
use cosmic::iced::widget::text::Wrapping;
use cosmic::iced::widget::{Space, column, row};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, Id, button, settings, text};
use super_voice_shared::models::assistants::{
    Assistant, AssistantId, AssistantList, AssistantSession, AssistantState, CustomKey, NewKey,
};

use super::common::{error_banner, muted, page_layout, status_dot};
use crate::state::assistants::{AssistantsState, ConnectDialog, KeyDialog};
use crate::ui::messages::{AssistantsMessage, Message, ShellMessage};

/// The name field in the dialog that makes a key, focused when it opens.
#[must_use]
pub fn key_name_id() -> Id {
    Id::new("assistant_key_name")
}

/// What the assistant can do through Super Voice.
fn does(id: AssistantId) -> String {
    match id {
        AssistantId::ClaudeCode | AssistantId::OpenCode => crate::fl!("assistant-adds-talk"),
        AssistantId::Codex | AssistantId::Antigravity => {
            crate::fl!("assistant-reads-dictated-replies")
        }
    }
}

fn message(message: AssistantsMessage) -> Message {
    Message::Assistants(message)
}

/// A state: a dot in its color and the word, on the neutral pill the window
/// header's states are drawn on.
fn state_pill<'a>(
    color: fn(&cosmic::Theme) -> cosmic::iced::Color,
    word: String,
) -> Element<'a, Message> {
    super::models::header_pill(
        row![status_dot(color, 10.0), super::models::pill_label(word)]
            .spacing(6.0)
            .align_y(Alignment::Center),
    )
}

/// Where a connected assistant stands.
fn connected_pill<'a>() -> Element<'a, Message> {
    state_pill(
        |theme| theme.cosmic().success.base.into(),
        crate::fl!("assistant-connected"),
    )
}

/// The last part of a session's folder, which names it in its row.
fn folder_name(folder: &str) -> String {
    std::path::Path::new(folder).file_name().map_or_else(
        || folder.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A folder as a terminal shows it: under the home folder, from `~`.
fn shown_folder(folder: &str, home: Option<&std::path::Path>) -> String {
    home.and_then(|home| std::path::Path::new(folder).strip_prefix(home).ok())
        .map_or_else(
            || folder.to_string(),
            |rest| {
                if rest.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{}", rest.display())
                }
            },
        )
}

/// When a session last spoke or listened: "Now" within the minute, then
/// minutes, then hours. Nothing before it has, or for what is not a time.
fn used_ago(last_used_at: Option<&str>, now: DateTime<Utc>) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(last_used_at?).ok()?;
    let minutes = (now - at.with_timezone(&Utc)).num_minutes().max(0);
    Some(match minutes {
        0 => crate::fl!("used-now"),
        1..=59 => crate::fl!("used-minutes-ago", minutes = minutes),
        _ => {
            let hours = minutes / 60;
            crate::fl!("used-hours-ago", hours = hours)
        }
    })
}

/// One open session, under the assistant it belongs to: the folder it works
/// in, whether it speaks or listens, and when it last did.
fn session_row(
    session: &AssistantSession,
    home: Option<&std::path::Path>,
    now: DateTime<Utc>,
) -> Element<'static, Message> {
    let spacing = cosmic::theme::spacing();
    let name = session
        .folder
        .as_deref()
        .map_or_else(|| crate::fl!("session-unnamed"), folder_name);
    let mut about = Vec::new();
    if let Some(folder) = &session.folder {
        about.push(shown_folder(folder, home));
    }
    if session.agents > 0 {
        about.push(crate::fl!("session-agents", count = session.agents));
    }
    let mut label = column![text::body(name).wrapping(Wrapping::Word)]
        .spacing(2)
        .width(Length::Fill);
    if !about.is_empty() {
        label = label.push(text::caption(about.join(" · ")).wrapping(Wrapping::Word));
    }
    let mut control = row![].spacing(spacing.space_xs).align_y(Alignment::Center);
    if session.speaking {
        control = control.push(state_pill(
            |theme| theme.cosmic().success.base.into(),
            crate::fl!("session-speaking"),
        ));
    } else if session.listening {
        control = control.push(state_pill(
            |theme| theme.cosmic().warning.base.into(),
            crate::fl!("session-listening"),
        ));
    }
    if let Some(ago) = used_ago(session.last_used_at.as_deref(), now) {
        control = control.push(muted(
            text::caption(ago)
                .width(Length::Fixed(84.0))
                .align_x(cosmic::iced::alignment::Horizontal::Right),
        ));
    }
    // Set in from its assistant's row, so it reads as that assistant's.
    widget::container(settings::item_row(vec![label.into(), control.into()]))
        .padding([0, 0, 0, spacing.space_l])
        .into()
}

/// One assistant's row: what it does, and Connect, or where it stands once
/// connected. One the daemon cannot find has no control.
fn row_for<'a>(assistant: &Assistant, state: &AssistantsState) -> Element<'a, Message> {
    let id = assistant.id;
    let item = settings::item::builder(id.display_name());
    match assistant.state {
        AssistantState::NotFound => item
            .description(crate::fl!("assistant-not-found"))
            .control(Space::new())
            .into(),
        AssistantState::Found => item
            .description(does(id))
            .control(
                button::suggested(crate::fl!("connect"))
                    .on_press(message(AssistantsMessage::AskConnect(id))),
            )
            .into(),
        AssistantState::Connected => connected_row(assistant, state),
    }
}

/// A connected assistant's row. Its switches sit under what it does, so they
/// are plainly this assistant's: whether its `listen` calls use the
/// conversation, and for one whose plugin can tell when it needs the user,
/// whether Super Voice announces it.
fn connected_row<'a>(assistant: &Assistant, state: &AssistantsState) -> Element<'a, Message> {
    let id = assistant.id;
    let spacing = cosmic::theme::spacing();
    let mut description = does(id);
    // An assistant reads its config when it starts.
    if state.connected_here.contains(&id) {
        description.push(' ');
        description.push_str(&crate::fl!(
            "assistant-restart-to-use",
            name = id.display_name()
        ));
    }
    // Laid out as `settings::item`'s own label, with the switch below it.
    let mut label = column![
        text::body(id.display_name()).wrapping(Wrapping::Word),
        text::caption(description).wrapping(Wrapping::Word),
    ]
    .spacing(2)
    .width(Length::Fill);
    let switch = |toggler: widget::Toggler<'a, Message>| {
        widget::container(toggler.spacing(spacing.space_xs)).padding([spacing.space_xxs, 0, 0, 0])
    };
    // The Antigravity CLI has no hook for when it needs the user.
    if id != AssistantId::Antigravity {
        label = label.push(switch(
            widget::toggler(assistant.announce_when_needed)
                .label(crate::fl!("announce-when-needed"))
                .on_toggle(move |on| message(AssistantsMessage::SetAnnounceWhenNeeded(id, on))),
        ));
    }
    label = label.push(switch(
        widget::toggler(assistant.conversation_context)
            .label(crate::fl!("use-conversation-as-context"))
            .on_toggle(move |on| message(AssistantsMessage::SetConversationContext(id, on))),
    ));
    let disconnect = button::standard(crate::fl!("disconnect")).on_press_maybe(
        (!state.disconnecting.contains(&id)).then_some(message(AssistantsMessage::Disconnect(id))),
    );
    let control = row![connected_pill(), disconnect]
        .spacing(spacing.space_xs)
        .align_y(Alignment::Center);
    settings::item_row(vec![label.into(), control.into()]).into()
}

/// When a key was made, as the Notes tab says when a note was: "Today,
/// 09:42", or the date for anything older. The daemon's own words when they
/// are not a time.
fn made_on(created_at: &str, today: NaiveDate) -> String {
    chrono::DateTime::parse_from_rfc3339(created_at).map_or_else(
        |_| created_at.to_string(),
        |time| super::notes::when(time.with_timezone(&chrono::Local).naive_local(), today),
    )
}

/// One key made by hand: its name, when it was made, and Remove.
fn key_row(key: &CustomKey, today: NaiveDate) -> Element<'static, Message> {
    settings::item::builder(key.name.clone())
        .description(made_on(&key.created_at, today))
        .control(
            button::standard(crate::fl!("remove"))
                .on_press(message(AssistantsMessage::RemoveKey(key.id.clone()))),
        )
        .into()
}

/// Any other assistant: a key made by hand, and the keys made so far.
fn another_assistant(list: &AssistantList) -> Element<'_, Message> {
    let mut section = settings::section()
        .title(crate::fl!("another-assistant"))
        .add(
            settings::item::builder(crate::fl!("any-mcp-assistant"))
                .description(crate::fl!("any-mcp-assistant-description"))
                .control(
                    button::standard(crate::fl!("create-key"))
                        .on_press(message(AssistantsMessage::AskKey)),
                ),
        );
    let today = chrono::Local::now().date_naive();
    let home = dirs::home_dir();
    let now = Utc::now();
    for key in &list.custom_keys {
        section = section.add(key_row(key, today));
        for session in &key.sessions {
            section = section.add(session_row(session, home.as_deref(), now));
        }
    }
    section.into()
}

/// What the page is for, in two lines.
fn intro<'a>() -> Element<'a, Message> {
    column![
        text::body(crate::fl!("assistants-intro")),
        muted(text::caption(crate::fl!("assistants-intro-caption"))),
    ]
    .spacing(cosmic::theme::spacing().space_xxs)
    .into()
}

pub fn page<'a>(state: &'a AssistantsState, action_error: Option<&'a str>) -> Element<'a, Message> {
    let mut blocks = Vec::new();
    // While a dialog is open, the error is in it, beside what failed.
    if let Some(error) = action_error.filter(|_| !state.dialog_open()) {
        blocks.push(error_banner(error));
    }
    blocks.push(intro());
    if let Some(list) = &state.list {
        let mut section = settings::section().title(crate::fl!("on-this-computer"));
        let home = dirs::home_dir();
        let now = Utc::now();
        for id in AssistantId::ALL {
            if let Some(assistant) = list.assistants.iter().find(|a| a.id == id) {
                section = section.add(row_for(assistant, state));
                for session in &assistant.sessions {
                    section = section.add(session_row(session, home.as_deref(), now));
                }
            }
        }
        blocks.push(section.into());
        blocks.push(another_assistant(list));
    }
    page_layout(crate::fl!("assistants"), settings::view_column(blocks))
}

/// The dialog over the page, while one is open: Connect, or the key.
pub fn dialog<'a>(
    state: &'a AssistantsState,
    action_error: Option<&'a str>,
) -> Option<Element<'a, Message>> {
    if let Some(connect) = state.connect {
        return Some(connect_dialog(connect, action_error));
    }
    state.key.as_ref().map(|key| key_dialog(key, action_error))
}

/// What the user lets an assistant do by connecting it, asked before the
/// daemon makes its key.
fn connect_dialog(dialog: ConnectDialog, error: Option<&str>) -> Element<'_, Message> {
    let name = dialog.id.display_name();
    let spacing = cosmic::theme::spacing();
    let can = column![
        text::body(crate::fl!("assistant-will-be-able-to", name = name)),
        text::body(crate::fl!("assistant-can-speak")),
        text::body(crate::fl!("assistant-can-listen")),
    ]
    .spacing(spacing.space_xxs);
    let how = match dialog.id {
        AssistantId::Codex => crate::fl!("connect-codex-caption"),
        AssistantId::ClaudeCode | AssistantId::OpenCode | AssistantId::Antigravity => {
            crate::fl!("connect-plugin-caption", name = name)
        }
    };
    let mut question = widget::dialog()
        .title(crate::fl!("connect-assistant-question", name = name))
        .control(can)
        .control(muted(text::caption(how)));
    if let Some(error) = error {
        question = question.control(error_banner(error));
    }
    // Claude Code's plugin can take a minute to install: the button says it
    // is working, and neither answer can be given twice.
    let (connect, answer) = if dialog.connecting {
        (crate::fl!("connecting-assistant"), false)
    } else {
        (crate::fl!("connect"), true)
    };
    question
        .primary_action(
            button::suggested(connect)
                .on_press_maybe(answer.then_some(message(AssistantsMessage::Connect))),
        )
        .secondary_action(
            button::standard(crate::fl!("cancel"))
                .on_press_maybe(answer.then_some(message(AssistantsMessage::CancelConnect))),
        )
        .into()
}

/// The name field, editable until the key is made.
fn name_field(name: &str, editable: bool) -> Element<'_, Message> {
    let mut input = widget::text_input("", name)
        .id(key_name_id())
        .width(Length::Fill);
    if editable {
        input = input
            .on_input(|name| message(AssistantsMessage::KeyNameChanged(name)))
            .on_submit(|_| message(AssistantsMessage::CreateKey));
    }
    column![
        text::body(crate::fl!("name")),
        input,
        muted(text::caption(crate::fl!("key-name-caption"))),
    ]
    .spacing(cosmic::theme::spacing().space_xxs)
    .into()
}

/// One value the assistant needs, with Copy.
fn value_row(label: String, value: &str) -> Element<'_, Message> {
    settings::item_row(vec![
        text::body(label).width(Length::Fixed(72.0)).into(),
        // A key has no spaces to wrap at.
        text::monotext(value)
            .wrapping(Wrapping::Glyph)
            .width(Length::Fill)
            .into(),
        button::standard(crate::fl!("copy"))
            .on_press(Message::Shell(ShellMessage::CopyText(value.to_string())))
            .into(),
    ])
    .into()
}

/// The address and the key just made, each to copy.
fn made_values(key: &NewKey) -> Element<'_, Message> {
    widget::list_column()
        .add(value_row(crate::fl!("address"), &key.address))
        .add(value_row(crate::fl!("key"), &key.key))
        .into()
}

/// A key for an assistant Super Voice does not connect itself: its name, then
/// the address and the key, shown this once.
fn key_dialog<'a>(dialog: &'a KeyDialog, error: Option<&'a str>) -> Element<'a, Message> {
    let title = crate::fl!("key-for-another-assistant");
    match dialog {
        KeyDialog::Naming { name, creating } => {
            let mut naming = widget::dialog()
                .title(title)
                .control(name_field(name, !creating));
            if let Some(error) = error {
                naming = naming.control(error_banner(error));
            }
            let create = if *creating {
                crate::fl!("creating-key")
            } else {
                crate::fl!("create")
            };
            naming
                .primary_action(
                    button::suggested(create).on_press_maybe(
                        dialog
                            .name_to_create()
                            .map(|_| message(AssistantsMessage::CreateKey)),
                    ),
                )
                .secondary_action(
                    button::standard(crate::fl!("cancel")).on_press_maybe(
                        (!creating).then_some(message(AssistantsMessage::CloseKey)),
                    ),
                )
                .into()
        }
        KeyDialog::Made(key) => widget::dialog()
            .title(title)
            .control(name_field(&key.name, false))
            .control(made_values(key))
            .control(muted(text::caption(crate::fl!("key-shown-only-now"))))
            .primary_action(
                button::suggested(crate::fl!("done"))
                    .on_press(message(AssistantsMessage::CloseKey)),
            )
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{folder_name, made_on, shown_folder, used_ago};
    use chrono::{NaiveDate, TimeZone, Utc};
    use std::path::Path;

    /// A session is named by its folder's last part, and shows the folder
    /// from `~` when it is under the home folder.
    #[test]
    fn a_session_shows_its_folder() {
        let home = Some(Path::new("/home/me"));
        assert_eq!(folder_name("/home/me/super-voice"), "super-voice");
        assert_eq!(folder_name("/"), "/");
        assert_eq!(
            shown_folder("/home/me/super-libre/super-voice", home),
            "~/super-libre/super-voice"
        );
        assert_eq!(shown_folder("/home/me", home), "~");
        assert_eq!(shown_folder("/home/meta/x", home), "/home/meta/x");
        assert_eq!(shown_folder("/srv/site", None), "/srv/site");
    }

    /// When a session last spoke or listened reads as now, minutes or hours,
    /// and as nothing before it has.
    #[test]
    fn a_session_shows_when_it_last_spoke_or_listened() {
        let now = Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap();
        // Without Fluent's isolation marks around the numbers.
        let ago =
            |at: &str| used_ago(Some(at), now).map(|ago| ago.replace(['\u{2068}', '\u{2069}'], ""));
        assert_eq!(ago("2026-11-20T09:59:30Z").as_deref(), Some("Now"));
        assert_eq!(ago("2026-11-20T09:57:00Z").as_deref(), Some("3 min ago"));
        assert_eq!(ago("2026-11-20T07:30:00Z").as_deref(), Some("2 h ago"));
        // A clock a little ahead of the app's is still now.
        assert_eq!(ago("2026-11-20T10:00:05Z").as_deref(), Some("Now"));
        assert_eq!(used_ago(None, now), None);
        assert_eq!(ago("soon"), None);
    }

    /// A key's time is the daemon's in UTC, shown in local time; words that
    /// are not a time are shown as they are.
    #[test]
    fn a_key_shows_when_it_was_made() {
        let today = NaiveDate::from_ymd_opt(2030, 1, 1).expect("a date");
        let shown = made_on("2026-11-16T11:03:00Z", today);
        assert!(shown.starts_with("2026-11-1"), "{shown}");
        assert_eq!(made_on("last week", today), "last week");
    }
}
