// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::daemon::client::ContextMode;

const SOURCE: &str = "github.com/super-stt/openai";

/// A backend with an API key, a switch on by default, a list defaulting to
/// "fast", and a base URL set away from its default.
fn backends(supports: &[&str]) -> Vec<BackendInfo> {
    vec![
        serde_json::from_value(serde_json::json!({
            "source": SOURCE,
            "name": "OpenAI",
            "models": [],
            "secrets": [{ "name": "api_key", "label": "API key", "required": true }],
            "options": [
                { "name": "stream", "type": "bool", "default": "true", "value": "true" },
                { "name": "speed", "choices": ["fast", "slow"], "default": "fast", "value": "fast" },
                { "name": "base_url", "default": "https://a", "value": "https://b" },
            ],
            "supports": supports,
        }))
        .expect("a backend"),
    ]
}

fn named(name: &str) -> (String, String) {
    (SOURCE.to_string(), name.to_string())
}

#[test]
fn the_list_seeds_the_option_fields_and_reads_the_secrets() {
    let mut state = ConfigureState::default();
    state.secrets_configured.insert(named("gone"), true);
    let out = update(&mut state, &backends(&[]), Input::BackendsLoaded);
    assert_eq!(out.effects, [Effect::ListSecrets(SOURCE.into())]);
    assert_eq!(state.option_input(SOURCE, "base_url"), "https://b");
    assert!(state.secrets_configured.is_empty());

    update(
        &mut state,
        &backends(&[]),
        Input::SecretsListed {
            source: SOURCE.into(),
            result: Ok(vec![("api_key".into(), true)]),
        },
    );
    assert!(state.secret_configured(SOURCE, "api_key"));
}

#[test]
fn toggling_to_the_default_clears_the_override() {
    let mut state = ConfigureState::default();
    let toggle = |value| Input::ToggleOption {
        source: SOURCE.into(),
        name: "stream".into(),
        value,
    };
    let out = update(&mut state, &backends(&[]), toggle(true));
    assert_eq!(
        out.effects,
        [Effect::ClearOption {
            source: SOURCE.into(),
            name: "stream".into()
        }]
    );
    assert!(out.clear_notice);
    let out = update(&mut state, &backends(&[]), toggle(false));
    assert_eq!(
        out.effects,
        [Effect::SetOption {
            source: SOURCE.into(),
            name: "stream".into(),
            value: "false".into()
        }]
    );
}

#[test]
fn picking_the_default_clears_the_override() {
    let mut state = ConfigureState::default();
    let choose = |value: &str| Input::ChooseOption {
        source: SOURCE.into(),
        name: "speed".into(),
        value: value.into(),
    };
    let out = update(&mut state, &backends(&[]), choose("fast"));
    assert!(matches!(out.effects[..], [Effect::ClearOption { .. }]));
    let out = update(&mut state, &backends(&[]), choose("slow"));
    assert!(matches!(&out.effects[..], [Effect::SetOption { value, .. }] if value == "slow"));
}

#[test]
fn an_emptied_text_option_is_cleared() {
    let mut state = ConfigureState::default();
    let save = || Input::SaveOption {
        source: SOURCE.into(),
        name: "base_url".into(),
    };
    update(
        &mut state,
        &backends(&[]),
        Input::OptionTyped {
            source: SOURCE.into(),
            name: "base_url".into(),
            value: "https://c".into(),
        },
    );
    let out = update(&mut state, &backends(&[]), save());
    assert!(matches!(&out.effects[..], [Effect::SetOption { value, .. }] if value == "https://c"));
    state.option_inputs.insert(named("base_url"), String::new());
    let out = update(&mut state, &backends(&[]), save());
    assert!(matches!(out.effects[..], [Effect::ClearOption { .. }]));
}

#[test]
fn an_empty_secret_is_not_saved() {
    let mut state = ConfigureState::default();
    let save = || Input::SaveSecret {
        source: SOURCE.into(),
        name: "api_key".into(),
    };
    assert!(
        update(&mut state, &backends(&[]), save())
            .effects
            .is_empty()
    );
    state.secret_inputs.insert(named("api_key"), String::new());
    assert!(
        update(&mut state, &backends(&[]), save())
            .effects
            .is_empty()
    );
}

