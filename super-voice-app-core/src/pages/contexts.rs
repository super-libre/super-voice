// SPDX-License-Identifier: GPL-3.0-only
//! The Contexts page: the dictation contexts, which one is in force, and the
//! editor open over one.
//!
//! Nothing in the list moves until the daemon has answered: every write is
//! followed by reading the list again. The editor is the exception: what the
//! user is typing is theirs, and a list read again under an open editor never
//! rewrites the sentence they are halfway through.
//!
//! Which context one backend uses is set from its Configure sheet, and lives
//! with that sheet ([`super::configure`]).

use super_voice_shared::models::contexts::DictationContext;

use crate::daemon::client::v1::contexts::{
    ContextCatalog, delete_context, list_contexts, set_active_context, set_context,
};
use crate::state::TextBuffer;
use crate::state::contexts::{ContextDraft, ContextsState, RowId};

/// What happened on the page: something the user did, or the answer to work
/// an [`Effect`] started.
#[derive(Debug, Clone)]
pub enum Input {
    /// Read the list: the page was opened, or another app changed it.
    Reload,
    /// The list was read, or why not.
    Loaded(Result<ContextCatalog, String>),

    /// Put a context in force, or (`None`) none.
    Activate(Option<String>),
    /// It is in force, or why not.
    Activated(Result<(), String>),

    /// Delete was pressed once for a context.
    DeleteRequested(String),
    /// The delete was called off.
    DeleteCancelled,
    /// Delete was pressed again: delete it.
    DeleteConfirmed(String),
    /// It was deleted, or why not.
    Deleted(Result<(), String>),

    /// Open the editor over a context.
    Edit(String),
    /// Open the editor over a new context.
    Create,
    /// Close the editor, and drop what was typed.
    CancelEdit,
    /// The name field now holds this.
    NameChanged(String),
    /// The app changed the prompt in its own editor.
    PromptEdited,
    /// A vocabulary row now holds this. Several lines pasted at once become
    /// several rows.
    TermChanged { index: usize, value: String },
    /// Enter in a vocabulary row: a new row below it.
    TermSubmitted(usize),
    /// A vocabulary row's clear button.
    TermRemoved(usize),
    /// Save what the editor holds.
    Save,
    /// The daemon stored it, as it stored it (blanks dropped, terms trimmed),
    /// or why not.
    Saved(Result<DictationContext, String>),
}

/// Work [`update`] asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Read the list ([`run_list`]); answers [`Input::Loaded`].
    List,
    /// Put this in force ([`run_activate`]); answers [`Input::Activated`].
    Activate(Option<String>),
    /// Delete this ([`run_delete`]); answers [`Input::Deleted`].
    Delete(String),
    /// Store this ([`run_save`]); answers [`Input::Saved`].
    Save(DictationContext),
    /// Put the cursor in this vocabulary row.
    FocusTerm(RowId),
    /// Show the editor.
    OpenEditor,
    /// Close the editor.
    CloseEditor,
}

/// Something to tell the user on the page. Each app says it in its own
/// words. The editor's own errors stay in the editor (its draft's `error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The list could not be read, or changed.
    CouldNotLoad(String),
    /// A save failed after its editor was closed.
    CouldNotSave(String),
}

/// What an [`update`] asks of the app.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The work to start, in order.
    pub effects: Vec<Effect>,
    /// Whether the page's notice is out of date and goes. Applied before
    /// [`notice`](Self::notice).
    pub clear_notice: bool,
    /// What to tell the user.
    pub notice: Option<Notice>,
}

/// What `input` does to the page.
pub fn update<B: TextBuffer>(state: &mut ContextsState<B>, input: Input) -> Outcome {
    let mut out = Outcome::default();
    match input {
        Input::Reload | Input::Activated(Ok(())) | Input::Deleted(Ok(())) => {
            reload(state, &mut out);
        }
        Input::Loaded(Ok(catalog)) => state.replace(catalog.contexts, catalog.active),
        Input::Loaded(Err(e)) | Input::Activated(Err(e)) | Input::Deleted(Err(e)) => {
            log::warn!("contexts: {e}");
            state.loading = false;
            // Read, if only to fail: the page shows the notice rather than a
            // spinner that never stops.
            state.loaded = true;
            out.notice = Some(Notice::CouldNotLoad(e));
        }
        Input::Activate(id) => {
            out.clear_notice = true;
            out.effects.push(Effect::Activate(id));
        }
        Input::DeleteRequested(id) => state.confirming_delete = Some(id),
        Input::DeleteCancelled => state.confirming_delete = None,
        Input::DeleteConfirmed(id) => {
            state.confirming_delete = None;
            out.clear_notice = true;
            out.effects.push(Effect::Delete(id));
        }
        editing => editor(state, editing, &mut out),
    }
    out
}

