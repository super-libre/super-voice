// SPDX-License-Identifier: GPL-3.0-only

//! Data models and types for the Super Voice application.

// Re-export AudioTheme from shared crate
pub use super_voice_shared::models::theme::AudioTheme;

/// Daemon connection status
#[derive(Debug, Clone, Default, PartialEq)]
pub enum DaemonStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error(String),
    /// User denied the settings-scope consent prompt (either just now
    /// or via the daemon's sticky deny cache). All settings-scope
    /// operations will fail until the user explicitly retries
    /// authorization — usually after `systemctl --user restart
    /// super-stt`. The auto-retry loop is suppressed in this state to
    /// avoid spamming the daemon's deny cache.
    Blocked(String),
}

/// One page of the application: a nav entry's own, or a tab of a feature's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Customization,
    /// Notes the user speaks, kept by the app: the list, one open in its
    /// place, and recording a new one.
    Notes,
    /// Speech the user makes from text and keeps: the next clip's text, the
    /// clips, and playing, exporting and deleting them.
    Clips,
    /// Speech to text's settings, and one section that tests them: when a
    /// recording stops, preview typing, how text is typed.
    SpeechToTextBehavior,
    /// The active transcription backend: its model picker, load/unload, and a
    /// side sheet for switching which installed backend is active.
    Models,
    /// Manage installed backends and browse installable ones (the old Models
    /// page's Installed / Browse tabs). No activation here — that lives on the
    /// Models page.
    Library,
    /// Self-update: current/latest version, automatic-check and beta-opt-in
    /// settings, and the apply flow.
    Updates,
    /// Named dictation contexts: what the user is dictating, as a prompt and a
    /// vocabulary, and which one is in force.
    Contexts,
    /// The cloned-voice library: record or import a reference clip, and manage
    /// the voices built from them.
    Voices,
    /// Text to speech's settings, and one section that tests them: where the
    /// read-aloud shortcut reads from, a test utterance, a read test.
    TextToSpeechBehavior,
    /// Super Voice's keyboard shortcuts, and adding each one that is missing.
    Shortcuts,
    /// The AI assistants Super Voice connects to, and keys for others.
    Assistants,
}

/// An entry in the nav rail: a page of its own, or a feature whose pages are
/// tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Models,
    Library,
    SpeechToText,
    TextToSpeech,
    Shortcuts,
    Assistants,
    Customization,
    Updates,
}

impl Section {
    /// The heading a feature's section shows above its tabs.
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::SpeechToText => crate::fl!("speech-to-text"),
            Self::TextToSpeech => crate::fl!("text-to-speech"),
            Self::Shortcuts => crate::fl!("shortcuts"),
            Self::Assistants => crate::fl!("assistants"),
            Self::Models => crate::fl!("models"),
            Self::Library => crate::fl!("library"),
            Self::Customization => crate::fl!("settings"),
            Self::Updates => crate::fl!("updates"),
        }
    }

    /// The section's tabs, in order, or its one page.
    #[must_use]
    pub const fn pages(self) -> &'static [Page] {
        match self {
            Self::Models => &[Page::Models],
            Self::Library => &[Page::Library],
            // Behavior is each feature's last tab: its settings and tests.
            Self::SpeechToText => &[Page::Notes, Page::Contexts, Page::SpeechToTextBehavior],
            Self::TextToSpeech => &[Page::Clips, Page::Voices, Page::TextToSpeechBehavior],
            Self::Shortcuts => &[Page::Shortcuts],
            Self::Assistants => &[Page::Assistants],
            Self::Customization => &[Page::Customization],
            Self::Updates => &[Page::Updates],
        }
    }

    /// The section a page is in.
    #[must_use]
    pub const fn of(page: Page) -> Self {
        match page {
            Page::Models => Self::Models,
            Page::Library => Self::Library,
            Page::Notes | Page::Contexts | Page::SpeechToTextBehavior => Self::SpeechToText,
            Page::Clips | Page::Voices | Page::TextToSpeechBehavior => Self::TextToSpeech,
            Page::Shortcuts => Self::Shortcuts,
            Page::Assistants => Self::Assistants,
            Page::Customization => Self::Customization,
            Page::Updates => Self::Updates,
        }
    }
}

impl Page {
    /// The page's name: its tab's label, or its nav entry's.
    #[must_use]
    pub fn title(self) -> String {
        match self {
            Self::Models => crate::fl!("models"),
            Self::Library => crate::fl!("library"),
            Self::Notes => crate::fl!("notes"),
            Self::Clips => crate::fl!("clips"),
            Self::Contexts => crate::fl!("contexts"),
            Self::Voices => crate::fl!("voices"),
            Self::SpeechToTextBehavior | Self::TextToSpeechBehavior => crate::fl!("behavior"),
            Self::Shortcuts => crate::fl!("shortcuts"),
            Self::Assistants => crate::fl!("assistants"),
            Self::Customization => crate::fl!("settings"),
            Self::Updates => crate::fl!("updates"),
        }
    }
}