#[test]
fn a_stored_secret_drops_its_field_and_reloads() {
    let mut state = ConfigureState::default();
    update(
        &mut state,
        &backends(&[]),
        Input::SecretTyped {
            source: SOURCE.into(),
            name: "api_key".into(),
            value: "sk-1".into(),
        },
    );
    let out = update(
        &mut state,
        &backends(&[]),
        Input::SaveSecret {
            source: SOURCE.into(),
            name: "api_key".into(),
        },
    );
    assert!(matches!(&out.effects[..], [Effect::SaveSecret { value, .. }] if value == "sk-1"));

    // A failure keeps what was typed, to try again.
    let failed = |result| Input::SecretSaved {
        source: SOURCE.into(),
        name: "api_key".into(),
        result,
    };
    let out = update(&mut state, &backends(&[]), failed(Err("down".into())));
    assert_eq!(out.notice, Some(Notice::CouldNotSave("down".into())));
    assert_eq!(state.secret_input(SOURCE, "api_key"), "sk-1");

    let out = update(&mut state, &backends(&[]), failed(Ok(())));
    assert_eq!(out.effects, [Effect::ReloadBackends]);
    assert_eq!(state.secret_input(SOURCE, "api_key"), "");
}

#[test]
fn a_write_reloads_or_says_why_not() {
    let mut state = ConfigureState::default();
    let out = update(&mut state, &backends(&[]), Input::Written(Ok(())));
    assert_eq!(out.effects, [Effect::ReloadBackends]);
    let out = update(&mut state, &backends(&[]), Input::Written(Err("no".into())));
    assert_eq!(out.notice, Some(Notice::CouldNotSave("no".into())));
    assert!(out.effects.is_empty());
}

#[test]
fn opening_reads_the_context_only_for_a_backend_that_takes_one() {
    let mut state = ConfigureState::default();
    let out = update(&mut state, &backends(&[]), Input::Open(SOURCE.into()));
    assert_eq!(out.effects, [Effect::ReloadContexts]);
    assert!(out.clear_notice);
    assert_eq!(state.open.as_deref(), Some(SOURCE));

    let takes = backends(&[supports::DICTATION_CONTEXT]);
    let out = update(&mut state, &takes, Input::Open(SOURCE.into()));
    assert_eq!(
        out.effects,
        [Effect::ReloadContexts, Effect::ReadContext(SOURCE.into())]
    );
}

#[test]
fn changed_contexts_read_the_open_sheets_context_again() {
    let mut state = ConfigureState::default();
    let list = backends(&[supports::DICTATION_CONTEXT]);
    update(&mut state, &list, Input::Open(SOURCE.into()));
    // Not read yet: nothing to refresh.
    assert!(
        update(&mut state, &list, Input::ContextsChanged)
            .effects
            .is_empty()
    );
    update(
        &mut state,
        &list,
        Input::ContextRead {
            source: SOURCE.into(),
            result: Ok(BackendContext::default()),
        },
    );
    let out = update(&mut state, &list, Input::ContextsChanged);
    assert_eq!(out.effects, [Effect::ReadContext(SOURCE.into())]);

    update(&mut state, &list, Input::Close);
    assert!(
        update(&mut state, &list, Input::ContextsChanged)
            .effects
            .is_empty()
    );
}

#[test]
fn a_failed_pick_is_said_and_a_failed_read_is_not() {
    let mut state = ConfigureState::default();
    let out = update(
        &mut state,
        &[],
        Input::ContextPicked {
            source: SOURCE.into(),
            result: Err("down".into()),
        },
    );
    assert_eq!(out.notice, Some(Notice::ContextNotChanged("down".into())));
    let out = update(
        &mut state,
        &[],
        Input::ContextRead {
            source: SOURCE.into(),
            result: Err("down".into()),
        },
    );
    assert_eq!(out, Outcome::default());
}

#[test]
fn a_pin_to_a_deleted_context_reads_as_none() {
    let ids = vec!["coding".to_string()];
    let pinned = |id: &str| BackendContext {
        mode: ContextMode::Pinned,
        id: Some(id.into()),
        context: None,
    };
    assert_eq!(pick_of(None, &ids), ContextPick::FollowActive);
    assert_eq!(
        pick_of(Some(&pinned("coding")), &ids),
        ContextPick::Pinned("coding".into())
    );
    assert_eq!(pick_of(Some(&pinned("gone")), &ids), ContextPick::Nothing);
    let nothing = BackendContext {
        mode: ContextMode::Nothing,
        ..BackendContext::default()
    };
    assert_eq!(pick_of(Some(&nothing), &ids), ContextPick::Nothing);
}
