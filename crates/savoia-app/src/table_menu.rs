//! Right-click menu for tables and views in the Database Explorer.
//!
//! The menu opens with an identity header (what you are about to touch, on
//! which connection, and whether it is read-only), then short groups of
//! actions with the most used first and destructive ones last. Actions from
//! later milestones stay visible but disabled, tagged with their milestone.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use savoia_core::{ConnectionId, Engine};

/// Rows "Open data" loads.
pub const PREVIEW_ROWS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Table,
    View,
}

impl ObjectKind {
    pub fn noun(self) -> &'static str {
        match self {
            ObjectKind::Table => "table",
            ObjectKind::View => "view",
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            ObjectKind::Table => "TABLE",
            ObjectKind::View => "VIEW",
        }
    }
}

/// A table or view picked in the explorer tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRef {
    pub connection: ConnectionId,
    pub engine: Engine,
    pub kind: ObjectKind,
    pub database: String,
    /// Postgres only; for MySQL the database is the schema.
    pub schema: Option<String>,
    pub name: String,
}

impl ObjectRef {
    /// Reads a tree id: `table:<conn>/<db>/<schema>/<name>` on Postgres,
    /// `table:<conn>/<db>/<name>` on MySQL. The name is the remainder, so it
    /// may itself contain `/`.
    pub fn parse(id: &str, engine: Engine) -> Option<Self> {
        let (kind, rest) = id.split_once(':')?;
        let kind = match kind {
            "table" => ObjectKind::Table,
            "view" => ObjectKind::View,
            _ => return None,
        };
        let parts = if engine.has_schemas() { 4 } else { 3 };
        let mut it = rest.splitn(parts, '/');
        let connection = it.next()?.parse().ok().map(ConnectionId)?;
        let database = it.next()?.to_owned();
        let schema = if engine.has_schemas() {
            Some(it.next()?.to_owned())
        } else {
            None
        };
        let name = it.next()?.to_owned();
        Some(Self {
            connection,
            engine,
            kind,
            database,
            schema,
            name,
        })
    }

    /// The container the name is qualified with: the schema on Postgres, the
    /// database on MySQL.
    fn container(&self) -> &str {
        self.schema.as_deref().unwrap_or(&self.database)
    }

    /// Always quoted, for generated SQL.
    pub fn qualified_sql(&self) -> String {
        format!(
            "{}.{}",
            quote(self.engine, self.container()),
            quote(self.engine, &self.name)
        )
    }

    /// Quoted only where needed, for pasting by hand.
    pub fn qualified_name(&self) -> String {
        format!(
            "{}.{}",
            quote_if_needed(self.engine, self.container()),
            quote_if_needed(self.engine, &self.name)
        )
    }

    pub fn select_sql(&self) -> String {
        format!(
            "SELECT * FROM {} LIMIT {PREVIEW_ROWS};",
            self.qualified_sql()
        )
    }

    pub fn truncate_sql(&self) -> String {
        format!("TRUNCATE TABLE {};", self.qualified_sql())
    }

    pub fn drop_sql(&self) -> String {
        format!("DROP {} {};", self.kind.keyword(), self.qualified_sql())
    }

    /// "shop › public" on Postgres, "shop" on MySQL.
    fn path(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{} › {schema}", self.database),
            None => self.database.clone(),
        }
    }
}

fn quote(engine: Engine, ident: &str) -> String {
    match engine {
        Engine::Postgres => format!("\"{}\"", ident.replace('"', "\"\"")),
        Engine::Mysql => format!("`{}`", ident.replace('`', "``")),
    }
}

fn quote_if_needed(engine: Engine, ident: &str) -> String {
    let mut chars = ident.chars();
    let plain = match engine {
        // Unquoted Postgres identifiers fold to lower case.
        Engine::Postgres => {
            chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        Engine::Mysql => {
            !ident.is_empty()
                && !ident.chars().all(|c| c.is_ascii_digit())
                && ident
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        }
    };
    if plain {
        ident.to_owned()
    } else {
        quote(engine, ident)
    }
}

/// What the menu asks the explorer to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAction {
    OpenData,
    ShowDiagram,
    NewSelect,
    CopyName,
    CopyQualifiedName,
    Truncate,
    Drop,
}

/// The connection the object lives on, for the header.
pub struct Origin {
    pub name: String,
    pub color: Option<u32>,
    pub read_only: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Normal,
    Danger,
}