/// The context page to display in the context drawer
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    /// The daemon connection: its status and socket, and how to start a daemon
    /// that is not running. Opened from View → Connection. While the daemon
    /// cannot be reached the window's body shows the same, with the nav
    /// hidden.
    Connection,
    /// Right-side sheet for adding a backend from a Git repo URL or a local
    /// directory. Scoped to the Models page — it closes on navigation away or
    /// daemon disconnect (see `AppModel::context_drawer`).
    AddBackend,
    /// Right-side sheet for editing the active/selected backend's secrets and
    /// options. Reuses the drawer instead of a full-page takeover so the
    /// backend list stays visible behind it. The backend is identified by
    /// `AppModel::configure_backend`; also Models-scoped.
    ConfigureBackend,
    /// Right-side search sheet for picking a transcription language. Scope
    /// (global vs per-model) is carried by `AppModel::language_picker_target`.
    LanguagePicker,
    /// Right-side sheet for choosing which installed backend to activate (the
    /// Models page's "Select transcription backend" flow). Scoped to the
    /// Models page; picking a backend activates it and closes the sheet.
    SelectBackend,
    /// Right-side sheet for choosing which installed backend fills a stage the
    /// app reads back from the daemon: the twin of [`Self::SelectBackend`],
    /// listing only the backends that can fill that stage; also Models-scoped.
    SelectBlockBackend(super::BlockStage),
    /// Right-side sheet for editing one dictation context: its name, its
    /// prompt, and its vocabulary. The context being edited is carried by
    /// `ContextsState::draft`, which is also what makes an open editor survive
    /// a refetch landing under it. Scoped to the Contexts page.
    EditContext,
}

/// Where a transient action error belongs, so a failed save surfaces as an
/// inline banner on the owning surface instead of hijacking the whole UI
/// (Tier 1 #13) or being dropped to the log (Tier 1 #15). One scope-tagged
/// slot ([`ActionError`]) is held on `AppModel`; the audit's "one error
/// surface" (App Tier 3 #11) generalizes this.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ErrorScope {
    /// The Settings page: the Primary Language, failure notices and Sleep
    /// When Idle saves.
    Customization,
    /// The per-backend Configure sheet: secret / option saves.
    ConfigureBackend,
    /// Speech to text's Behavior tab, Recording and Sounds: preview-typing,
    /// stop-mode, sound and volume saves, and the daemon-mic recording result.
    Recording,
    /// The Input Simulation page: write-method save.
    InputSimulation,
    /// The Models page's Post-processing section: post-processor toggle and
    /// model selection saves.
    PostProcessing,
    /// The Models page's Synthesis section: its model and voice saves.
    Synthesis,
    /// The Speech tab: the test utterance.
    Speech,
    /// The Voices tab: recording, upload, rename, and delete failures.
    Voices,
    /// The Read aloud tab: a failed read method save, or a failed read test.
    ReadAloud,
    /// The Shortcuts page: a shortcut that could not be added.
    Shortcuts,
    /// The Assistants page: a failed list fetch, connect, disconnect, or key.
    /// Shown in the page's dialog while one is open.
    Assistants,
    /// The Contexts page: a failed list fetch, activate, or delete. A failed
    /// *save* is not here — it belongs in the editor that caused it, beside the
    /// field the user has to fix.
    Contexts,
    /// The Notes tab: a note that could not be read, recorded, saved,
    /// exported or deleted.
    Notes,
    /// The Clips tab: a clip that could not be read, made, played, exported
    /// or deleted.
    Clips,
}

/// Where a failed save of the stage is reported: on its own card.
impl From<crate::state::BlockStage> for ErrorScope {
    fn from(stage: crate::state::BlockStage) -> Self {
        match stage {
            crate::state::BlockStage::PostProcessing => Self::PostProcessing,
            crate::state::BlockStage::Synthesis => Self::Synthesis,
        }
    }
}

/// A scope-tagged, transient action failure rendered as an inline banner on the
/// page named by [`ErrorScope`]. Set on a failed save, cleared when the user
/// retries that action, reconnects, or closes the owning surface.
#[derive(Clone, Debug)]
pub struct ActionError {
    pub scope: ErrorScope,
    pub message: String,
}

/// Which tab of the Models page is active.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ModelsTab {
    /// Backends already installed and discovered by the daemon.
    #[default]
    Installed,
    /// Official backends the user can install.
    Download,
}

/// Menu actions
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    Connection,
    About,
}

#[cfg(test)]
mod section_tests {
    use super::Section;

    /// Every page is in exactly one section, the one `Section::of` names.
    #[test]
    fn each_page_is_in_the_section_it_names() {
        let sections = [
            Section::Models,
            Section::Library,
            Section::SpeechToText,
            Section::TextToSpeech,
            Section::Shortcuts,
            Section::Assistants,
            Section::Customization,
            Section::Updates,
        ];
        let mut seen = Vec::new();
        for section in sections {
            for &page in section.pages() {
                assert_eq!(Section::of(page), section, "{page:?}");
                assert!(!seen.contains(&page), "{page:?} is in two sections");
                seen.push(page);
            }
        }
        // The same pages everywhere: text to speech's Behavior tab leaves out
        // reading aloud where the session has none, rather than a tab going.
        assert_eq!(seen.len(), 12, "every page is in a section");
    }
}
