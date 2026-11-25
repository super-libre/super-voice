// SPDX-License-Identifier: GPL-3.0-only
//! The Library's table: the kit's `DataTable` over the core's rows.
//!
//! Six columns while the inspector sits beside the table (Name, Feature,
//! Kind, Runs, Version, State), four when it is a sheet (Feature goes, since
//! Kind says it, and Version moves to the inspector). The columns share the
//! table's width, so they are worked out again when the window is resized.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::table::{Column as KitColumn, TableDelegate, TableState};
use gpui_kit::{
    App, Context, FontWeight, IntoElement, ParentElement as _, Pixels, SharedString, Styled as _,
    Window, div, px,
};
use super_voice_app_core::pages::library::Row;

use super::{INSPECTOR, features_words, kinds_words, runs_words, state_cell};
use crate::t;
use crate::ui::width::{self, Inspector};

/// One of the table's columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Feature,
    Kind,
    Runs,
    Version,
    State,
}

impl Field {
    /// Its heading's message key.
    const fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Feature => "feature",
            Self::Kind => "kind",
            Self::Runs => "library-runs",
            Self::Version => "version",
            Self::State => "library-state",
        }
    }

    /// Its share of the table's width.
    const fn share(self) -> f32 {
        match self {
            Self::Name => 0.9,
            Self::Feature | Self::Kind => 1.2,
            Self::Runs => 0.65,
            Self::Version => 0.75,
            Self::State => 1.55,
        }
    }
}

/// A column, and how wide it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub field: Field,
    pub width: Pixels,
}

/// The columns for content `content` wide.
#[must_use]
pub fn columns(content: Pixels) -> Vec<Column> {
    let (fields, table): (&[Field], Pixels) = match width::inspector(content) {
        Inspector::Beside => (
            &[
                Field::Name,
                Field::Feature,
                Field::Kind,
                Field::Runs,
                Field::Version,
                Field::State,
            ],
            content - px(INSPECTOR),
        ),
        // The narrow list has no table; its columns don't matter.
        Inspector::Sheet | Inspector::Page => (
            &[Field::Name, Field::Kind, Field::Runs, Field::State],
            content,
        ),
    };
    // Room for the scrollbar, and a pixel for the border.
    let room = (table - px(14.)).max(px(320.));
    let shares: f32 = fields.iter().map(|f| f.share()).sum();
    fields
        .iter()
        .map(|&field| Column {
            field,
            width: (room * (field.share() / shares)).floor(),
        })
        .collect()
}

/// The table's rows and columns, as the page hands them over.
#[derive(Default)]
pub struct Rows {
    pub rows: Vec<Row>,
    pub columns: Vec<Column>,
}

impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> KitColumn {
        let column = self.columns[col_ix];
        let kit = KitColumn::new(column.field.key(), t(column.field.key()))
            .width(column.width)
            .resizable(false)
            .movable(false)
            .selectable(false);
        if column.field == Field::Version {
            kit.text_right()
        } else {
            kit
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let (Some(row), Some(column)) = (self.rows.get(row_ix), self.columns.get(col_ix)) else {
            return div().into_any_element();
        };
        let text = |words: String| div().truncate().child(words).into_any_element();
        match column.field {
            Field::Name => div()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(row.name.clone())
                .into_any_element(),
            Field::Feature => text(features_words(&row.features)),
            Field::Kind => text(kinds_words(&row.kinds)),
            Field::Runs => text(runs_words(row.runs).to_string()),
            Field::Version => div()
                .w_full()
                .text_right()
                .text_color(cx.theme().muted_foreground)
                .child(row.version.clone())
                .into_any_element(),
            Field::State => state_cell(row, false, cx),
        }
    }

    fn cell_label(&self, row_ix: usize, col_ix: usize, _: &App) -> Option<SharedString> {
        let row = self.rows.get(row_ix)?;
        Some(match self.columns.get(col_ix)?.field {
            Field::Name => row.name.clone().into(),
            Field::Feature => features_words(&row.features).into(),
            Field::Kind => kinds_words(&row.kinds).into(),
            Field::Runs => runs_words(row.runs),
            Field::Version => row.version.clone().into(),
            Field::State => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six columns beside the inspector, four under a sheet; they fill the
    /// table's width.
    #[test]
    fn the_columns_follow_the_width() {
        let wide = columns(px(945.));
        assert_eq!(wide.len(), 6);
        let total: Pixels = wide.iter().map(|c| c.width).fold(px(0.), |a, b| a + b);
        assert!(total <= px(945. - INSPECTOR) && total > px(945. - INSPECTOR - 24.));
        let compact = columns(px(672.));
        assert_eq!(
            compact.iter().map(|c| c.field).collect::<Vec<_>>(),
            [Field::Name, Field::Kind, Field::Runs, Field::State]
        );
    }
}
