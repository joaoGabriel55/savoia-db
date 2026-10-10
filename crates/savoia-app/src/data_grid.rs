//! The data view's grid: rows loaded from the server plus the pending
//! changes on them. New rows show first, then loaded rows; edited cells,
//! new rows and rows marked for deletion are tinted until committed.

use std::collections::{BTreeMap, BTreeSet};

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::edit::{Change, Value};
use savoia_core::{Row, TableInfo};

/// What the grid asks of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Sort by a data column (0-based), descending or not; `None` resets.
    Sort(Option<(usize, bool)>),
    More,
}

/// A grid row: a new one (index into `inserted`) or a loaded one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    New(usize),
    Loaded(usize),
}

pub struct DataRows {
    columns: Vec<Column>,
    names: Vec<String>,
    numeric: Vec<bool>,
    pub(crate) rows: Vec<Row>,
    /// Another page exists on the server.
    pub(crate) more: bool,
    pub(crate) loading: bool,
    pub(crate) request: Option<Request>,
    /// Data columns (0-based) of the row's key; empty when not editable.
    key: Vec<usize>,
    pub(crate) editable: bool,
    pub(crate) inserted: Vec<Vec<Value>>,
    /// New values by (loaded row, data column).
    pub(crate) edits: BTreeMap<(usize, usize), Value>,
    /// Loaded rows marked for deletion.
    pub(crate) deleted: BTreeSet<usize>,
    /// The cell being edited, as (grid row, data column), and its input.
    pub(crate) editing: Option<(usize, usize, Entity<InputState>)>,
    /// The selected cell, as (grid row, data column).
    pub(crate) selected: Option<(usize, usize)>,
}

impl DataRows {
    pub fn new(info: &TableInfo, key: &[String], editable: bool) -> Self {
        let numeric: Vec<bool> = info
            .columns
            .iter()
            .map(|c| is_numeric_type(&c.data_type))
            .collect();
        let columns = std::iter::once(
            Column::new("#", "")
                .width(px(52.))
                .text_right()
                .fixed_left()
                .resizable(false)
                .selectable(false),
        )
        .chain(info.columns.iter().zip(&numeric).map(|(c, numeric)| {
            let column = Column::new(SharedString::from(c.name.clone()), c.name.clone()).sortable();
            if *numeric {
                column.text_right()
            } else {
                column
            }
        }))
        .collect();
        let names: Vec<String> = info.columns.iter().map(|c| c.name.clone()).collect();
        let key = key
            .iter()
            .filter_map(|k| names.iter().position(|n| n == k))
            .collect();
        Self {
            columns,
            names,
            numeric,
            rows: Vec::new(),
            more: false,
            loading: true,
            request: None,
            key,
            editable,
            inserted: Vec::new(),
            edits: BTreeMap::new(),
            deleted: BTreeSet::new(),
            editing: None,
            selected: None,
        }
    }

    /// Loaded rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[cfg(test)]
    pub fn row(&self, ix: usize) -> &Row {
        &self.rows[ix]
    }

    pub fn source(&self, grid_row: usize) -> Source {
        match grid_row.checked_sub(self.inserted.len()) {
            None => Source::New(grid_row),
            Some(loaded) => Source::Loaded(loaded),
        }
    }

    /// The value shown in a cell, pending edits included.
    pub fn value(&self, grid_row: usize, col: usize) -> Option<&str> {
        match self.source(grid_row) {
            Source::New(i) => self.inserted[i][col].as_deref(),
            Source::Loaded(i) => match self.edits.get(&(i, col)) {
                Some(value) => value.as_deref(),
                None => self.rows[i][col].as_deref(),
            },
        }
    }

    /// Sets a cell's pending value. Setting a loaded cell back to what the
    /// server has drops the edit.
    pub fn set(&mut self, grid_row: usize, col: usize, value: Value) {
        match self.source(grid_row) {
            Source::New(i) => self.inserted[i][col] = value,
            Source::Loaded(i) => {
                if self.rows[i][col].as_deref() == value.as_deref() {
                    self.edits.remove(&(i, col));
                } else {
                    self.edits.insert((i, col), value);
                }
            }
        }
    }

