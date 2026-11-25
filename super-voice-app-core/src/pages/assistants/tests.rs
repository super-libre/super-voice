// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use chrono::{TimeZone, Utc};
use super_voice_shared::models::assistants::{
    Assistant, AssistantId, AssistantList, AssistantState,
};

use super::*;

fn list(state: AssistantState) -> AssistantList {
    AssistantList {
        assistants: vec![Assistant {
            id: AssistantId::ClaudeCode,
            state,
            announce_when_needed: false,
            conversation_context: false,
            show_talk: false,
            autosend_dictated_prompts: false,
            sessions: Vec::new(),
        }],
        custom_keys: Vec::new(),
        address: "http://127.0.0.1:7300/v1/mcp".to_string(),
    }
}

/// Connect is asked first, sent once, and a failure leaves the dialog up to
/// try again, with the list read again.
#[test]
fn connecting_asks_then_sends_once_and_survives_a_failure() {
    let mut state = AssistantsState::default();
    update(&mut state, Input::AskConnect(AssistantId::ClaudeCode));
    assert_eq!(
        update(&mut state, Input::Connect).effects,
        [Effect::Connect(AssistantId::ClaudeCode)]
    );
    assert!(
        update(&mut state, Input::Connect).effects.is_empty(),
        "once"
    );
    assert_eq!(
        update(&mut state, Input::CancelConnect),
        Outcome::default(),
        "not while it connects"
    );
    let failed = update(
        &mut state,
        Input::Connected(AssistantId::ClaudeCode, Err("no".into())),
    );
    assert_eq!(failed.effects, [Effect::List]);
    assert_eq!(
        failed.notice,
        Some(Notice::CouldNotConnect(
            AssistantId::ClaudeCode,
            "no".into()
        ))
    );
    assert!(state.connect.is_some_and(|d| !d.connecting));

    update(&mut state, Input::Connect);
    update(
        &mut state,
        Input::Connected(AssistantId::ClaudeCode, Ok(list(AssistantState::Connected))),
    );
    assert!(state.connect.is_none());
    assert!(state.connected_here.contains(&AssistantId::ClaudeCode));
}

/// A switch moves at once; the daemon's answer settles it.
#[test]
fn a_switch_moves_at_once() {
    let mut state = AssistantsState {
        list: Some(list(AssistantState::Connected)),
        ..AssistantsState::default()
    };
    let outcome = update(
        &mut state,
        Input::SetFlag(AssistantId::ClaudeCode, Flag::ShowTalk, true),
    );
    assert_eq!(
        outcome.effects,
        [Effect::SetFlag(
            AssistantId::ClaudeCode,
            Flag::ShowTalk,
            true
        )]
    );
    assert!(
        state
            .list
            .as_ref()
            .is_some_and(|l| l.assistants[0].show_talk)
    );
}

/// Disconnecting its open page goes back to the list.
#[test]
fn a_disconnect_closes_its_page() {
    let mut state = AssistantsState {
        open: Some(AssistantId::ClaudeCode),
        ..AssistantsState::default()
    };
    assert_eq!(
        update(&mut state, Input::Disconnect(AssistantId::ClaudeCode)).effects,
        [Effect::Disconnect(AssistantId::ClaudeCode)]
    );
    assert!(
        update(&mut state, Input::Disconnect(AssistantId::ClaudeCode))
            .effects
            .is_empty(),
        "its button waits"
    );
    update(
        &mut state,
        Input::Disconnected(AssistantId::ClaudeCode, Ok(list(AssistantState::Found))),
    );
    assert_eq!(state.open, None);
    assert!(state.disconnecting.is_empty());
}

/// A key is made once, under a name, and the dialog can't close while it is.
#[test]
fn a_key_is_made_once_and_kept_until_closed() {
    let mut state = AssistantsState::default();
    assert_eq!(
        update(&mut state, Input::AskKey).effects,
        [Effect::FocusKeyName]
    );
    assert!(
        update(&mut state, Input::CreateKey).effects.is_empty(),
        "no name"
    );
    update(&mut state, Input::KeyNameChanged(" Zed ".into()));
    assert_eq!(
        update(&mut state, Input::CreateKey).effects,
        [Effect::CreateKey("Zed".into())]
    );
    update(&mut state, Input::CloseKey);
    assert!(state.key.is_some(), "not while it is being made");
    let made = update(
        &mut state,
        Input::KeyCreated(Ok(NewKey {
            id: "k1".into(),
            name: "Zed".into(),
            key: "secret".into(),
            address: "http://127.0.0.1:7300/v1/mcp".into(),
        })),
    );
    assert_eq!(made.effects, [Effect::List]);
    assert!(matches!(state.key, Some(KeyDialog::Made(_))));
    update(&mut state, Input::CloseKey);
    assert!(state.key.is_none());
}

#[test]
fn each_assistant_has_its_own_switches() {
    assert_eq!(
        flags(AssistantId::ClaudeCode),
        [
            Flag::ShowTalk,
            Flag::AutosendDictatedPrompts,
            Flag::AnnounceWhenNeeded,
            Flag::ConversationContext
        ]
    );
    assert_eq!(
        flags(AssistantId::Codex),
        [Flag::AnnounceWhenNeeded, Flag::ConversationContext]
    );
    assert_eq!(flags(AssistantId::Antigravity), [Flag::ConversationContext]);
}

/// A session is named by its folder's last part, and shows the folder from
/// `~` when it is under the home folder.
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
    assert_eq!(used_ago(Some("2026-11-20T09:59:30Z"), now), Some(Ago::Now));
    assert_eq!(
        used_ago(Some("2026-11-20T09:57:00Z"), now),
        Some(Ago::Minutes(3))
    );
    assert_eq!(
        used_ago(Some("2026-11-20T07:30:00Z"), now),
        Some(Ago::Hours(2))
    );
    assert_eq!(used_ago(Some("2026-11-20T10:00:05Z"), now), Some(Ago::Now));
    assert_eq!(used_ago(None, now), None);
    assert_eq!(used_ago(Some("soon"), now), None);
}

/// A key says when it was made: today or yesterday at a time, the date
/// before that, and the daemon's own words when they are not a time.
#[test]
fn a_key_says_when_it_was_made() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-25T15:00:00Z")
        .expect("a time")
        .with_timezone(&chrono::Utc);
    assert_eq!(
        made_on("2026-11-25T09:42:10Z", &now),
        MadeOn::Today("09:42".into())
    );
    assert_eq!(
        made_on("2026-11-24T22:25:00Z", &now),
        MadeOn::Yesterday("22:25".into())
    );
    assert_eq!(
        made_on("2026-11-16T11:03:00Z", &now),
        MadeOn::Earlier("2026-11-16 11:03".into())
    );
    assert_eq!(
        made_on("last week", &now),
        MadeOn::Unread("last week".into())
    );
}
