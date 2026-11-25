// SPDX-License-Identifier: GPL-3.0-only
//! The Contexts page for an app written in another language: a store that
//! holds the page's state, runs the core's logic for it (`pages::contexts`) on
//! the stores' runtime, and tells the app whenever the page changed.
//!
//! The app keeps its own fields in the editor. It takes the editor's name,
//! prompt and rows when the editor opens, and the rows again whenever they
//! change other than by typing in one: a row added, removed, or several pasted
//! at once.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super_voice_app_core::pages::contexts::{self as page, Effect, Input, Notice, Reach};
use super_voice_app_core::state::contexts::{ContextDraft, ContextsState};

use crate::{Buffer, RUNTIME};

/// What the app is told. Called from the store's own threads.
#[uniffi::export(with_foreign)]
pub trait ContextsObserver: Send + Sync {
    /// The page changed; draw this.
    fn changed(&self, page: ContextsPage);
}

/// What the user did on the page.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ContextsAction {
    /// Read the list again: the page was opened.
    Reload,
    /// Put a context in force, or (`None`) none.
    Activate { id: Option<String> },
    /// Delete was pressed once for a context.
    DeleteRequested { id: String },
    /// The delete was called off.
    DeleteCancelled,
    /// Delete was pressed again.
    DeleteConfirmed { id: String },
    /// Open the editor over a context.
    Edit { id: String },
    /// Open the editor over a new context.
    Create,
    /// Close the editor, and drop what was typed.
    CancelEdit,
    /// The name field now holds this.
    NameChanged { name: String },
    /// Vocabulary row `index` now holds this. Several lines become several
    /// rows.
    TermChanged { index: u32, value: String },
    /// Enter in row `index`: a new row below it.
    TermSubmitted { index: u32 },
    /// Row `index`'s clear button.
    TermRemoved { index: u32 },
    /// Save what the editor holds.
    Save,
}

impl From<ContextsAction> for Input {
    fn from(action: ContextsAction) -> Self {
        let index = |i: u32| usize::try_from(i).unwrap_or(usize::MAX);
        match action {
            ContextsAction::Reload => Self::Reload,
            ContextsAction::Activate { id } => Self::Activate(id),
            ContextsAction::DeleteRequested { id } => Self::DeleteRequested(id),
            ContextsAction::DeleteCancelled => Self::DeleteCancelled,
            ContextsAction::DeleteConfirmed { id } => Self::DeleteConfirmed(id),
            ContextsAction::Edit { id } => Self::Edit(id),
            ContextsAction::Create => Self::Create,
            ContextsAction::CancelEdit => Self::CancelEdit,
            ContextsAction::NameChanged { name } => Self::NameChanged(name),
            ContextsAction::TermChanged { index: i, value } => Self::TermChanged {
                index: index(i),
                value,
            },
            ContextsAction::TermSubmitted { index: i } => Self::TermSubmitted(index(i)),
            ContextsAction::TermRemoved { index: i } => Self::TermRemoved(index(i)),
            ContextsAction::Save => Self::Save,
        }
    }
}

/// One context in the list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContextSummary {
    pub id: String,
    /// Its name, or its id when it has none.
    pub name: String,
    /// Whether it carries a prompt.
    pub has_prompt: bool,
    /// The words it listens for.
    pub vocabulary: Vec<String>,
}

/// One vocabulary row in the editor.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TermRow {
    /// The row's id, for as long as the row exists: what focus is asked by.
    pub id: u64,
    pub text: String,
}

/// The editor, open over one context.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContextEditor {
    /// The context's id: fixed while the editor is open.
    pub id: String,
    /// Whether the context is new.
    pub is_new: bool,
    pub name: String,
    pub prompt: String,
    /// The vocabulary rows, the last one empty for the next word.
    pub terms: Vec<TermRow>,
    /// Why the last save was refused, shown in the editor.
    pub error: Option<String>,
    /// Whether a save is on its way.
    pub saving: bool,
}

impl From<&ContextDraft<Buffer>> for ContextEditor {
    fn from(draft: &ContextDraft<Buffer>) -> Self {
        Self {
            id: draft.id.clone(),
            is_new: draft.is_new,
            name: draft.name.clone(),
            prompt: draft.prompt.text.clone(),
            terms: draft
                .terms
                .iter()
                .map(|(id, text)| TermRow {
                    id: id.get(),
                    text: text.clone(),
                })
                .collect(),
            error: draft.error.clone(),
            saving: draft.saving,
        }
    }
}

/// Something to tell the user on the page. The app says it in its own words.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ContextsNotice {
    CouldNotLoad { message: String },
    CouldNotSave { message: String },
}

impl From<Notice> for ContextsNotice {
    fn from(notice: Notice) -> Self {
        match notice {
            Notice::CouldNotLoad(message) => Self::CouldNotLoad { message },
            Notice::CouldNotSave(message) => Self::CouldNotSave { message },
        }
    }
}

/// How far a context reaches among the installed backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ContextReach {
    /// None of them can use a context.
    Nothing,
    /// `accepting` of the `total` installed backends can.
    Partly { accepting: u32, total: u32 },
}