    pub fn add_row(&mut self) {
        self.inserted.insert(0, vec![None; self.names.len()]);
        self.editing = None;
    }

    /// Marks a loaded row for deletion, or unmarks it; removes a new row.
    pub fn toggle_delete(&mut self, grid_row: usize) {
        match self.source(grid_row) {
            Source::New(i) => {
                self.inserted.remove(i);
            }
            Source::Loaded(i) => {
                if !self.deleted.remove(&i) {
                    self.deleted.insert(i);
                }
            }
        }
        self.editing = None;
    }

    pub fn has_changes(&self) -> bool {
        !self.inserted.is_empty() || !self.edits.is_empty() || !self.deleted.is_empty()
    }

    pub fn discard(&mut self) {
        self.inserted.clear();
        self.edits.clear();
        self.deleted.clear();
        self.editing = None;
    }

    fn key_of(&self, loaded: usize) -> Vec<(String, Value)> {
        self.key
            .iter()
            .map(|&col| {
                let value = self.rows[loaded][col].as_deref().map(str::to_owned);
                (self.names[col].clone(), value)
            })
            .collect()
    }

    /// The pending changes: deletes, then updates, then inserts. Inserts
    /// list only the columns given a value, so the rest take defaults.
    pub fn changes(&self) -> Vec<Change> {
        let mut changes: Vec<Change> = self
            .deleted
            .iter()
            .map(|&i| Change::Delete {
                key: self.key_of(i),
            })
            .collect();
        let mut updates: BTreeMap<usize, Vec<(String, Value)>> = BTreeMap::new();
        for ((row, col), value) in &self.edits {
            if !self.deleted.contains(row) {
                updates
                    .entry(*row)
                    .or_default()
                    .push((self.names[*col].clone(), value.clone()));
            }
        }
        changes.extend(updates.into_iter().map(|(row, values)| Change::Update {
            key: self.key_of(row),
            values,
        }));
        changes.extend(self.inserted.iter().rev().map(|row| {
            Change::Insert {
                values: row
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.is_some())
                    .map(|(col, v)| (self.names[col].clone(), v.clone()))
                    .collect(),
            }
        }));
        changes
    }
}

/// A coarse guess from the type the server prints, for alignment.
pub fn is_numeric_type(data_type: &str) -> bool {
    let t = data_type.to_ascii_lowercase();
    [
        "int", "numeric", "decimal", "real", "double", "float", "serial", "money",
    ]
    .iter()
    .any(|n| t.contains(n))
        && !t.contains("interval")
        && !t.contains("point")
}

