// SPDX-License-Identifier: GPL-3.0-only
//! The app's pages, as the sidebar lists them: each feature's own under its
//! heading, then what the app has once.
//!
//! # Adding a page
//!
//! 1. Write the page in its own module here, `pages/<name>.rs`: a view that
//!    implements [`PageView`] (see `about.rs` for a whole one, over its core
//!    page through [`crate::effects::CorePage`]).
//! 2. Add `mod <name>;` below, and one line to [`build`]:
//!    `Page::Notes => PageHandle::new::<notes::NotesPage>(page, shared, window, cx),`
//!
//! Every page is built when the window opens and kept while it is open, so a
//! page keeps its state when another is shown. A page never calls the daemon
//! from `new`: it starts in [`PageView::reached`], which the window calls
//! each time the daemon answers (first, and again after it was down). That
//! keeps `just app-shot`, which never reaches the daemon, from asking it for
//! anything.
//!
//! Only the page on screen, and those whose mark the sidebar shows
//! ([`Page::reads_while_hidden`]), read when the daemon answers; the others
//! read one at a time after them, a moment apart ([`next_to_catch_up`]), or
//! at once if shown before their turn. Fourteen pages asking at once is more
//! than the daemon lets one client ask in a moment, and a page that hasn't
//! read offers the palette none of its commands.

mod about;
mod assistants;
mod clips;
mod contexts;
mod finish_setup;
mod library;
mod models;
mod notes;
mod settings;
mod shortcuts;
mod stt_behavior;
mod take;
mod tts_behavior;
mod updates;
mod voices;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{Icon, IconName};
use gpui_kit::{
    AnyView, App, AppContext as _, Context, Entity, Render, SharedString, Subscription, Window,
};
use super_voice_app_core::events::Event;
use super_voice_app_core::pages::readiness::Feature;

use crate::shared::Shared;
use crate::shell::Shell;

/// Build `page`'s view. One line per page; a page not built yet is a
/// placeholder.
pub fn build(page: Page, shared: &Shared, window: &mut Window, cx: &mut App) -> PageHandle {
    match page {
        Page::About => PageHandle::new::<about::AboutPage>(page, shared, window, cx),
        Page::Shortcuts => PageHandle::new::<shortcuts::ShortcutsPage>(page, shared, window, cx),
        Page::Assistants => PageHandle::new::<assistants::AssistantsPage>(page, shared, window, cx),
        Page::Settings => PageHandle::new::<settings::SettingsPage>(page, shared, window, cx),
        Page::Updates => PageHandle::new::<updates::UpdatesPage>(page, shared, window, cx),
        Page::Notes => PageHandle::new::<notes::NotesPage>(page, shared, window, cx),
        Page::Contexts => PageHandle::new::<contexts::ContextsPage>(page, shared, window, cx),
        Page::SttBehavior => {
            PageHandle::new::<stt_behavior::BehaviorPage>(page, shared, window, cx)
        }
        Page::Clips => PageHandle::new::<clips::ClipsPage>(page, shared, window, cx),
        Page::Voices => PageHandle::new::<voices::VoicesPage>(page, shared, window, cx),
        Page::TtsBehavior => {
            PageHandle::new::<tts_behavior::TtsBehaviorPage>(page, shared, window, cx)
        }
        Page::SttModels | Page::TtsModels => {
            PageHandle::new::<models::ModelsPage>(page, shared, window, cx)
        }
        Page::Library => PageHandle::new::<library::LibraryPage>(page, shared, window, cx),
        Page::FinishSetup => {
            PageHandle::new::<finish_setup::FinishSetupPage>(page, shared, window, cx)
        }
    }
}