/// Builds the menu. `on_action` runs the picked action.
pub fn build(
    menu: PopupMenu,
    object: &ObjectRef,
    origin: &Origin,
    on_action: impl Fn(TableAction, &mut Window, &mut App) + 'static,
) -> PopupMenu {
    let on_action = std::rc::Rc::new(on_action);
    let item = |icon: Icon, label: String, hint: Option<String>, tone: Tone, action| {
        let on_action = on_action.clone();
        entry(icon, label, hint, tone, false)
            .on_click(move |_, window, cx| on_action(action, window, cx))
    };
    let later = |icon: Icon, label: &str, milestone: &str| {
        entry(
            icon,
            label.into(),
            Some(milestone.into()),
            Tone::Normal,
            true,
        )
    };
    let danger = |icon: Icon, label: String, action| {
        if origin.read_only {
            entry(icon, label, Some("read-only".into()), Tone::Danger, true)
        } else {
            item(icon, label, None, Tone::Danger, action)
        }
    };

    let is_table = object.kind == ObjectKind::Table;
    menu.min_w(px(280.))
        .item(header(object, origin))
        .separator()
        .item(item(
            Icon::new(IconName::Play),
            "Open data".into(),
            Some(format!("first {PREVIEW_ROWS} rows")),
            Tone::Normal,
            TableAction::OpenData,
        ))
        .item(item(
            Icon::new(Lucide::Workflow),
            "Show diagram".into(),
            None,
            Tone::Normal,
            TableAction::ShowDiagram,
        ))
        .item(later(Icon::new(Lucide::TableProperties), "Structure", "M2"))
        .separator()
        .item(item(
            Icon::new(Lucide::SquarePen),
            "New SELECT in console".into(),
            None,
            Tone::Normal,
            TableAction::NewSelect,
        ))
        .item(item(
            Icon::new(IconName::Copy),
            "Copy name".into(),
            None,
            Tone::Normal,
            TableAction::CopyName,
        ))
        .item(item(
            Icon::new(IconName::Copy),
            "Copy qualified name".into(),
            Some(object.qualified_name()),
            Tone::Normal,
            TableAction::CopyQualifiedName,
        ))
        .separator()
        .item(later(Icon::new(Lucide::Download), "Export…", "M4"))
        .when(is_table, |menu| {
            menu.item(later(Icon::new(Lucide::Upload), "Import…", "M4"))
        })
        .separator()
        .when(is_table, |menu| {
            menu.item(danger(
                Icon::new(Lucide::Eraser),
                "Truncate…".into(),
                TableAction::Truncate,
            ))
        })
        .item(danger(
            Icon::new(Lucide::Trash),
            format!("Drop {}…", object.kind.noun()),
            TableAction::Drop,
        ))
}

/// Not clickable: names the object, where it lives, and on which connection.
fn header(object: &ObjectRef, origin: &Origin) -> PopupMenuItem {
    let name = object.name.clone();
    let kind = object.kind;
    let path = object.path();
    let connection = origin.name.clone();
    let color = origin.color;
    let read_only = origin.read_only;
    PopupMenuItem::element(move |_, cx| {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let icon = match kind {
            ObjectKind::Table => Icon::new(Lucide::Table),
            ObjectKind::View => Icon::new(IconName::Eye),
        };
        v_flex()
            .w_full()
            .py_1()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_2()
                    .child(icon.small().text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_semibold()
                            .text_color(theme.foreground)
                            .child(name.clone()),
                    )
                    .child(div().text_xs().text_color(muted).child(kind.noun())),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .text_xs()
                    .text_color(muted)
                    // Lines up with the name, past the icon.
                    .pl(px(22.))
                    .child(path.clone())
                    .child("·")
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(color.map_or(muted, |c| rgb(c).into())),
                    )
                    .child(connection.clone())
                    .when(read_only, |el| {
                        el.child(div().text_color(theme.warning).child("read-only"))
                    }),
            )
    })
    .disabled(true)
}

/// One action row: icon and label on the left, a muted hint on the right.
fn entry(
    icon: Icon,
    label: String,
    hint: Option<String>,
    tone: Tone,
    disabled: bool,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (ink, icon_ink) = match (tone, disabled) {
            (_, true) => (muted, muted.opacity(0.6)),
            (Tone::Danger, false) => (theme.danger, theme.danger),
            (Tone::Normal, false) => (theme.foreground, muted),
        };
        h_flex()
            .w_full()
            .gap_2()
            .child(icon.clone().small().text_color(icon_ink))
            .child(div().flex_1().text_color(ink).child(label.clone()))
            .when_some(hint.clone(), |el, hint| {
                el.child(
                    div()
                        .ml_4()
                        .max_w(px(160.))
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(hint),
                )
            })
    })
    .disabled(disabled)
}

#[cfg(test)]
mod tests {
    use savoia_core::{ConnectionId, Engine};

    use super::{ObjectKind, ObjectRef};

    #[test]
    fn parses_postgres_and_mysql_ids() {
        let id = ConnectionId::new();
        let pg =
            ObjectRef::parse(&format!("table:{id}/shop/public/orders"), Engine::Postgres).unwrap();
        assert_eq!(pg.schema.as_deref(), Some("public"));
        assert_eq!(pg.name, "orders");
        assert_eq!(pg.kind, ObjectKind::Table);

        let my = ObjectRef::parse(&format!("view:{id}/shop/a/b"), Engine::Mysql).unwrap();
        assert_eq!(my.database, "shop");
        assert_eq!(my.schema, None);
        assert_eq!(my.name, "a/b");
        assert_eq!(my.kind, ObjectKind::View);

        assert!(ObjectRef::parse(&format!("schema:{id}/shop/public"), Engine::Postgres).is_none());
    }

    #[test]
    fn generates_quoted_sql() {
        let id = ConnectionId::new();
        let pg =
            ObjectRef::parse(&format!("table:{id}/db/public/Order\"s"), Engine::Postgres).unwrap();
        assert_eq!(
            pg.select_sql(),
            "SELECT * FROM \"public\".\"Order\"\"s\" LIMIT 200;"
        );
        assert_eq!(pg.qualified_name(), "public.\"Order\"\"s\"");
        assert_eq!(
            pg.truncate_sql(),
            "TRUNCATE TABLE \"public\".\"Order\"\"s\";"
        );

        let my = ObjectRef::parse(&format!("view:{id}/shop/daily_sales"), Engine::Mysql).unwrap();
        assert_eq!(my.drop_sql(), "DROP VIEW `shop`.`daily_sales`;");
        assert_eq!(my.qualified_name(), "shop.daily_sales");
    }
}