impl From<Reach> for ContextReach {
    fn from(reach: Reach) -> Self {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        match reach {
            Reach::Nothing => Self::Nothing,
            Reach::Partly { accepting, total } => Self::Partly {
                accepting: count(accepting),
                total: count(total),
            },
        }
    }
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContextsPage {
    /// Whether the list has been read.
    pub loaded: bool,
    /// The contexts, in the order the user arranged them.
    pub contexts: Vec<ContextSummary>,
    /// The context in force, or `None`.
    pub active: Option<String>,
    /// The context whose Delete was pressed once.
    pub confirming_delete: Option<String>,
    /// The editor, while it is open.
    pub editor: Option<ContextEditor>,
    /// The vocabulary row to put the cursor in, asked for the
    /// `focus_asked`th time: the app moves the cursor when that goes up.
    pub focus_row: Option<u64>,
    pub focus_asked: u64,
    /// What to tell the user.
    pub notice: Option<ContextsNotice>,
    /// How far a context reaches among the installed backends, when the page
    /// says so: `None` when every one of them can use it, or none is
    /// installed, or the list isn't read yet.
    pub reach: Option<ContextReach>,
}

/// The page's state, the notice it shows, and the focus it last asked for.
#[derive(Default)]
struct Held {
    state: ContextsState<Buffer>,
    notice: Option<ContextsNotice>,
    focus_row: Option<u64>,
    focus_asked: u64,
    reach: Option<Reach>,
}

/// The Contexts page's store.
#[derive(uniffi::Object)]
pub struct ContextsStore {
    held: Mutex<Held>,
    observer: Arc<dyn ContextsObserver>,
}

#[uniffi::export]
impl ContextsStore {
    /// A store that tells `observer` what changes. Nothing is read until the
    /// app sends [`ContextsAction::Reload`].
    #[uniffi::constructor]
    pub fn new(observer: Arc<dyn ContextsObserver>) -> Arc<Self> {
        crate::identify();
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            observer,
        })
    }

    /// What the page shows now.
    pub fn page(&self) -> ContextsPage {
        snapshot(&self.lock())
    }

    /// The user did `action`.
    pub fn send(self: Arc<Self>, action: ContextsAction) {
        if action == ContextsAction::Reload {
            Arc::clone(&self).read_reach();
        }
        self.dispatch(action.into());
    }

    /// The editor's prompt now holds `text`.
    pub fn set_prompt(self: Arc<Self>, text: String) {
        {
            let mut held = self.lock();
            let Some(draft) = held.state.draft.as_mut() else {
                return;
            };
            if draft.prompt.text == text {
                return;
            }
            draft.prompt.text = text;
        }
        self.dispatch(Input::PromptEdited);
    }
}

impl ContextsStore {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read which installed backends can use a context, for the note under
    /// the page's introduction. A list that can't be read leaves the note as
    /// it was: the note is a hint, and the page works without it.
    fn read_reach(self: Arc<Self>) {
        RUNTIME.spawn(async move {
            let Ok(backends) = super_voice_app_core::daemon::client::list_backends().await else {
                return;
            };
            let page = {
                let mut held = self.lock();
                held.reach = page::reach(&backends);
                snapshot(&held)
            };
            self.observer.changed(page);
        });
    }

    /// Hand `input` to the page's logic, tell the app what changed, and start
    /// the work it asked for.
    fn dispatch(self: Arc<Self>, input: Input) {
        let (page, effects) = {
            let mut held = self.lock();
            let outcome = page::update(&mut held.state, input);
            if outcome.clear_notice {
                held.notice = None;
            }
            if let Some(notice) = outcome.notice {
                held.notice = Some(notice.into());
            }
            // Showing and closing the editor is its draft coming and going.
            let mut work = Vec::new();
            for effect in outcome.effects {
                match effect {
                    Effect::FocusTerm(row) => {
                        held.focus_row = Some(row.get());
                        held.focus_asked += 1;
                    }
                    Effect::OpenEditor | Effect::CloseEditor => {}
                    other => work.push(other),
                }
            }
            (snapshot(&held), work)
        };
        self.observer.changed(page);
        for effect in effects {
            let store = Arc::clone(&self);
            RUNTIME.spawn(async move {
                let input = match effect {
                    Effect::List => page::run_list().await,
                    Effect::Activate(id) => page::run_activate(id).await,
                    Effect::Delete(id) => page::run_delete(id).await,
                    Effect::Save(context) => page::run_save(context).await,
                    Effect::FocusTerm(_) | Effect::OpenEditor | Effect::CloseEditor => return,
                };
                store.dispatch(input);
            });
        }
    }
}

fn snapshot(held: &Held) -> ContextsPage {
    let state = &held.state;
    ContextsPage {
        loaded: state.loaded,
        contexts: state
            .items
            .iter()
            .map(|c| ContextSummary {
                id: c.id.clone(),
                name: c.display_name(),
                has_prompt: !c.prompt.trim().is_empty(),
                vocabulary: c.vocabulary.clone(),
            })
            .collect(),
        active: state.active.clone(),
        confirming_delete: state.confirming_delete.clone(),
        editor: state.draft.as_ref().map(ContextEditor::from),
        focus_row: held.focus_row,
        focus_asked: held.focus_asked,
        notice: held.notice.clone(),
        reach: held.reach.map(ContextReach::from),
    }
}
