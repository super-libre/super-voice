// SPDX-License-Identifier: GPL-3.0-only
//! A model's language, on its Models card: what it resolves to, the languages
//! it can be set to, and setting or clearing its own.
//!
//! Two answers from two endpoints, held apart: what is set changes with every
//! pick, what may be set never changes for a model. Both are kept by the model
//! they describe ([`ModelLanguages`]), so an answer for one model is never
//! read as another's.
//!
//! An app that shows one model a stage keeps a [`Focus`], and answers for a
//! model no longer focused are dropped. The app shows two in one
//! stage at times (the loaded model and a staged one), so it keeps none.

use crate::daemon::client::v1::pipeline::language as client;
use crate::languages;
use crate::state::Stage;
use crate::state::language::{LanguageResolution, ModelLanguages};

/// One model, addressed through the stage it runs in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelKey {
    pub stage: Stage,
    pub source: String,
    pub model: String,
}

impl ModelKey {
    #[must_use]
    pub fn new(stage: Stage, source: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            stage,
            source: source.into(),
            model: model.into(),
        }
    }

    /// Whether this names no model: an idle stage reports "nothing loaded" as
    /// an empty pair rather than an absent one.
    fn is_empty(&self) -> bool {
        self.source.is_empty() || self.model.is_empty()
    }
}

/// The model each stage shows, for an app that shows one a stage.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    shown: Vec<ModelKey>,
}

impl Focus {
    /// The model `stage` shows.
    #[must_use]
    pub fn get(&self, stage: Stage) -> Option<&ModelKey> {
        self.shown.iter().find(|key| key.stage == stage)
    }

    /// Every model shown, one a stage.
    pub fn keys(&self) -> impl Iterator<Item = &ModelKey> {
        self.shown.iter()
    }

    fn set(&mut self, key: ModelKey) {
        self.shown.retain(|shown| shown.stage != key.stage);
        if !key.is_empty() {
            self.shown.push(key);
        }
    }

    fn holds(&self, key: &ModelKey) -> bool {
        self.get(key.stage) == Some(key)
    }
}

/// What happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A card shows this model: read its language and what it can be set to.
    Focus(ModelKey),
    /// The daemon said how the model's language resolves.
    Loaded(ModelKey, LanguageResolution),
    /// The daemon said what the model can be set to.
    Offered(ModelKey, Vec<String>),
    /// That couldn't be read. The picker offers nothing rather than languages
    /// the model would refuse.
    OfferUnread(ModelKey),
    /// The user picked a language for the model (`None`: follow the global
    /// one).
    Picked(ModelKey, Option<String>),
    /// The daemon stored it, answering with how it now resolves.
    Saved(ModelKey, LanguageResolution),
    /// Reading or storing it failed: why.
    Failed(ModelKey, String),
    /// The primary language changed: a model that follows it reads
    /// differently now.
    PrimaryChanged,
}

/// What the app should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// [`run_read`].
    Read(ModelKey),
    /// [`run_read_offer`].
    ReadOffer(ModelKey),
    /// [`run_save`]: a tag, or `None` to clear the model's own.
    Save(ModelKey, Option<String>),
}

/// Something to tell the user. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    CouldNotSaveLanguage(String),
}

/// What an input came to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// Take the language's notice down.
    pub clear_notice: bool,
    pub notice: Option<Notice>,
}

impl Outcome {
    fn effects(effects: Vec<Effect>) -> Self {
        Self {
            effects,
            ..Self::default()
        }
    }
}