/// Read the list again. Saying so, so the page says "loading" rather than
/// "no contexts".
fn reload<B>(state: &mut ContextsState<B>, out: &mut Outcome) {
    state.loading = true;
    out.effects.push(Effect::List);
}

/// The editor over one context.
fn editor<B: TextBuffer>(state: &mut ContextsState<B>, input: Input, out: &mut Outcome) {
    match input {
        Input::Edit(id) => {
            state.confirming_delete = None;
            if let Some(context) = state.get(&id) {
                state.draft = Some(ContextDraft::editing(context));
                out.effects.push(Effect::OpenEditor);
            }
        }
        Input::Create => {
            state.draft = Some(ContextDraft::creating(state.next_id()));
            out.effects.push(Effect::OpenEditor);
        }
        Input::CancelEdit => {
            state.draft = None;
            out.effects.push(Effect::CloseEditor);
        }
        Input::Save => save(state, out),
        Input::Saved(Ok(stored)) => {
            state.draft = None;
            out.effects.push(Effect::CloseEditor);
            // The row changes as the editor closes, rather than after the
            // list is read again; reading it again picks up anything else the
            // save moved.
            state.upsert(stored);
            reload(state, out);
        }
        Input::Saved(Err(e)) => match state.draft.as_mut() {
            Some(draft) => {
                draft.saving = false;
                draft.error = Some(e);
            }
            // The editor was closed under the save: the page is the only
            // place left to say so.
            None => out.notice = Some(Notice::CouldNotSave(e)),
        },
        typing => {
            let Some(draft) = state.draft.as_mut() else {
                return;
            };
            match typing {
                Input::NameChanged(name) => {
                    draft.name = name;
                    draft.error = None;
                }
                Input::PromptEdited => draft.error = None,
                Input::TermChanged { index, value } => {
                    draft.set_row(index, &value);
                    draft.error = None;
                }
                // Enter opens the next row with the cursor in it, so a list
                // is typed rather than clicked together.
                Input::TermSubmitted(index) => {
                    out.effects
                        .push(Effect::FocusTerm(draft.insert_row_after(index)));
                }
                Input::TermRemoved(index) => {
                    if let Some(focus) = draft.remove_row(index) {
                        out.effects.push(Effect::FocusTerm(focus));
                    }
                }
                _ => {}
            }
        }
    }
}

/// Store what the editor holds, once it has a name.
fn save<B: TextBuffer>(state: &mut ContextsState<B>, out: &mut Outcome) {
    let Some(draft) = state.draft.as_mut() else {
        return;
    };
    let context = draft.to_context();
    // Checked here as well as by the daemon, so an empty name is answered by
    // the form rather than a round trip.
    if let Err(e) = context.check() {
        draft.error = Some(e);
        return;
    }
    draft.saving = true;
    draft.error = None;
    out.effects.push(Effect::Save(context));
}

/// How far a context reaches among the installed backends, when the page
/// says so: a context that reaches nothing is the one failure the user cannot
/// see, so the page says it rather than let them wonder why nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// None of the installed backends can use a context.
    Nothing,
    /// `accepting` of the `total` installed backends can.
    Partly { accepting: usize, total: usize },
}

/// How far a context reaches among `backends`, the installed ones; `None`
/// when there is nothing to say (no backend installed, or all of them can use
/// one).
#[must_use]
pub fn reach(backends: &[crate::daemon::backends::BackendInfo]) -> Option<Reach> {
    let accepting = backends
        .iter()
        .filter(|b| {
            super_voice_registry_types::index::supports::has(
                &b.supports,
                super_voice_registry_types::index::supports::DICTATION_CONTEXT,
            )
        })
        .count();
    reach_of(accepting, backends.len())
}

/// [`reach`], from the count of backends that can use a context and the count
/// installed.
#[must_use]
pub const fn reach_of(accepting: usize, total: usize) -> Option<Reach> {
    if total == 0 || accepting == total {
        None
    } else if accepting == 0 {
        Some(Reach::Nothing)
    } else {
        Some(Reach::Partly { accepting, total })
    }
}

/// Read the list.
pub async fn run_list() -> Input {
    Input::Loaded(list_contexts().await.map_err(|e| e.to_string()))
}

/// Put `id` in force, or none.
pub async fn run_activate(id: Option<String>) -> Input {
    Input::Activated(
        set_active_context(id)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string()),
    )
}

/// Delete `id`.
pub async fn run_delete(id: String) -> Input {
    Input::Deleted(delete_context(id).await.map_err(|e| e.to_string()))
}

