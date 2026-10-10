//! The data view's grid: rows loaded from the server plus the pending
//! changes on them. New rows show first, then loaded rows; edited cells,
//! new rows and rows marked for deletion are tinted until committed.
//!
//! Only base-table columns edit. Columns from related tables (lookups and
//! summaries) are read-only; the row menu opens the related rows instead.

use std::collections::{BTreeMap, BTreeSet};

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::Row;
use savoia_core::edit::{Change, Value};

/// What the grid asks of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Sort by a data column (0-based), descending or not; `None` resets.
    Sort(Option<(usize, bool)>),
    More,
    /// Open the rows related to a grid row: `relations[ix]`.
    Open {
        row: usize,
        relation: usize,
    },
    /// Choose a value for a foreign-key cell from the referenced table.
    Pick {
        row: usize,
        col: usize,
    },
}

/// A grid row: a new one (index into `inserted`) or a loaded one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    New(usize),
    Loaded(usize),
}

/// One data column of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridColumn {
    pub label: String,
    /// The base-table column it shows, if it is one: only those edit.
    pub base: Option<String>,
    pub numeric: bool,
    /// The referenced table's name, when the column is a single-column
    /// foreign key whose value can be picked from it.
    pub picks_from: Option<String>,
}

/// Rows related to a grid row, offered in its menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub label: String,
    /// Grid columns (0-based) whose values select the related rows.
    pub columns: Vec<usize>,
}

