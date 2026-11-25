// SPDX-License-Identifier: GPL-3.0-only
//! How wide the window and a page's content are, and the rules that follow
//! from it (the skill's "Narrow windows").
//!
//! The window is wide from 960 px, compact (a 48 px rail of icons) from 600,
//! and narrow below (the pages in a drawer). Inside a page the rules follow
//! the content's width, not the window's: Notes in two panes from 640 px, the
//! Library inspector beside its table from 760, a settings row's control under
//! its text below 480, a card's fields in one row when each gets 170.
//!
//! Every width here is worked out from the window's, so a page reads it in
//! `render` with no state of its own:
//!
//! ```ignore
//! let panes = if width::content(window) >= width::TWO_PANES { 2 } else { 1 };
//! ```

use gpui_kit::component::window_paddings;
use gpui_kit::{Pixels, Window, px};

/// The window's width from which it is [`WindowSize::Wide`].
pub const WIDE: Pixels = px(960.);
/// The window's width from which it is [`WindowSize::Compact`].
pub const COMPACT: Pixels = px(600.);
/// The smallest window.
pub const MIN_WINDOW: (f32, f32) = (360., 520.);

/// The sidebar's width in a wide window: the kit's own.
pub const SIDEBAR: Pixels = px(255.);
/// The rail of icons in a compact window: the kit's collapsed sidebar.
pub const RAIL: Pixels = px(48.);
/// The drawer in a narrow window.
pub const DRAWER: Pixels = px(280.);

/// Content from which a list and its detail sit side by side.
pub const TWO_PANES: Pixels = px(640.);
/// Content from which an inspector sits beside its table.
pub const INSPECTOR_BESIDE: Pixels = px(760.);
/// Content from which an inspector is a sheet over its table; below, a page.
pub const INSPECTOR_SHEET: Pixels = px(600.);
/// A settings row narrower than this puts its control under its text.
pub const ROW_STACKS: Pixels = px(480.);
/// The least a field in a card's row may get.
pub const FIELD: Pixels = px(170.);
/// The space between fields in a row.
pub const FIELD_GAP: Pixels = px(12.);
/// A card narrower than this has one field a row.
pub const CARD_ONE_FIELD: Pixels = px(400.);
/// The space a page leaves at its sides: [`page_padding`].
pub const PAGE_PADDING_WIDE: Pixels = px(24.);
pub const PAGE_PADDING_NARROW: Pixels = px(16.);

/// How the window is laid out, by its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSize {
    /// 960 px and wider: the sidebar with its groups.
    Wide,
    /// 600 to 959 px: a rail of icons.
    Compact,
    /// Under 600 px: the pages in a drawer.
    Narrow,
}

impl WindowSize {
    /// The layout for a window `width` wide.
    #[must_use]
    pub fn for_width(width: Pixels) -> Self {
        if width >= WIDE {
            Self::Wide
        } else if width >= COMPACT {
            Self::Compact
        } else {
            Self::Narrow
        }
    }

    /// The layout of `window`.
    #[must_use]
    pub fn of(window: &Window) -> Self {
        Self::for_width(window_width(window))
    }

    /// How much of the window's width the navigation takes.
    #[must_use]
    pub const fn navigation(self) -> Pixels {
        match self {
            Self::Wide => SIDEBAR,
            Self::Compact => RAIL,
            Self::Narrow => px(0.),
        }
    }
}

/// The window's visible width: without the shadow a client-decorated window
/// draws around itself.
#[must_use]
pub fn window_width(window: &Window) -> Pixels {
    let paddings = window_paddings(window);
    window.viewport_size().width - paddings.left - paddings.right
}

/// The width of the content area, where a page is drawn.
///
/// `let wide = width::content(window) >= width::TWO_PANES;`
#[must_use]
pub fn content(window: &Window) -> Pixels {
    content_for(window_width(window))
}

