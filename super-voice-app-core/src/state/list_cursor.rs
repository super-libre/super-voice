// SPDX-License-Identifier: GPL-3.0-only
//! The keyboard's place in a list of notes, clips or contexts: the row Up and
//! Down move to and Enter opens.
//!
//! It follows what is picked: until the keyboard moves, it is on the row
//! picked (the note open), or the first; once another row is picked with
//! the pointer, it is on that one again.

/// Where the keyboard is in a list. Rows are named by their ids.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListCursor {
    /// The row the keyboard moved to.
    at: Option<String>,
    /// The row picked when it moved there.
    picked_then: Option<String>,
}

impl ListCursor {
    /// The row the keyboard is on among `rows`, with `picked` the row
    /// picked: where it moved, unless another row was picked since or its
    /// row has gone; else the row picked; else the first. `None` for an empty
    /// list.
    #[must_use]
    pub fn current<'a>(&self, rows: &[&'a str], picked: Option<&str>) -> Option<&'a str> {
        let find = |id: &str| rows.iter().copied().find(|row| *row == id);
        self.at
            .as_deref()
            .filter(|_| self.picked_then.as_deref() == picked)
            .and_then(find)
            .or_else(|| picked.and_then(find))
            .or_else(|| rows.first().copied())
    }

    /// Move one row down, or up, from [`Self::current`]; the ends hold.
    pub fn step(&mut self, rows: &[&str], picked: Option<&str>, down: bool) {
        let Some(current) = self.current(rows, picked) else {
            return;
        };
        let at = rows.iter().position(|row| *row == current).unwrap_or(0);
        let next = if down {
            (at + 1).min(rows.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        self.at = Some(rows[next].to_string());
        self.picked_then = picked.map(str::to_string);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: [&str; 3] = ["a", "b", "c"];

    #[test]
    fn it_starts_on_the_row_picked_or_the_first() {
        let cursor = ListCursor::default();
        assert_eq!(cursor.current(&ROWS, None), Some("a"));
        assert_eq!(cursor.current(&ROWS, Some("b")), Some("b"));
        assert_eq!(cursor.current(&[], None), None);
    }

    #[test]
    fn up_and_down_move_one_row_and_stop_at_the_ends() {
        let mut cursor = ListCursor::default();
        cursor.step(&ROWS, None, true);
        assert_eq!(cursor.current(&ROWS, None), Some("b"));
        cursor.step(&ROWS, None, true);
        cursor.step(&ROWS, None, true);
        assert_eq!(cursor.current(&ROWS, None), Some("c"));
        for _ in 0..4 {
            cursor.step(&ROWS, None, false);
        }
        assert_eq!(cursor.current(&ROWS, None), Some("a"));
    }

    #[test]
    fn a_row_picked_with_the_pointer_takes_the_keyboard_there() {
        let mut cursor = ListCursor::default();
        cursor.step(&ROWS, Some("a"), true);
        assert_eq!(cursor.current(&ROWS, Some("a")), Some("b"));
        // "c" picked with the pointer: the keyboard follows.
        assert_eq!(cursor.current(&ROWS, Some("c")), Some("c"));
        cursor.step(&ROWS, Some("c"), false);
        assert_eq!(cursor.current(&ROWS, Some("c")), Some("b"));
    }

    #[test]
    fn a_row_that_went_leaves_the_keyboard_on_the_one_picked() {
        let mut cursor = ListCursor::default();
        cursor.step(&ROWS, None, true);
        assert_eq!(cursor.current(&["a", "c"], None), Some("a"));
    }
}
