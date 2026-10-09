//! Result grid: a virtualized `DataTable` fed by a simple column/row model.

use gpui_kit::component::table::{Column, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::*;

pub struct ResultSet {
    columns: Vec<Column>,
    /// Per data column (excluding the row-number column): right-align as a number.
    numeric: Vec<bool>,
    rows: Vec<Vec<SharedString>>,
}

impl ResultSet {
    pub fn empty() -> Self {
        Self {
            columns: vec![row_number_column()],
            numeric: Vec::new(),
            rows: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether a statement has produced a result set (beyond the row-number column).
    pub fn has_result(&self) -> bool {
        self.columns.len() > 1
    }
}

fn row_number_column() -> Column {
    Column::new("#", "")
        .width(px(44.))
        .text_right()
        .fixed_left()
        .resizable(false)
        .selectable(false)
}

impl TableDelegate for ResultSet {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let text = match col_ix {
            0 => SharedString::from((row_ix + 1).to_string()),
            _ => self.rows[row_ix][col_ix - 1].clone(),
        };
        let cell = h_flex()
            .size_full()
            .font_family("monospace")
            .text_sm()
            .child(text);
        match col_ix {
            0 => cell.justify_end().text_color(cx.theme().muted_foreground),
            _ if self.numeric[col_ix - 1] => cell.justify_end(),
            _ => cell,
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        match col_ix {
            0 => (row_ix + 1).to_string(),
            _ => self.rows[row_ix][col_ix - 1].to_string(),
        }
    }
}