pub struct DataRows {
    columns: Vec<Column>,
    data: Vec<GridColumn>,
    pub(crate) rows: Vec<Row>,
    /// Another page exists on the server.
    pub(crate) more: bool,
    pub(crate) loading: bool,
    pub(crate) request: Option<Request>,
    /// Data columns (0-based) of the row's key.
    key: Vec<usize>,
    pub(crate) editable: bool,
    pub(crate) relations: Vec<Relation>,
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
    pub fn new(data: Vec<GridColumn>, key: &[String], editable: bool) -> Self {
        let columns = std::iter::once(
            Column::new("#", "")
                .width(px(52.))
                .text_right()
                .fixed_left()
                .resizable(false)
                .selectable(false),
        )
        .chain(data.iter().map(|c| {
            let column =
                Column::new(SharedString::from(c.label.clone()), c.label.clone()).sortable();
            if c.numeric {
                column.text_right()
            } else {
                column
            }
        }))
        .collect();
        let key = key
            .iter()
            .filter_map(|k| data.iter().position(|c| c.base.as_ref() == Some(k)))
            .collect();
        Self {
            columns,
            data,
            rows: Vec::new(),
            more: false,
            loading: true,
            request: None,
            key,
            editable,
            relations: Vec::new(),
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

    /// Whether a cell can take an edit.
    pub fn can_edit(&self, grid_row: usize, col: usize) -> bool {
        let deleted =
            matches!(self.source(grid_row), Source::Loaded(i) if self.deleted.contains(&i));
        self.editable && !deleted && self.data.get(col).is_some_and(|c| c.base.is_some())
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
    /// server has drops the edit. Read-only columns ignore it.
    pub fn set(&mut self, grid_row: usize, col: usize, value: Value) {
        if self.data.get(col).is_none_or(|c| c.base.is_none()) {
            return;
        }
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
        self.inserted.insert(0, vec![None; self.data.len()]);
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

    fn base(&self, col: usize) -> String {
        self.data[col].base.clone().unwrap_or_default()
    }

    fn key_of(&self, loaded: usize) -> Vec<(String, Value)> {
        self.key
            .iter()
            .map(|&col| {
                (
                    self.base(col),
                    self.rows[loaded][col].as_deref().map(str::to_owned),
                )
            })
            .collect()
    }

    /// The loaded values of `columns` in a grid row, for opening related
    /// rows; `None` for a new row.
    pub fn values(&self, grid_row: usize, columns: &[usize]) -> Option<Vec<Value>> {
        let Source::Loaded(i) = self.source(grid_row) else {
            return None;
        };
        Some(
            columns
                .iter()
                .map(|&c| self.rows[i][c].as_deref().map(str::to_owned))
                .collect(),
        )
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
                    .push((self.base(*col), value.clone()));
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
                    .map(|(col, v)| (self.base(col), v.clone()))
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
        let related = col_ix > 0 && self.data[col_ix - 1].base.is_none();
        let tint = match source {
            Source::New(_) => Some(theme.success.opacity(0.14)),
            _ if deleted => Some(theme.danger.opacity(0.14)),
            Source::Loaded(i) if col_ix > 0 && self.edits.contains_key(&(i, col_ix - 1)) => {
                Some(theme.warning.opacity(0.2))
            }
            _ if related => Some(theme.muted.opacity(0.5)),
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
        let cell = cell.when(self.data[col].numeric, |c| c.justify_end());
        match self.value(row_ix, col) {
            Some(text) => cell
                .child(SharedString::from(text.to_owned()))
                .into_any_element(),
            None => cell
                .text_color(muted)
                .child(match source {
                    Source::New(_) if !related => "default",
                    _ => "NULL",
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
        let table = cx.entity().downgrade();
        let act = |f: Box<dyn Fn(&mut DataRows)>| {
            let table = table.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                table
                    .update(cx, |t, cx| {
                        f(t.delegate_mut());
                        t.refresh(cx);
                        cx.notify();
                    })
                    .ok();
            }
        };
        let source = self.source(row_ix);
        let cell = self
            .selected
            .filter(|(row, _)| *row == row_ix)
            .map(|(_, col)| col);
        let mut menu = menu;

        // Related rows: drill down to children, or open a referenced row.
        if matches!(source, Source::Loaded(_)) {
            for (relation, r) in self.relations.iter().enumerate() {
                menu = menu.item(PopupMenuItem::new(r.label.clone()).on_click(act(Box::new(
                    move |rows| {
                        rows.request = Some(Request::Open {
                            row: row_ix,
                            relation,
                        })
                    },
                ))));
            }
        }
        if !self.editable {
            return menu;
        }
        if !self.relations.is_empty() {
            menu = menu.separator();
        }
        if let Some(col) = cell.filter(|&c| self.can_edit(row_ix, c)) {
            if let Some(table) = self.data[col].picks_from.clone() {
                menu = menu.item(
                    PopupMenuItem::new(format!("Choose from {table}…")).on_click(act(Box::new(
                        move |rows| rows.request = Some(Request::Pick { row: row_ix, col }),
                    ))),
                );
            }
            menu = menu.item(
                PopupMenuItem::new(format!("Set {} to NULL", self.data[col].label))
                    .on_click(act(Box::new(move |rows| rows.set(row_ix, col, None)))),
            );
        }
        let delete_label = match source {
            Source::New(_) => "Remove new row",
            Source::Loaded(i) if self.deleted.contains(&i) => "Keep row",
            Source::Loaded(_) => "Delete row",
        };
        menu.item(
            PopupMenuItem::new(delete_label)
                .icon(IconName::Delete)
                .on_click(act(Box::new(move |rows| rows.toggle_delete(row_ix)))),
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

    use super::{DataRows, GridColumn, is_numeric_type};

    fn column(label: &str, base: bool) -> GridColumn {
        GridColumn {
            label: label.into(),
            base: base.then(|| label.into()),
            numeric: false,
            picks_from: None,
        }
    }

    fn rows() -> DataRows {
        let columns = vec![
            column("id", true),
            column("name", true),
            column("customer › name", false),
        ];
        let mut rows = DataRows::new(columns, &["id".into()], true);
        rows.rows = vec![
            Box::new([Some("1".into()), Some("a".into()), Some("x".into())]),
            Box::new([Some("2".into()), None, None]),
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
    fn related_columns_never_edit() {
        let mut rows = rows();
        assert!(!rows.can_edit(0, 2));
        rows.set(0, 2, Some("y".into()));
        assert!(!rows.has_changes());
        assert_eq!(rows.values(1, &[0, 2]), Some(vec![Some("2".into()), None]));
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