/// Take in `input`, and say what to do. `focus` is `None` for an app that
/// keeps every answer.
pub fn update(languages: &mut ModelLanguages, focus: Option<&mut Focus>, input: Input) -> Outcome {
    let focused = |key: &ModelKey| focus.as_deref().is_none_or(|focus| focus.holds(key));
    match input {
        Input::Focus(key) => {
            let effects = if key.is_empty() {
                Vec::new()
            } else {
                vec![Effect::Read(key.clone()), Effect::ReadOffer(key.clone())]
            };
            if let Some(focus) = focus {
                focus.set(key);
            }
            Outcome::effects(effects)
        }
        Input::Loaded(key, block) => {
            if focused(&key) {
                languages.record(key.stage, key.source, key.model, block);
            }
            Outcome::default()
        }
        Input::Saved(key, block) => {
            if !focused(&key) {
                return Outcome::default();
            }
            languages.record(key.stage, key.source, key.model, block);
            Outcome {
                clear_notice: true,
                ..Outcome::default()
            }
        }
        Input::Offered(key, tags) => {
            if focused(&key) {
                languages.record_offer(key.stage, key.source, key.model, tags);
            }
            Outcome::default()
        }
        Input::OfferUnread(_) => Outcome::default(),
        Input::Picked(key, tag) => Outcome::effects(vec![Effect::Save(key, tag)]),
        Input::Failed(key, e) => {
            if !focused(&key) {
                return Outcome::default();
            }
            Outcome {
                notice: Some(Notice::CouldNotSaveLanguage(e)),
                ..Outcome::default()
            }
        }
        // Every model shown, or for an app that keeps every answer, every
        // model answered for: a block that follows the primary language goes
        // stale on whichever card shows it.
        Input::PrimaryChanged => {
            let keys: Vec<ModelKey> = match focus {
                Some(focus) => focus.keys().cloned().collect(),
                None => languages
                    .pairs()
                    .map(|(stage, source, model)| ModelKey::new(stage, source, model))
                    .collect(),
            };
            Outcome::effects(keys.into_iter().map(Effect::Read).collect())
        }
    }
}

/// How a model's language resolves, for its button's label. The app words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The model's own: this tag.
    Override(String),
    /// Following the primary language, which is this tag.
    Global(String),
    /// Neither set: the model's default, this tag.
    Default(String),
}

/// How `block` resolves.
#[must_use]
pub fn resolved(block: &LanguageResolution) -> Resolved {
    match (block.effective.as_deref(), block.source.as_str()) {
        (Some(tag), "override") => Resolved::Override(tag.to_string()),
        (Some(tag), "global") => Resolved::Global(tag.to_string()),
        _ => Resolved::Default(block.primary.clone()),
    }
}

/// What a model's language button shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Button {
    /// Nothing known yet: a neutral label, and a picker to open.
    Unread,
    /// The model can be set: how it resolves now, and a picker to open.
    Pickable(Resolved),
    /// The model speaks one language, this tag: shown, with nothing to pick.
    Fixed(String),
}

/// The button for `key`, or `None` while nothing is known about its language,
/// and for a model with one language that doesn't say which.
///
/// An offer of nothing is a model with one language; one of some is a model
/// that can be set. Before the offer lands, a resolution alone reads as one
/// that can be set, as the app's button does.
#[must_use]
pub fn button(languages: &ModelLanguages, key: &ModelKey) -> Option<Button> {
    let block = languages.get(key.stage, &key.source, &key.model);
    match (languages.offered(key.stage, &key.source, &key.model), block) {
        (Some([]), block) => block
            .map(|b| b.primary.clone())
            .filter(|tag| !tag.is_empty())
            .map(Button::Fixed),
        (Some(_), None) => Some(Button::Unread),
        (None, None) => None,
        (_, Some(block)) => Some(Button::Pickable(resolved(block))),
    }
}

/// The tag the model's own language is set to, or `None` while it follows
/// the primary language: the row a picker marks.
#[must_use]
pub fn chosen(languages: &ModelLanguages, key: &ModelKey) -> Option<String> {
    languages
        .get(key.stage, &key.source, &key.model)
        .and_then(|block| match resolved(block) {
            Resolved::Override(tag) => Some(tag),
            _ => None,
        })
}

/// One row of a model's language picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Follow the primary language: clears the model's own.
    FollowGlobal,
    /// A language the model can be set to: its tag and name.
    Language(String, String),
}

/// A model's language picker for `query`: Follow global, then Auto-detect when
/// the model offers it, then its other languages by name. A search never hides
/// the first two.
#[must_use]
pub fn picker_rows(offers: &[String], query: &str) -> Vec<Row> {
    let auto = offers
        .iter()
        .find(|tag| tag.eq_ignore_ascii_case("auto"))
        .map(|tag| Row::Language(tag.clone(), languages::friendly_name(tag)));
    let rest: Vec<String> = offers
        .iter()
        .filter(|tag| !tag.eq_ignore_ascii_case("auto"))
        .cloned()
        .collect();
    std::iter::once(Row::FollowGlobal)
        .chain(auto)
        .chain(
            languages::picker_rows(&rest, query)
                .into_iter()
                .map(|(tag, name)| Row::Language(tag, name)),
        )
        .collect()
}

