// SPDX-License-Identifier: GPL-3.0-only
//! The app's pages, as the sidebar lists them: each feature's own under its
//! heading, then what the app has once.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{Icon, IconName};

/// A heading in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    SpeechToText,
    TextToSpeech,
    General,
}

impl Section {
    pub const ALL: [Self; 3] = [Self::SpeechToText, Self::TextToSpeech, Self::General];

    /// The heading's message key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SpeechToText => "speech-to-text",
            Self::TextToSpeech => "text-to-speech",
            Self::General => "general",
        }
    }

    /// The section's pages, in the order the sidebar lists them.
    pub fn pages(self) -> impl Iterator<Item = Page> {
        Page::ALL
            .into_iter()
            .filter(move |page| page.section() == self)
    }
}

/// One page of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    SttModels,
    Notes,
    Contexts,
    SttBehavior,
    TtsModels,
    Clips,
    Voices,
    TtsBehavior,
    Library,
    Shortcuts,
    Assistants,
    Settings,
    Updates,
    About,
}

impl Page {
    pub const ALL: [Self; 14] = [
        Self::SttModels,
        Self::Notes,
        Self::Contexts,
        Self::SttBehavior,
        Self::TtsModels,
        Self::Clips,
        Self::Voices,
        Self::TtsBehavior,
        Self::Library,
        Self::Shortcuts,
        Self::Assistants,
        Self::Settings,
        Self::Updates,
        Self::About,
    ];

    /// The heading the page is listed under.
    #[must_use]
    pub const fn section(self) -> Section {
        match self {
            Self::SttModels | Self::Notes | Self::Contexts | Self::SttBehavior => {
                Section::SpeechToText
            }
            Self::TtsModels | Self::Clips | Self::Voices | Self::TtsBehavior => {
                Section::TextToSpeech
            }
            Self::Library
            | Self::Shortcuts
            | Self::Assistants
            | Self::Settings
            | Self::Updates
            | Self::About => Section::General,
        }
    }

    /// The page's message key: its name in the sidebar and its title.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SttModels | Self::TtsModels => "models",
            Self::Notes => "notes",
            Self::Contexts => "contexts",
            Self::SttBehavior | Self::TtsBehavior => "behavior",
            Self::Clips => "clips",
            Self::Voices => "voices",
            Self::Library => "library",
            Self::Shortcuts => "shortcuts",
            Self::Assistants => "assistants",
            Self::Settings => "settings",
            Self::Updates => "updates",
            Self::About => "about",
        }
    }

    /// A name for the page that stays the same in every language, for
    /// `just app-shot` and the element ids.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::SttModels => "stt-models",
            Self::Notes => "notes",
            Self::Contexts => "contexts",
            Self::SttBehavior => "stt-behavior",
            Self::TtsModels => "tts-models",
            Self::Clips => "clips",
            Self::Voices => "voices",
            Self::TtsBehavior => "tts-behavior",
            Self::Library => "library",
            Self::Shortcuts => "shortcuts",
            Self::Assistants => "assistants",
            Self::Settings => "settings",
            Self::Updates => "updates",
            Self::About => "about",
        }
    }

    /// The page named `slug`, as [`Self::slug`] gives it.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.slug() == slug)
    }

    /// The page's icon in the sidebar.
    #[must_use]
    pub fn icon(self) -> Icon {
        match self {
            Self::SttModels | Self::TtsModels => Lucide::Box.into(),
            Self::Notes => Lucide::NotebookPen.into(),
            Self::Contexts => IconName::BookOpen.into(),
            Self::SttBehavior | Self::TtsBehavior => Lucide::SlidersHorizontal.into(),
            Self::Clips => Lucide::AudioLines.into(),
            Self::Voices => Lucide::UserRoundPen.into(),
            Self::Library => Lucide::Library.into(),
            Self::Shortcuts => Lucide::Keyboard.into(),
            Self::Assistants => IconName::Bot.into(),
            Self::Settings => IconName::Settings.into(),
            Self::Updates => IconName::RefreshCw.into(),
            Self::About => IconName::Info.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_under_one_heading_and_named_once() {
        let listed: Vec<Page> = Section::ALL.into_iter().flat_map(Section::pages).collect();
        assert_eq!(listed, Page::ALL);
        for page in Page::ALL {
            assert_eq!(Page::from_slug(page.slug()), Some(page));
        }
    }

    /// Every word the shell shows is in the English catalog: a missing key
    /// would show as the key itself.
    #[test]
    fn every_heading_and_page_has_its_words() {
        let keys = Section::ALL
            .into_iter()
            .map(Section::key)
            .chain(Page::ALL.into_iter().map(Page::key))
            .chain(crate::shell::KEYS.iter().copied());
        for key in keys {
            assert!(
                super_voice_app_core::i18n::LANGUAGE_LOADER.has(key),
                "no message {key}"
            );
        }
    }
}