impl TableDelegate for DataRows {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.inserted.len() + self.rows.len()
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
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let source = self.source(row_ix);
        let deleted = matches!(source, Source::Loaded(i) if self.deleted.contains(&i));
        let tint = match source {
            Source::New(_) => Some(theme.success.opacity(0.14)),
            _ if deleted => Some(theme.danger.opacity(0.14)),
            Source::Loaded(i) if col_ix > 0 && self.edits.contains_key(&(i, col_ix - 1)) => {
                Some(theme.warning.opacity(0.2))
            }
            _ => None,
        };
        let cell = h_flex()
            .size_full()
            .font_family("monospace")
            .text_sm()
            .when_some(tint, |c, tint| c.bg(tint))
            .when(deleted, |c| c.line_through());
        if col_ix == 0 {
            let label = match source {
                Source::New(_) => "new".to_string(),
                Source::Loaded(i) => (i + 1).to_string(),
            };
            return cell
                .justify_end()
                .text_color(muted)
                .child(label)
                .into_any_element();
        }
        let col = col_ix - 1;
        if let Some((_, _, input)) = self
            .editing
            .as_ref()
            .filter(|(r, c, _)| (*r, *c) == (row_ix, col))
        {
            return Input::new(input).xsmall().into_any_element();
        }
        let cell = cell.when(self.numeric[col], |c| c.justify_end());
        match self.value(row_ix, col) {
            Some(text) => cell
                .child(SharedString::from(text.to_owned()))
                .into_any_element(),
            None => cell
                .text_color(muted)
                .child(match source {
                    Source::New(_) => "default",
                    Source::Loaded(_) => "NULL",
                })
                .into_any_element(),
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let sort = match (col_ix, sort) {
            (0, _) | (_, ColumnSort::Default) => None,
            (col, ColumnSort::Ascending) => Some((col - 1, false)),
            (col, ColumnSort::Descending) => Some((col - 1, true)),
        };
        self.request = Some(Request::Sort(sort));
        cx.notify();
    }

    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        if !self.editable {
            return menu;
        }
        let table = cx.entity().downgrade();
        let update = move |f: fn(&mut DataRows, usize, Option<usize>), col: Option<usize>| {
            let table = table.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                table
                    .update(cx, |t, cx| {
                        f(t.delegate_mut(), row_ix, col);
                        t.refresh(cx);
                        cx.notify();
                    })
                    .ok();
            }
        };
        let source = self.source(row_ix);
        let delete_label = match source {
            Source::New(_) => "Remove new row",
            Source::Loaded(i) if self.deleted.contains(&i) => "Keep row",
            Source::Loaded(_) => "Delete row",
        };
        let cell = self
            .selected
            .filter(|(row, _)| *row == row_ix)
            .map(|(_, col)| col);
        menu.when_some(cell, |menu, col| {
            menu.item(
                PopupMenuItem::new(format!("Set {} to NULL", self.names[col])).on_click(update(
                    |rows, row, col| rows.set(row, col.unwrap_or(0), None),
                    Some(col),
                )),
            )
        })
        .item(
            PopupMenuItem::new(delete_label)
                .icon(IconName::Delete)
                .on_click(update(|rows, row, _| rows.toggle_delete(row), None)),
        )
    }

    fn has_more(&self, _: &App) -> bool {
        self.more && !self.loading
    }

    fn load_more(&mut self, _: &mut Window, cx: &mut Context<TableState<Self>>) {
        if self.more && !self.loading {
            self.request = Some(Request::More);
            cx.notify();
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        match col_ix {
            0 => (row_ix + 1).to_string(),
            _ => self.value(row_ix, col_ix - 1).unwrap_or("NULL").to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use savoia_core::edit::Change;
    use savoia_core::{ColumnInfo, TableInfo, TableKind};

    use super::{DataRows, is_numeric_type};

    fn rows() -> DataRows {
        let mut info = TableInfo::new("t", TableKind::Table);
        info.columns = ["id", "name"]
            .iter()
            .map(|c| ColumnInfo {
                name: (*c).into(),
                data_type: "text".into(),
                nullable: true,
                default: None,
            })
            .collect();
        let mut rows = DataRows::new(&info, &["id".into()], true);
        rows.rows = vec![
            Box::new([Some("1".into()), Some("a".into())]),
            Box::new([Some("2".into()), None]),
        ];
        rows
    }

    #[test]
    fn edits_become_changes_in_order() {
        let mut rows = rows();
        rows.add_row();
        rows.set(0, 1, Some("new".into()));
        rows.set(1, 1, Some("A".into()));
        rows.set(2, 1, Some(String::new()));
        assert_eq!(rows.value(2, 1), Some(""), "empty, not NULL");
        rows.toggle_delete(1);
        let key = |id: &str| vec![("id".to_string(), Some(id.to_string()))];
        assert_eq!(
            rows.changes(),
            [
                Change::Delete { key: key("1") },
                Change::Update {
                    key: key("2"),
                    values: vec![("name".into(), Some(String::new()))]
                },
                Change::Insert {
                    values: vec![("name".into(), Some("new".into()))]
                },
            ]
        );
    }

    #[test]
    fn setting_the_old_value_drops_the_edit() {
        let mut rows = rows();
        rows.set(0, 1, Some("b".into()));
        rows.set(0, 1, Some("a".into()));
        rows.set(1, 1, None);
        assert!(!rows.has_changes());
        rows.add_row();
        rows.toggle_delete(0);
        assert!(!rows.has_changes(), "removing a new row leaves nothing");
    }

    #[test]
    fn numeric_types() {
        for t in [
            "integer",
            "bigint",
            "numeric(10,2)",
            "double precision",
            "int unsigned",
        ] {
            assert!(is_numeric_type(t), "{t}");
        }
        for t in ["text", "interval", "timestamp with time zone", "point"] {
            assert!(!is_numeric_type(t), "{t}");
        }
    }
}