/// Read how the model's language resolves.
pub async fn run_read(key: ModelKey) -> Input {
    match client::get_model_language(key.stage, key.model.clone()).await {
        Ok(block) => Input::Loaded(key, block),
        Err(e) => Input::Failed(key, e.to_string()),
    }
}

/// Read what the model can be set to.
pub async fn run_read_offer(key: ModelKey) -> Input {
    match client::list_model_languages(key.stage, key.model.clone()).await {
        Ok(tags) => Input::Offered(key, tags),
        Err(e) => {
            log::warn!("Could not read the languages for {}: {e}", key.model);
            Input::OfferUnread(key)
        }
    }
}

/// Set the model's language to `tag`, or clear it with `None`.
pub async fn run_save(key: ModelKey, tag: Option<String>) -> Input {
    let result = match tag {
        Some(tag) => client::set_model_language(key.stage, key.model.clone(), tag).await,
        None => client::clear_model_language(key.stage, key.model.clone()).await,
    };
    match result {
        Ok(block) => Input::Saved(key, block),
        Err(e) => Input::Failed(key, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(model: &str) -> ModelKey {
        ModelKey::new(Stage::Transcription, "src/a", model)
    }

    fn block(effective: Option<&str>, source: &str, primary: &str) -> LanguageResolution {
        LanguageResolution {
            effective: effective.map(String::from),
            source: source.into(),
            primary: primary.into(),
        }
    }

    #[test]
    fn focusing_a_model_reads_its_language_and_offer() {
        let mut languages = ModelLanguages::default();
        let mut focus = Focus::default();
        let outcome = update(
            &mut languages,
            Some(&mut focus),
            Input::Focus(key("whisper")),
        );
        assert_eq!(
            outcome.effects,
            [
                Effect::Read(key("whisper")),
                Effect::ReadOffer(key("whisper"))
            ]
        );
        assert_eq!(focus.get(Stage::Transcription), Some(&key("whisper")));
    }

    /// An idle stage reports an empty pair, which the daemon would refuse.
    #[test]
    fn an_empty_pair_reads_nothing_and_clears_the_focus() {
        let mut languages = ModelLanguages::default();
        let mut focus = Focus::default();
        update(
            &mut languages,
            Some(&mut focus),
            Input::Focus(key("whisper")),
        );
        let outcome = update(&mut languages, Some(&mut focus), Input::Focus(key("")));
        assert!(outcome.effects.is_empty());
        assert_eq!(focus.get(Stage::Transcription), None);
    }

    #[test]
    fn answers_for_a_model_no_longer_focused_are_dropped() {
        let mut languages = ModelLanguages::default();
        let mut focus = Focus::default();
        update(&mut languages, Some(&mut focus), Input::Focus(key("old")));
        update(&mut languages, Some(&mut focus), Input::Focus(key("new")));

        update(
            &mut languages,
            Some(&mut focus),
            Input::Loaded(key("old"), block(Some("es"), "override", "en")),
        );
        update(
            &mut languages,
            Some(&mut focus),
            Input::Offered(key("old"), vec!["es".into()]),
        );
        let failed = update(
            &mut languages,
            Some(&mut focus),
            Input::Failed(key("old"), "down".into()),
        );
        assert!(
            languages
                .get(Stage::Transcription, "src/a", "old")
                .is_none()
        );
        assert!(
            languages
                .offered(Stage::Transcription, "src/a", "old")
                .is_none()
        );
        assert_eq!(failed.notice, None);

        update(
            &mut languages,
            Some(&mut focus),
            Input::Loaded(key("new"), block(Some("fr"), "override", "en")),
        );
        assert!(
            languages
                .get(Stage::Transcription, "src/a", "new")
                .is_some()
        );
    }

    /// Each stage has a focus of its own.
    #[test]
    fn focusing_one_stage_leaves_the_other() {
        let mut languages = ModelLanguages::default();
        let mut focus = Focus::default();
        let post = ModelKey::new(Stage::PostProcessing, "src/b", "s1-mini");
        update(&mut languages, Some(&mut focus), Input::Focus(post.clone()));
        update(
            &mut languages,
            Some(&mut focus),
            Input::Focus(key("whisper")),
        );
        update(
            &mut languages,
            Some(&mut focus),
            Input::Loaded(post.clone(), block(None, "default", "en")),
        );
        assert!(
            languages
                .get(Stage::PostProcessing, "src/b", "s1-mini")
                .is_some()
        );
    }

    /// Without a focus every answer is kept, each under its own model.
    #[test]
    fn without_a_focus_every_answer_is_kept() {
        let mut languages = ModelLanguages::default();
        update(
            &mut languages,
            None,
            Input::Loaded(key("loaded"), block(Some("es"), "override", "en")),
        );
        update(
            &mut languages,
            None,
            Input::Loaded(key("staged"), block(Some("de"), "override", "en")),
        );
        assert_eq!(languages.pairs().count(), 2);
    }

    #[test]
    fn a_pick_is_stored_and_a_failure_told() {
        let mut languages = ModelLanguages::default();
        let picked = update(
            &mut languages,
            None,
            Input::Picked(key("whisper"), Some("fr".into())),
        );
        assert_eq!(
            picked.effects,
            [Effect::Save(key("whisper"), Some("fr".into()))]
        );
        let failed = update(
            &mut languages,
            None,
            Input::Failed(key("whisper"), "down".into()),
        );
        assert_eq!(
            failed.notice,
            Some(Notice::CouldNotSaveLanguage("down".into()))
        );
        let saved = update(
            &mut languages,
            None,
            Input::Saved(key("whisper"), block(Some("fr"), "override", "en")),
        );
        assert!(saved.clear_notice);
        assert_eq!(chosen(&languages, &key("whisper")).as_deref(), Some("fr"));
    }

    #[test]
    fn a_primary_change_reads_again_what_is_shown() {
        let mut languages = ModelLanguages::default();
        let mut focus = Focus::default();
        update(
            &mut languages,
            Some(&mut focus),
            Input::Focus(key("whisper")),
        );
        let outcome = update(&mut languages, Some(&mut focus), Input::PrimaryChanged);
        assert_eq!(outcome.effects, [Effect::Read(key("whisper"))]);

        let mut kept = ModelLanguages::default();
        update(
            &mut kept,
            None,
            Input::Loaded(key("a"), block(None, "default", "en")),
        );
        update(
            &mut kept,
            None,
            Input::Loaded(key("b"), block(None, "default", "en")),
        );
        let outcome = update(&mut kept, None, Input::PrimaryChanged);
        assert_eq!(
            outcome.effects,
            [Effect::Read(key("a")), Effect::Read(key("b"))]
        );
    }

    #[test]
    fn a_resolution_names_where_it_came_from() {
        assert_eq!(
            resolved(&block(Some("fr"), "override", "en")),
            Resolved::Override("fr".into())
        );
        assert_eq!(
            resolved(&block(Some("de"), "global", "en")),
            Resolved::Global("de".into())
        );
        assert_eq!(
            resolved(&block(None, "", "en")),
            Resolved::Default("en".into())
        );
    }

    #[test]
    fn the_button_follows_the_offer() {
        let mut languages = ModelLanguages::default();
        let k = key("whisper");
        assert_eq!(button(&languages, &k), None);

        languages.record(
            k.stage,
            k.source.clone(),
            k.model.clone(),
            block(Some("de"), "global", "en"),
        );
        assert_eq!(
            button(&languages, &k),
            Some(Button::Pickable(Resolved::Global("de".into())))
        );

        // One language: shown, nothing to pick.
        languages.record_offer(k.stage, k.source.clone(), k.model.clone(), Vec::new());
        assert_eq!(button(&languages, &k), Some(Button::Fixed("en".into())));

        let other = key("other");
        languages.record_offer(
            other.stage,
            other.source.clone(),
            other.model.clone(),
            vec!["en".into()],
        );
        assert_eq!(button(&languages, &other), Some(Button::Unread));
    }

    #[test]
    fn follow_global_and_auto_detect_come_first() {
        let offers = ["fr", "auto", "en", "ar"].map(String::from);
        let rows = picker_rows(&offers, "");
        assert_eq!(rows[0], Row::FollowGlobal);
        assert_eq!(rows[1], Row::Language("auto".into(), "Auto-detect".into()));
        assert_eq!(rows[2], Row::Language("ar".into(), "Arabic".into()));
        assert_eq!(rows.len(), 5);

        // A search never hides them.
        let rows = picker_rows(&offers, "fre");
        assert_eq!(
            rows,
            [
                Row::FollowGlobal,
                Row::Language("auto".into(), "Auto-detect".into()),
                Row::Language("fr".into(), "French".into()),
            ]
        );

        // A model that can't detect has no Auto-detect row.
        assert_eq!(
            picker_rows(&["en".to_string()], ""),
            [
                Row::FollowGlobal,
                Row::Language("en".into(), "English".into())
            ]
        );
    }
}