/// [`content`], for a window `window_width` wide.
#[must_use]
pub fn content_for(window_width: Pixels) -> Pixels {
    window_width - WindowSize::for_width(window_width).navigation()
}

/// The space a page leaves at each side: 24 px, 16 in a narrow window.
///
/// `div().px(width::page_padding(window))`
#[must_use]
pub fn page_padding(window: &Window) -> Pixels {
    if WindowSize::of(window) == WindowSize::Narrow {
        PAGE_PADDING_NARROW
    } else {
        PAGE_PADDING_WIDE
    }
}

/// Where an inspector goes for content `content` wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inspector {
    /// Beside its table.
    Beside,
    /// In a sheet over the table.
    Sheet,
    /// In place of the table, with a back button.
    Page,
}

/// Where an inspector goes for content `content` wide.
///
/// `match width::inspector(width::content(window)) { Inspector::Beside => …, … }`
#[must_use]
pub fn inspector(content: Pixels) -> Inspector {
    if content >= INSPECTOR_BESIDE {
        Inspector::Beside
    } else if content >= INSPECTOR_SHEET {
        Inspector::Sheet
    } else {
        Inspector::Page
    }
}

/// How many of `fields` fields a card `card` wide puts in a row: all of them
/// when each gets 170 px, otherwise two, and one in a card under 400 px.
///
/// `let columns = width::fields_per_row(card_width, 3);`
#[must_use]
pub fn fields_per_row(card: Pixels, fields: usize) -> usize {
    if fields <= 1 {
        return fields;
    }
    // reason: a handful of fields; the count fits an f32 exactly.
    #[allow(clippy::cast_precision_loss)]
    let n = fields as f32;
    if card >= FIELD * n + FIELD_GAP * (n - 1.) {
        fields
    } else if card >= CARD_ONE_FIELD {
        2
    } else {
        1
    }
}

/// Whether a settings row `row` wide puts its control under its text.
///
/// `SettingsRow` asks this itself; a page asks it for a row of its own.
#[must_use]
pub fn row_stacks(row: Pixels) -> bool {
    row < ROW_STACKS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_wide_compact_or_narrow_by_its_width() {
        assert_eq!(WindowSize::for_width(px(1200.)), WindowSize::Wide);
        assert_eq!(WindowSize::for_width(px(960.)), WindowSize::Wide);
        assert_eq!(WindowSize::for_width(px(959.)), WindowSize::Compact);
        assert_eq!(WindowSize::for_width(px(600.)), WindowSize::Compact);
        assert_eq!(WindowSize::for_width(px(599.)), WindowSize::Narrow);
        assert_eq!(WindowSize::for_width(px(360.)), WindowSize::Narrow);
    }

    #[test]
    fn the_content_is_the_window_less_its_navigation() {
        assert_eq!(content_for(px(1200.)), px(945.));
        assert_eq!(content_for(px(720.)), px(672.));
        assert_eq!(content_for(px(420.)), px(420.));
    }

    #[test]
    fn an_inspector_moves_as_the_content_narrows() {
        assert_eq!(inspector(px(945.)), Inspector::Beside);
        assert_eq!(inspector(px(672.)), Inspector::Sheet);
        assert_eq!(inspector(px(420.)), Inspector::Page);
    }

    #[test]
    fn a_cards_fields_share_a_row_while_each_gets_170_px() {
        assert_eq!(fields_per_row(px(800.), 3), 3);
        assert_eq!(fields_per_row(px(800.), 4), 4);
        assert_eq!(fields_per_row(px(600.), 4), 2);
        assert_eq!(fields_per_row(px(534.), 3), 3);
        assert_eq!(fields_per_row(px(533.), 3), 2);
        assert_eq!(fields_per_row(px(380.), 3), 1);
        assert_eq!(fields_per_row(px(300.), 1), 1);
    }

    #[test]
    fn a_settings_row_stacks_below_480_px() {
        assert!(row_stacks(px(479.)));
        assert!(!row_stacks(px(480.)));
    }
}
