// SPDX-License-Identifier: GPL-3.0-only
//! A list the keyboard reaches, as the kit's own `List` is: Tab focuses it,
//! Up and Down move the keyboard's row (the core's `ListCursor`), Enter opens
//! it. The keys are the kit's, bound in its `List` context. While the list
//! has the keyboard's focus, and the last input was a key (as the kit's
//! `DataTable` draws its ring), the keyboard's row has the theme's focus ring.
//!
//! The list's scrolling element holds the focus; its rows are its children,
//! from [`KeyList::first_row`] on, so the keyboard's row scrolls into view.
//!
//! ```ignore
//! key_list::wire(div().id("notes-list").overflow_y_scroll(), self, cx).children(rows)
//! ```

use gpui_kit::base::actions::{Confirm, SelectDown, SelectUp};
use gpui_kit::component::ThemeStyled as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Div, FocusHandle, InteractiveElement as _, IntoElement, ParentElement as _,
    ScrollHandle, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use super_voice_app_core::state::ListCursor;

/// What a page keeps for its list.
pub struct KeyListState {
    focus: FocusHandle,
    scroll: ScrollHandle,
    cursor: ListCursor,
}

impl KeyListState {
    #[must_use]
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            scroll: ScrollHandle::new(),
            cursor: ListCursor::default(),
        }
    }

    /// Give the list the keyboard's focus.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }
}

/// A page with a list the keyboard reaches.
pub trait KeyList: Sized + 'static {
    fn key_list(&self) -> &KeyListState;
    fn key_list_mut(&mut self) -> &mut KeyListState;
    /// The rows' ids, in the order shown.
    fn rows(&self, cx: &App) -> Vec<String>;
    /// The row picked (open), if any.
    fn picked(&self) -> Option<String>;
    /// Open the row `id`: Enter was pressed on it.
    fn open_row(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>);
    /// Which of the scrolling element's children is the first row: one more
    /// for each child above the rows (a count, the row of a take).
    fn first_row(&self, _cx: &App) -> usize {
        0
    }
}

/// The row the keyboard is on.
fn current<V: KeyList>(view: &V, cx: &App) -> Option<String> {
    let rows = view.rows(cx);
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let picked = view.picked();
    view.key_list()
        .cursor
        .current(&rows, picked.as_deref())
        .map(str::to_string)
}

/// Move the keyboard's row, and scroll it into view.
fn step<V: KeyList>(view: &mut V, down: bool, cx: &mut Context<V>) {
    let rows = view.rows(cx);
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let picked = view.picked();
    let first = view.first_row(cx);
    let list = view.key_list_mut();
    list.cursor.step(&rows, picked.as_deref(), down);
    if let Some(at) = list
        .cursor
        .current(&rows, picked.as_deref())
        .and_then(|row| rows.iter().position(|r| *r == row))
    {
        list.scroll.scroll_to_item(first + at);
    }
    cx.notify();
}

/// Make `list`, the list's scrolling element, reach the keyboard.
pub fn wire<V: KeyList>(list: Stateful<Div>, view: &V, cx: &mut Context<V>) -> Stateful<Div> {
    list.key_context("List")
        .track_focus(&view.key_list().focus)
        .track_scroll(&view.key_list().scroll)
        .role(gpui_kit::Role::List)
        .on_action(cx.listener(|this: &mut V, _: &SelectDown, _, cx| step(this, true, cx)))
        .on_action(cx.listener(|this: &mut V, _: &SelectUp, _, cx| step(this, false, cx)))
        .on_action(cx.listener(|this: &mut V, _: &Confirm, window, cx| {
            if let Some(id) = current(this, cx) {
                this.open_row(&id, window, cx);
            }
        }))
}

/// The row that has the focus ring: the keyboard's, while the list has the
/// keyboard's focus and the last input was a key.
pub fn ringed<V: KeyList>(view: &V, window: &Window, cx: &App) -> Option<String> {
    let list = view.key_list();
    (list.focus.is_focused(window) && window.last_input_was_keyboard())
        .then(|| current(view, cx))
        .flatten()
}

/// `row`, with the theme's focus ring when it is the `ringed` one.
pub fn row(row: impl IntoElement, ringed: bool, window: &Window, cx: &App) -> Div {
    div()
        .relative()
        .child(row)
        .when(ringed, |row| row.focus_ring_style(window, cx))
}
