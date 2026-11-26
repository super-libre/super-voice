// SPDX-License-Identifier: GPL-3.0-only
//! The page's Selects, kept in step with the core's state.
//!
//! A kit `Select` keeps its items and its selection in a state entity of its
//! own. The page builds one per picker the first time it draws it, and each
//! time it draws it again gives it the items and the selection the core's
//! state says, which the Select takes only when they changed. A pick comes
//! back to the page by its [`PickerKey`].

use std::collections::HashMap;

use gpui_kit::component::IndexPath;
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectItem, SelectState};
use gpui_kit::{AppContext as _, Context, Entity, SharedString, Subscription, Window};
use super_voice_app_core::state::Stage;

/// One row of a picker: what it stands for, and its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: SharedString,
    pub name: SharedString,
}

impl Choice {
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

impl SelectItem for Choice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &SharedString {
        &self.id
    }
}

/// Which picker.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PickerKey {
    Model(Stage),
    Device(Stage),
    Language(Stage),
    /// The synthesis model's voice.
    Voice,
    /// The open Configure sheet's dictation context.
    Context,
    /// One of the open Configure sheet's options, by its backend and name.
    Option(String, String),
}

/// The kit's `SearchableVec`, not a plain `Vec`: a plain list ignores what
/// is typed in a searchable picker's field, and the language picker lists a
/// hundred languages.
pub type PickerState = Entity<SelectState<SearchableVec<Choice>>>;

struct Picker {
    state: PickerState,
    items: Vec<Choice>,
    _picked: Subscription,
}

/// What the page does with a pick.
pub type OnPick<P> = fn(&mut P, &PickerKey, &SharedString, &mut Window, &mut Context<P>);

/// Every picker the page has drawn.
pub struct Pickers<P: 'static> {
    pickers: HashMap<PickerKey, Picker>,
    on_pick: OnPick<P>,
}

impl<P: 'static> Pickers<P> {
    pub fn new(on_pick: OnPick<P>) -> Self {
        Self {
            pickers: HashMap::new(),
            on_pick,
        }
    }

    /// The picker `key`, holding `items` with `selected` picked (none when
    /// `selected` isn't one of them).
    pub fn get(
        &mut self,
        key: &PickerKey,
        items: Vec<Choice>,
        selected: Option<&str>,
        searchable: bool,
        window: &mut Window,
        cx: &mut Context<P>,
    ) -> PickerState {
        let at = |items: &[Choice]| {
            selected
                .and_then(|id| items.iter().position(|c| c.id.as_ref() == id))
                .map(|row| IndexPath::default().row(row))
        };
        if let Some(picker) = self.pickers.get_mut(key) {
            let state = picker.state.clone();
            if picker.items == items {
                let now = state.read(cx).selected_value().map(SharedString::as_ref);
                if now != selected.filter(|id| items.iter().any(|c| c.id.as_ref() == *id)) {
                    let index = at(&items);
                    state.update(cx, |state, cx| state.set_selected_index(index, window, cx));
                }
            } else {
                picker.items.clone_from(&items);
                let index = at(&items);
                state.update(cx, |state, cx| {
                    state.set_items(SearchableVec::new(items), window, cx);
                    state.set_selected_index(index, window, cx);
                });
            }
            return state;
        }
        let index = at(&items);
        let rows = items.clone();
        let state = cx.new(|cx| {
            SelectState::new(SearchableVec::new(rows), index, window, cx).searchable(searchable)
        });
        let on_pick = self.on_pick;
        let picked_key = key.clone();
        let picked = cx.subscribe_in(
            &state,
            window,
            move |page, _, event: &SelectEvent<SearchableVec<Choice>>, window, cx| {
                let SelectEvent::Confirm(Some(id)) = event else {
                    return;
                };
                on_pick(page, &picked_key, id, window, cx);
            },
        );
        self.pickers.insert(
            key.clone(),
            Picker {
                state: state.clone(),
                items,
                _picked: picked,
            },
        );
        state
    }

    /// Forget the pickers of the Configure sheet, whose backend changed.
    pub fn forget_sheet(&mut self) {
        self.pickers
            .retain(|key, _| !matches!(key, PickerKey::Context | PickerKey::Option(..)));
    }
}
