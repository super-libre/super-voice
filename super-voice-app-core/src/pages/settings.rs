// SPDX-License-Identifier: GPL-3.0-only
//! The Settings page: what both features share. The primary language, how
//! failures are reported, and how long an unused model stays in memory.
//!
//! The failure notices and Sleep When Idle change once the daemon has stored
//! them, so a failed save leaves the page on the value in force. The primary
//! language shows the pick at once, as the settings app always has.

use super_voice_shared::models::notification_method::NotificationMethod;

use crate::daemon::client::v1::settings::language;
use crate::daemon::client::{
    get_idle_unload, get_notification_method, set_idle_unload, set_notification_method,
};
use crate::state::language::LanguageState;
use crate::state::settings::SettingsState;

/// The idle times offered, in seconds. The first is what turning Sleep When
/// Idle on picks.
pub const IDLE_UNLOAD_CHOICES: [u64; 5] = [300, 60, 900, 1800, 3600];

/// The failure notices offered, in the order the page lists them.
pub const NOTIFICATION_METHODS: [NotificationMethod; 4] = [
    NotificationMethod::Auto,
    NotificationMethod::Desktop,
    NotificationMethod::Typed,
    NotificationMethod::Off,
];

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// The daemon said how failures are reported.
    NotificationMethodLoaded(NotificationMethod),
    /// The user picked how failures are reported.
    NotificationMethodPicked(NotificationMethod),
    /// The daemon stored it, or why not.
    NotificationMethodSaved(Result<NotificationMethod, String>),
    /// The daemon said how long an unused model stays in memory.
    IdleUnloadLoaded(u64),
    /// Sleep When Idle was switched.
    SleepToggled(bool),
    /// The user picked how long.
    IdleUnloadPicked(u64),
    /// The daemon stored it, or why not.
    IdleUnloadSaved(Result<u64, String>),
    /// The daemon said what the primary language is (`None`: unset).
    PrimaryLanguageLoaded(Option<String>),
    /// It couldn't be read: why.
    PrimaryLanguageUnread(String),
    /// The tags the primary language can be.
    LanguagesLoaded(Vec<String>),
    /// The user picked a primary language (`None`: unset it).
    PrimaryLanguagePicked(Option<String>),
    /// The daemon stored it, or why not.
    PrimaryLanguageSaved(Result<(), String>),
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Store it: [`run_save_notification_method`].
    SaveNotificationMethod(NotificationMethod),
    /// Store it: [`run_save_idle_unload`].
    SaveIdleUnload(u64),
    /// Store it: [`run_save_primary_language`].
    SavePrimaryLanguage(Option<String>),
    /// The primary language changed: read again what follows it, such as a
    /// model whose language is the primary one.
    LanguageChanged,
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotSaveNotificationMethod(String),
    CouldNotSaveIdleUnload(String),
    CouldNotSaveLanguage(String),
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the page's notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn effect(effect: Effect) -> Self {
        Self {
            effects: vec![effect],
            ..Self::default()
        }
    }

    fn cleared() -> Self {
        Self {
            clear_notice: true,
            ..Self::default()
        }
    }

    fn notice(notice: Notice) -> Self {
        Self {
            notice: Some(notice),
            ..Self::default()
        }
    }
}

/// Take in `input`, and say what to do.
pub fn update(state: &mut SettingsState, language: &mut LanguageState, input: Input) -> Outcome {
    match input {
        Input::NotificationMethodLoaded(method) | Input::NotificationMethodSaved(Ok(method)) => {
            state.notification_method = method;
            Outcome::cleared()
        }
        Input::NotificationMethodPicked(method) => {
            Outcome::effect(Effect::SaveNotificationMethod(method))
        }
        Input::NotificationMethodSaved(Err(e)) => {
            Outcome::notice(Notice::CouldNotSaveNotificationMethod(e))
        }
        Input::IdleUnloadLoaded(after_secs) | Input::IdleUnloadSaved(Ok(after_secs)) => {
            state.idle_unload_secs = after_secs;
            Outcome::cleared()
        }
        Input::SleepToggled(on) => Outcome::effect(Effect::SaveIdleUnload(if on {
            IDLE_UNLOAD_CHOICES[0]
        } else {
            0
        })),
        Input::IdleUnloadPicked(after_secs) => Outcome::effect(Effect::SaveIdleUnload(after_secs)),
        Input::IdleUnloadSaved(Err(e)) => Outcome::notice(Notice::CouldNotSaveIdleUnload(e)),
        Input::PrimaryLanguageLoaded(tag) => {
            language.primary_language = tag;
            Outcome::default()
        }
        Input::LanguagesLoaded(tags) => {
            language.global_offers = tags;
            Outcome::default()
        }
        Input::PrimaryLanguagePicked(tag) => {
            language.primary_language.clone_from(&tag);
            Outcome::effect(Effect::SavePrimaryLanguage(tag))
        }
        Input::PrimaryLanguageSaved(Ok(())) => Outcome {
            effects: vec![Effect::LanguageChanged],
            clear_notice: true,
            notice: None,
        },
        // A failed read is told as the language failing, as the settings app
        // always has: either way the picker can't be trusted.
        Input::PrimaryLanguageSaved(Err(e)) | Input::PrimaryLanguageUnread(e) => {
            Outcome::notice(Notice::CouldNotSaveLanguage(e))
        }
    }
}