/// What a page is to the window. Only `new` and `Render` are required; the
/// rest have defaults that do nothing.
///
/// A page draws its own header ([`crate::ui::page_header::PageHeader`]) and
/// fills the content area, scrolling itself. It reads its width with
/// [`crate::ui::width`].
pub trait PageView: Render + Sized + 'static {
    /// Build the page. `page` is which one it is (a view can serve more than
    /// one); `shared` holds what every page may read or use. No daemon calls
    /// here: see [`Self::reached`].
    fn new(page: Page, shared: &Shared, window: &mut Window, cx: &mut Context<Self>) -> Self;

    /// The daemon answers, first or again after it was down: read what the
    /// page shows from it.
    fn reached(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Something the daemon said. Every page hears every event, whether it
    /// is shown or not.
    fn heard(&mut self, _event: &Event, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// The page is shown, or shown again.
    fn shown(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Alt+Left (Cmd+[ too on macOS), or the back button: go back from a
    /// detail shown alone to its list. `false` when there is nowhere to go
    /// back to.
    fn back(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        false
    }

    /// The window is asked to close (its close button, or the desktop's).
    /// `true` when the page has work in flight it must finish first, such as
    /// a take recording: it finishes, then says so with
    /// [`crate::shared::Navigator::finished_closing`], and the window closes
    /// once every page that waited has. The shell closes it anyway after a
    /// short wait, so a stuck daemon can't keep it open.
    fn closing(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> bool {
        false
    }

    /// The commands the page offers the palette (Ctrl+K), whatever page is
    /// shown. The palette lists them under their group, then shows this page
    /// and calls [`Self::run`] with the id of the one picked.
    fn commands(&self, _cx: &App) -> Vec<PaletteCommand> {
        Vec::new()
    }

    /// Run the command `id`, which [`Self::commands`] offered. The page is
    /// shown by then.
    fn run(&mut self, _id: &str, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// A count or a dot beside the page's name in the sidebar: 3 notes, an
    /// update ready.
    fn sidebar_mark(&self, _cx: &App) -> Option<SidebarMark> {
        None
    }

    /// Made-up state for `just app-shot`, which never reaches the daemon.
    /// `scenes` are the shot's words after its width (`dictating`), for a
    /// page that draws more than one state; a page ignores the ones it
    /// doesn't know.
    #[cfg(feature = "screenshot")]
    fn pretend(&mut self, _scenes: &[&str], _window: &mut Window, _cx: &mut Context<Self>) {}
}

/// The Notes page's command that records a new note, which Ctrl+R (Cmd+R
/// on macOS) runs from anywhere in the window.
pub const NEW_NOTE: &str = "new-note";

/// Speech to text › Models' command that shows the Typed step, where the
/// card says how to let the daemon type: the toast's Allow typing.
pub const ALLOW_TYPING: &str = "allow-typing";

/// A command in the palette.
#[derive(Clone)]
pub struct PaletteCommand {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Icon,
    /// The window's keys that run it too, as a key binding names them
    /// ("ctrl-r"), shown on its row.
    pub keys: Option<&'static str>,
    /// Where it acts, on the right of its row: "Speech to text · Transcription".
    pub detail: Option<SharedString>,
    /// The palette's group it is listed under; `None` is Commands.
    pub group: Option<SharedString>,
    /// More words the search matches.
    pub keywords: Vec<SharedString>,
}

// Some are for the pages built later.
#[allow(dead_code)]
impl PaletteCommand {
    /// `PaletteCommand::new("unload-all", t("unload-all-models"), IconName::Square)`
    #[must_use]
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        icon: impl Into<Icon>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: icon.into(),
            keys: None,
            detail: None,
            group: None,
            keywords: Vec::new(),
        }
    }

    /// The window's keys that run it too: `.keys(shell::NEW_NOTE_KEYS)`.
    #[must_use]
    pub fn keys(mut self, keys: &'static str) -> Self {
        self.keys = Some(keys);
        self
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    #[must_use]
    pub fn keywords(mut self, keywords: impl IntoIterator<Item = SharedString>) -> Self {
        self.keywords.extend(keywords);
        self
    }
}

/// A mark beside a page's name in the sidebar.
// For the pages built later: Notes, Clips, Voices, Updates.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMark {
    /// How many it holds: notes, clips, voices. Hidden at 0.
    Count(usize),
    /// A bare dot in the theme's info colour: an update is ready. Not a
    /// status dot, so not one of their colours.
    Update,
    /// A yellow dot: something needs setting up (Finish setup).
    NeedsSetup,
}

/// A built page, as the window holds it: its view, and its [`PageView`]
/// calls without its type.
#[derive(Clone)]
pub struct PageHandle {
    view: AnyView,
    calls: Rc<dyn PageCalls>,
    /// The daemon answered while the page was hidden: it reads when shown.
    stale: Rc<Cell<bool>>,
}

impl PageHandle {
    /// Build a `T` for `page`.
    pub fn new<T: PageView>(
        page: Page,
        shared: &Shared,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let entity = cx.new(|cx| T::new(page, shared, window, cx));
        Self {
            view: entity.clone().into(),
            calls: Rc::new(entity),
            stale: Rc::default(),
        }
    }

    #[must_use]
    pub fn view(&self) -> AnyView {
        self.view.clone()
    }

    pub fn reached(&self, window: &mut Window, cx: &mut App) {
        self.stale.set(false);
        self.calls.reached(window, cx);
    }

    /// The daemon answered while the page is hidden: it reads when it is
    /// next shown ([`Self::reach_if_stale`]).
    pub fn mark_stale(&self) {
        self.stale.set(true);
    }

    /// Whether the daemon answered while the page was hidden, and it hasn't
    /// read since.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale.get()
    }

    /// Read from the daemon, if it answered while the page was hidden.
    pub fn reach_if_stale(&self, window: &mut Window, cx: &mut App) {
        if self.stale.get() {
            self.reached(window, cx);
        }
    }

    pub fn heard(&self, event: &Event, window: &mut Window, cx: &mut App) {
        self.calls.heard(event, window, cx);
    }

    pub fn shown(&self, window: &mut Window, cx: &mut App) {
        self.calls.shown(window, cx);
    }

    pub fn back(&self, window: &mut Window, cx: &mut App) -> bool {
        self.calls.back(window, cx)
    }

    pub fn closing(&self, window: &mut Window, cx: &mut App) -> bool {
        self.calls.closing(window, cx)
    }

    #[must_use]
    pub fn commands(&self, cx: &App) -> Vec<PaletteCommand> {
        self.calls.commands(cx)
    }

    pub fn run(&self, id: &str, window: &mut Window, cx: &mut App) {
        self.calls.run(id, window, cx);
    }

    #[must_use]
    pub fn sidebar_mark(&self, cx: &App) -> Option<SidebarMark> {
        self.calls.sidebar_mark(cx)
    }

    /// Notify the shell whenever the page changes, for its sidebar mark.
    pub fn observe_by(&self, cx: &mut Context<Shell>) -> Subscription {
        self.calls.observe_by(cx)
    }

    #[cfg(feature = "screenshot")]
    pub fn pretend(&self, scenes: &[&str], window: &mut Window, cx: &mut App) {
        self.calls.pretend(scenes, window, cx);
    }
}

/// [`PageView`]'s calls on an `Entity<T>`, without `T`.
trait PageCalls {
    fn reached(&self, window: &mut Window, cx: &mut App);
    fn heard(&self, event: &Event, window: &mut Window, cx: &mut App);
    fn shown(&self, window: &mut Window, cx: &mut App);
    fn back(&self, window: &mut Window, cx: &mut App) -> bool;
    fn closing(&self, window: &mut Window, cx: &mut App) -> bool;
    fn commands(&self, cx: &App) -> Vec<PaletteCommand>;
    fn run(&self, id: &str, window: &mut Window, cx: &mut App);
    fn sidebar_mark(&self, cx: &App) -> Option<SidebarMark>;
    fn observe_by(&self, cx: &mut Context<Shell>) -> Subscription;
    #[cfg(feature = "screenshot")]
    fn pretend(&self, scenes: &[&str], window: &mut Window, cx: &mut App);
}

impl<T: PageView> PageCalls for Entity<T> {
    fn reached(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.reached(window, cx));
    }

    fn heard(&self, event: &Event, window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.heard(event, window, cx));
    }

    fn shown(&self, window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.shown(window, cx));
    }

    fn back(&self, window: &mut Window, cx: &mut App) -> bool {
        self.update(cx, |page, cx| page.back(window, cx))
    }

    fn closing(&self, window: &mut Window, cx: &mut App) -> bool {
        self.update(cx, |page, cx| page.closing(window, cx))
    }

    fn commands(&self, cx: &App) -> Vec<PaletteCommand> {
        self.read(cx).commands(cx)
    }

    fn run(&self, id: &str, window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.run(id, window, cx));
    }

    fn sidebar_mark(&self, cx: &App) -> Option<SidebarMark> {
        self.read(cx).sidebar_mark(cx)
    }

    fn observe_by(&self, cx: &mut Context<Shell>) -> Subscription {
        cx.observe(self, |_, _, cx| cx.notify())
    }

    #[cfg(feature = "screenshot")]
    fn pretend(&self, scenes: &[&str], window: &mut Window, cx: &mut App) {
        self.update(cx, |page, cx| page.pretend(scenes, window, cx));
    }
}