/// Store `context`.
pub async fn run_save(context: DictationContext) -> Input {
    Input::Saved(set_context(context).await.map_err(|e| e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reach_is_said_only_when_some_backend_misses_out() {
        assert_eq!(reach_of(0, 0), None);
        assert_eq!(reach_of(2, 2), None);
        assert_eq!(reach_of(0, 3), Some(Reach::Nothing));
        assert_eq!(
            reach_of(1, 3),
            Some(Reach::Partly {
                accepting: 1,
                total: 3
            })
        );
    }

    fn catalog(ids: &[&str], active: Option<&str>) -> ContextCatalog {
        ContextCatalog {
            active: active.map(str::to_string),
            contexts: ids
                .iter()
                .map(|id| DictationContext {
                    id: (*id).to_string(),
                    name: id.to_uppercase(),
                    vocabulary: vec!["kubectl".into()],
                    ..DictationContext::default()
                })
                .collect(),
        }
    }

    #[test]
    fn a_write_reads_the_list_again() {
        let mut state = ContextsState::<String>::default();
        let out = update(&mut state, Input::Activate(Some("a".into())));
        assert_eq!(out.effects, [Effect::Activate(Some("a".into()))]);
        let out = update(&mut state, Input::Activated(Ok(())));
        assert_eq!(out.effects, [Effect::List]);
        assert!(state.loading);
        update(&mut state, Input::Loaded(Ok(catalog(&["a"], Some("a")))));
        assert!(state.loaded && !state.loading);
        assert_eq!(state.active.as_deref(), Some("a"));
    }

    #[test]
    fn a_failed_read_is_said_and_stops_loading() {
        let mut state = ContextsState::<String>::default();
        update(&mut state, Input::Reload);
        let out = update(&mut state, Input::Loaded(Err("down".into())));
        assert_eq!(out.notice, Some(Notice::CouldNotLoad("down".into())));
        assert!(state.loaded && !state.loading);
    }

    #[test]
    fn delete_takes_two_presses() {
        let mut state = ContextsState::<String>::default();
        update(&mut state, Input::Loaded(Ok(catalog(&["a"], None))));
        assert!(
            update(&mut state, Input::DeleteRequested("a".into()))
                .effects
                .is_empty()
        );
        assert_eq!(state.confirming_delete.as_deref(), Some("a"));
        let out = update(&mut state, Input::DeleteConfirmed("a".into()));
        assert_eq!(out.effects, [Effect::Delete("a".into())]);
        assert!(state.confirming_delete.is_none());
    }

    #[test]
    fn the_editor_opens_saves_and_closes() {
        let mut state = ContextsState::<String>::default();
        update(&mut state, Input::Loaded(Ok(catalog(&["a"], None))));
        let out = update(&mut state, Input::Create);
        assert_eq!(out.effects, [Effect::OpenEditor]);

        // No name: answered by the form.
        assert!(update(&mut state, Input::Save).effects.is_empty());
        assert!(state.draft.as_ref().is_some_and(|d| d.error.is_some()));

        update(&mut state, Input::NameChanged("Coding".into()));
        update(
            &mut state,
            Input::TermChanged {
                index: 0,
                value: "kubectl\nrebase".into(),
            },
        );
        let out = update(&mut state, Input::Save);
        let [Effect::Save(context)] = out.effects.as_slice() else {
            panic!("a save, got {:?}", out.effects);
        };
        assert_eq!(context.vocabulary, ["kubectl", "rebase"]);

        let out = update(&mut state, Input::Saved(Ok(context.clone())));
        assert_eq!(out.effects, [Effect::CloseEditor, Effect::List]);
        assert!(state.draft.is_none());
        assert!(state.get(&context.id).is_some());
    }

    #[test]
    fn enter_in_a_row_makes_the_next_and_focuses_it() {
        let mut state = ContextsState::<String>::default();
        update(&mut state, Input::Create);
        let out = update(&mut state, Input::TermSubmitted(0));
        let draft = state.draft.as_ref().expect("a draft");
        assert_eq!(draft.terms.len(), 2);
        assert_eq!(out.effects, [Effect::FocusTerm(draft.terms[1].0)]);
    }

    #[test]
    fn a_failed_save_stays_in_the_editor_unless_it_was_closed() {
        let mut state = ContextsState::<String>::default();
        update(&mut state, Input::Create);
        let out = update(&mut state, Input::Saved(Err("taken".into())));
        assert!(out.notice.is_none());
        assert_eq!(
            state.draft.as_ref().and_then(|d| d.error.as_deref()),
            Some("taken")
        );
        update(&mut state, Input::CancelEdit);
        let out = update(&mut state, Input::Saved(Err("taken".into())));
        assert_eq!(out.notice, Some(Notice::CouldNotSave("taken".into())));
    }
}