/// The idle times to offer while Sleep When Idle is on, shortest first. A time
/// set outside the app is listed too, so the choice in force is always one of
/// them.
#[must_use]
pub fn idle_unload_choices(current: u64) -> Vec<u64> {
    let mut choices = IDLE_UNLOAD_CHOICES.to_vec();
    if current > 0 && !choices.contains(&current) {
        choices.push(current);
    }
    choices.sort_unstable();
    choices
}

/// An idle time as people read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleTime {
    Minutes(u64),
    Seconds(u64),
}

/// `after_secs` in whole minutes when it is some, else in seconds.
#[must_use]
pub const fn idle_time(after_secs: u64) -> IdleTime {
    if after_secs.is_multiple_of(60) {
        IdleTime::Minutes(after_secs / 60)
    } else {
        IdleTime::Seconds(after_secs)
    }
}

/// Read how failures are reported. A daemon that can't say, or names a way
/// this app doesn't know, reads as the default.
pub async fn run_load_notification_method() -> Input {
    let method = match get_notification_method().await {
        Ok(name) => name.parse().unwrap_or_default(),
        Err(e) => {
            log::warn!("Failed to load the notification method: {e}");
            NotificationMethod::default()
        }
    };
    Input::NotificationMethodLoaded(method)
}

/// Read how long an unused model stays in memory. A daemon that can't say
/// reads as keeping it.
pub async fn run_load_idle_unload() -> Input {
    Input::IdleUnloadLoaded(get_idle_unload().await.unwrap_or_else(|e| {
        log::warn!("Failed to load the idle unload setting: {e}");
        0
    }))
}

/// Read the primary language.
pub async fn run_load_primary_language() -> Input {
    match language::get_primary_language().await {
        Ok(tag) => Input::PrimaryLanguageLoaded(tag),
        Err(e) => Input::PrimaryLanguageUnread(e.to_string()),
    }
}

/// Read the tags the primary language can be. A failed read offers nothing,
/// rather than a list of the app's own the daemon may refuse.
pub async fn run_load_languages() -> Input {
    Input::LanguagesLoaded(
        language::list_primary_languages()
            .await
            .unwrap_or_else(|e| {
                log::warn!("Could not read the global languages: {e}");
                Vec::new()
            }),
    )
}

pub async fn run_save_notification_method(method: NotificationMethod) -> Input {
    Input::NotificationMethodSaved(
        set_notification_method(method.to_string())
            .await
            .map(|()| method)
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_save_idle_unload(after_secs: u64) -> Input {
    Input::IdleUnloadSaved(
        set_idle_unload(after_secs)
            .await
            .map(|()| after_secs)
            .map_err(|e| e.to_string()),
    )
}

pub async fn run_save_primary_language(tag: Option<String>) -> Input {
    let result = match tag {
        Some(tag) => language::set_primary_language(tag).await,
        None => language::clear_primary_language().await,
    };
    Input::PrimaryLanguageSaved(result.map_err(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(state: &mut SettingsState, input: Input) -> Outcome {
        update(state, &mut LanguageState::default(), input)
    }

    /// A failed save leaves the page on the value in force.
    #[test]
    fn the_notification_method_changes_once_stored() {
        let mut state = SettingsState::default();
        let outcome = run(
            &mut state,
            Input::NotificationMethodPicked(NotificationMethod::Off),
        );
        assert_eq!(
            outcome.effects,
            [Effect::SaveNotificationMethod(NotificationMethod::Off)]
        );
        assert_eq!(state.notification_method, NotificationMethod::Auto);

        let failed = run(
            &mut state,
            Input::NotificationMethodSaved(Err("down".into())),
        );
        assert_eq!(
            failed.notice,
            Some(Notice::CouldNotSaveNotificationMethod("down".into()))
        );
        assert_eq!(state.notification_method, NotificationMethod::Auto);

        let saved = run(
            &mut state,
            Input::NotificationMethodSaved(Ok(NotificationMethod::Off)),
        );
        assert!(saved.clear_notice);
        assert_eq!(state.notification_method, NotificationMethod::Off);
    }

    #[test]
    fn sleep_when_idle_turns_on_at_the_first_choice_and_off_at_zero() {
        let mut state = SettingsState::default();
        assert_eq!(
            run(&mut state, Input::SleepToggled(true)).effects,
            [Effect::SaveIdleUnload(300)]
        );
        assert_eq!(
            run(&mut state, Input::SleepToggled(false)).effects,
            [Effect::SaveIdleUnload(0)]
        );
    }

    #[test]
    fn a_time_set_elsewhere_is_offered_in_order() {
        assert_eq!(idle_unload_choices(300), [60, 300, 900, 1800, 3600]);
        assert_eq!(idle_unload_choices(120), [60, 120, 300, 900, 1800, 3600]);
    }

    #[test]
    fn idle_times_read_in_minutes_when_whole() {
        assert_eq!(idle_time(900), IdleTime::Minutes(15));
        assert_eq!(idle_time(90), IdleTime::Seconds(90));
    }

    /// The pick shows at once; what follows the language is read again once
    /// it is stored.
    #[test]
    fn a_primary_language_shows_at_once_and_is_followed_once_stored() {
        let mut state = SettingsState::default();
        let mut language = LanguageState::default();
        let picked = update(
            &mut state,
            &mut language,
            Input::PrimaryLanguagePicked(Some("fr".into())),
        );
        assert_eq!(language.primary_language.as_deref(), Some("fr"));
        assert_eq!(
            picked.effects,
            [Effect::SavePrimaryLanguage(Some("fr".into()))]
        );
        let saved = update(
            &mut state,
            &mut language,
            Input::PrimaryLanguageSaved(Ok(())),
        );
        assert_eq!(saved.effects, [Effect::LanguageChanged]);
    }
}
