// SPDX-License-Identifier: GPL-3.0-only

use super::*;

fn event(name: &str, payload: Value) -> WidgetEvent {
    WidgetEvent {
        name: name.to_string(),
        payload,
    }
}

/// Every subscribed topic must be granted by a requested scope, or the daemon
/// refuses the whole stream with `403 scope_denied`. The mapping lives in
/// super-voice-shared, pinned to the daemon's `Topic::required_scope`.
#[test]
fn the_scopes_cover_the_topics() {
    use super_voice_shared::daemon::widget_subscription::uncovered_topic;
    assert_eq!(uncovered_topic(SETTINGS_SCOPES, TOPICS), None);
}

#[test]
fn a_status_event_keeps_its_timestamp_and_data() {
    let e = event(
        "daemon_status_changed",
        serde_json::json!({
            "status": "ready",
            "model_loaded": true,
            "timestamp": "2026-05-22T12:35:14Z",
        }),
    );
    let n = notification(&e).expect("a notification");
    assert_eq!(n.event_type, "daemon_status_changed");
    assert_eq!(n.timestamp, "2026-05-22T12:35:14Z");
    assert_eq!(n.data["status"], "ready");
}

#[test]
fn a_status_event_without_a_string_timestamp_is_dropped() {
    let missing = event(
        "daemon_status_changed",
        serde_json::json!({ "status": "ready" }),
    );
    assert!(notification(&missing).is_none());
    let number = event(
        "download_progress",
        serde_json::json!({ "percentage": 25.0, "timestamp": 1_716_393_314_i64 }),
    );
    assert!(notification(&number).is_none());
}

fn status(name: &str, feature: &str) -> WidgetEvent {
    event(
        name,
        serde_json::json!({
            "feature": feature,
            "status": "ready",
            "timestamp": "2026-11-04T12:00:00Z",
        }),
    )
}

#[test]
fn each_features_status_events_are_read() {
    for name in ["daemon_status_changed", "download_progress"] {
        for feature in ["stt", "tts"] {
            assert!(
                matches!(parse(&status(name, feature)), Some(Event::Status(_))),
                "{name} from {feature} must reach the app"
            );
        }
    }
}

/// The daemon's own news names no feature, and is everyone's.
#[test]
fn the_daemons_own_status_events_are_read() {
    let e = event(
        "daemon_status_changed",
        serde_json::json!({
            "status": "settings_changed",
            "setting": "language",
            "timestamp": "2026-11-04T12:00:00Z",
        }),
    );
    assert!(matches!(parse(&e), Some(Event::Status(_))));
}

#[test]
fn an_unknown_features_status_events_are_dropped() {
    for name in ["daemon_status_changed", "download_progress"] {
        assert!(
            parse(&status(name, "ocr")).is_none(),
            "{name} from a feature the app doesn't know isn't its own"
        );
    }
}

#[test]
fn speaking_carries_its_utterance() {
    let e = event(
        "speaking_state",
        serde_json::json!({ "is_speaking": true, "utterance_id": "u1" }),
    );
    assert!(matches!(
        parse(&e),
        Some(Event::Speaking { is_speaking: true, utterance_id: Some(id) }) if id == "u1"
    ));
}

#[test]
fn a_level_is_on_the_meters_scale() {
    assert!((level_percent(0.0) - 0.0).abs() < f32::EPSILON);
    assert!((level_percent(0.1) - 1.0).abs() < f32::EPSILON);
    let speech = level_percent(0.005);
    assert!((0.55..0.6).contains(&speech), "{speech}");
    let e = event(
        "playback_frequency_bands",
        serde_json::json!({ "total_energy": 0.005 }),
    );
    assert!(matches!(
        parse(&e),
        Some(Event::PlaybackLevel {
            is_speech: true,
            ..
        })
    ));
}

/// Either feature's settings change names the setting.
#[test]
fn a_settings_change_names_its_setting() {
    let changed =
        |payload| notification(&event("daemon_status_changed", payload)).map(Event::Status);
    let stt = changed(serde_json::json!({
        "status": "settings_changed",
        "setting": "idle_unload",
        "timestamp": "2026-05-22T12:35:14Z",
    }))
    .expect("an event");
    assert_eq!(setting_changed(&stt), Some("idle_unload"));
    let tts = changed(serde_json::json!({
        "feature": "tts",
        "status": "settings_changed",
        "setting": "voice",
        "timestamp": "2026-05-22T12:35:14Z",
    }))
    .expect("an event");
    assert_eq!(setting_changed(&tts), Some("voice"));
    let ready = changed(serde_json::json!({
        "status": "ready",
        "timestamp": "2026-05-22T12:35:14Z",
    }))
    .expect("an event");
    assert_eq!(setting_changed(&ready), None);
    assert_eq!(setting_changed(&Event::Connected), None);
}