/// The page to read next among those left behind when the daemon answered:
/// the first still waiting, in the sidebar's order.
pub fn next_to_catch_up(pages: impl IntoIterator<Item = (Page, bool)>) -> Option<Page> {
    pages
        .into_iter()
        .find_map(|(page, stale)| stale.then_some(page))
}

/// A heading in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Finish setup, above the features, with no heading, and only while
    /// something is missing.
    Setup,
    SpeechToText,
    TextToSpeech,
    General,
}

impl Section {
    pub const ALL: [Self; 4] = [
        Self::Setup,
        Self::SpeechToText,
        Self::TextToSpeech,
        Self::General,
    ];

    /// The heading's message key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            // No heading of its own; its page is General's, in a breadcrumb.
            Self::Setup | Self::General => "general",
            Self::SpeechToText => "speech-to-text",
            Self::TextToSpeech => "text-to-speech",
        }
    }

    /// The feature the section is, `None` for General.
    #[must_use]
    pub const fn feature(self) -> Option<Feature> {
        match self {
            Self::SpeechToText => Some(Feature::SpeechToText),
            Self::TextToSpeech => Some(Feature::TextToSpeech),
            Self::Setup | Self::General => None,
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
    /// What macOS has to allow, while something is missing.
    FinishSetup,
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
    /// Whether the page is a preview: it works, but may still change. A
    /// "Preview" tag sits beside its name in the sidebar and its header.
    #[must_use]
    pub const fn is_preview(self) -> bool {
        matches!(self, Self::Assistants)
    }

    pub const ALL: [Self; 15] = [
        Self::FinishSetup,
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
            Self::FinishSetup => Section::Setup,
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
            Self::FinishSetup => "finish-setup",
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
    /// `just app-shot` and the log.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::FinishSetup => "finish-setup",
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
    #[cfg(any(test, feature = "screenshot"))]
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.slug() == slug)
    }

    /// Whether the page reads from the daemon when it answers even while
    /// another page is shown: those whose sidebar mark comes from what they
    /// read (how many notes, clips and voices; an update ready).
    #[must_use]
    pub const fn reads_while_hidden(self) -> bool {
        matches!(
            self,
            Self::Notes | Self::Clips | Self::Voices | Self::Library | Self::Updates
        )
    }

    /// Whether the page needs the daemon. While the daemon is down, these
    /// are dimmed in the sidebar and the Connection page takes their place;
    /// the others still open.
    #[must_use]
    pub const fn needs_daemon(self) -> bool {
        !matches!(
            self,
            Self::Shortcuts | Self::Settings | Self::Updates | Self::About
        )
    }

    /// The feature whose readiness the page's icon carries on the rail: each
    /// feature's Models page.
    #[must_use]
    pub const fn feature(self) -> Option<Feature> {
        match self {
            Self::SttModels => Some(Feature::SpeechToText),
            Self::TtsModels => Some(Feature::TextToSpeech),
            _ => None,
        }
    }

    /// The page's full name, with its feature's: "Text to speech › Models",
    /// where the sidebar's headings don't show. A page of General is its
    /// name alone.
    #[must_use]
    pub fn full_name(self) -> SharedString {
        match self.section() {
            Section::Setup | Section::General => crate::t(self.key()),
            section => format!("{} › {}", crate::t(section.key()), crate::t(self.key())).into(),
        }
    }

    /// The page's icon in the sidebar.
    #[must_use]
    pub fn icon(self) -> Icon {
        match self {
            Self::FinishSetup => Lucide::ListChecks.into(),
            Self::SttModels | Self::TtsModels => Lucide::Brain.into(),
            Self::Notes => Lucide::NotebookPen.into(),
            Self::Contexts => Lucide::BookOpenText.into(),
            Self::SttBehavior | Self::TtsBehavior => Lucide::SlidersHorizontal.into(),
            Self::Clips => Lucide::FileVolume.into(),
            Self::Voices => Lucide::UserRoundPen.into(),
            Self::Library => Lucide::LibraryBig.into(),
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

    /// The pages left behind read one at a time in the sidebar's order, each
    /// once: one already read, shown before its turn, is passed over.
    #[test]
    fn the_pages_left_behind_read_one_at_a_time_in_order() {
        let mut stale: Vec<(Page, bool)> = Page::ALL
            .into_iter()
            .map(|page| (page, !page.reads_while_hidden() && page != Page::SttModels))
            .collect();
        // Shown before its turn: it read then.
        stale
            .iter_mut()
            .filter(|(page, _)| *page == Page::Contexts)
            .for_each(|(_, s)| *s = false);
        let mut read = Vec::new();
        while let Some(page) = next_to_catch_up(stale.iter().copied()) {
            read.push(page);
            stale
                .iter_mut()
                .filter(|(p, _)| *p == page)
                .for_each(|(_, s)| *s = false);
        }
        assert_eq!(
            read,
            [
                Page::FinishSetup,
                Page::SttBehavior,
                Page::TtsModels,
                Page::TtsBehavior,
                Page::Shortcuts,
                Page::Assistants,
                Page::Settings,
                Page::About,
            ]
        );
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
