//! `CREATE TABLE` text rebuilt from the catalog's table details. It covers
//! columns, defaults, keys and indexes; storage options, checks, triggers
//! and grants aren't loaded, so it is a reading aid, not a dump.

use crate::sql_text::quote_ident;
use crate::{Engine, TableInfo, TableKind};

/// The DDL of `table`, qualified by `container` (schema on Postgres,
/// database on MySQL).
pub fn create_table(engine: Engine, container: &str, table: &TableInfo) -> String {
    let q = |ident: &str| quote_ident(engine, ident);
    let list = |names: &[String]| names.iter().map(|n| q(n)).collect::<Vec<_>>().join(", ");
    let name = format!("{}.{}", q(container), q(&table.name));
    if table.kind == TableKind::View {
        let columns: Vec<String> = table
            .columns
            .iter()
            .map(|c| format!("--   {} {}", q(&c.name), c.data_type))
            .collect();
        return format!(
            "-- View {name}. Its definition isn't loaded; its columns are:\n{}\n",
            columns.join("\n")
        );
    }

    let mut lines: Vec<String> = table
        .columns
        .iter()
        .map(|c| {
            let mut line = format!("  {} {}", q(&c.name), c.data_type);
            if !c.nullable {
                line.push_str(" NOT NULL");
            }
            if let Some(default) = &c.default {
                line.push_str(&format!(" DEFAULT {default}"));
            }
            line
        })
        .collect();
    if !table.primary_key.is_empty() {
        let constraint = table
            .indexes
            .iter()
            .find(|i| i.primary)
            .filter(|_| engine == Engine::Postgres)
            .map(|i| format!("CONSTRAINT {} ", q(&i.name)))
            .unwrap_or_default();
        lines.push(format!(
            "  {constraint}PRIMARY KEY ({})",
            list(&table.primary_key)
        ));
    }
    for fk in &table.foreign_keys {
        lines.push(format!(
            "  CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}.{} ({})",
            q(&fk.name),
            list(&fk.columns),
            q(&fk.ref_schema),
            q(&fk.ref_table),
            list(&fk.ref_columns)
        ));
    }
    let mut out = format!("CREATE TABLE {name} (\n{}\n);\n", lines.join(",\n"));
    for index in table.indexes.iter().filter(|i| !i.primary) {
        let columns: Vec<String> = index
            .columns
            .iter()
            // Expression indexes come back as their expression text.
            .map(|c| if is_plain(c) { q(c) } else { c.clone() })
            .collect();
        out.push_str(&format!(
            "CREATE {}INDEX {} ON {name} ({});\n",
            if index.unique { "UNIQUE " } else { "" },
            q(&index.name),
            columns.join(", ")
        ));
    }
    out
}

fn is_plain(column: &str) -> bool {
    !column.is_empty()
        && column
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnInfo, ForeignKey, IndexInfo};

    fn orders() -> TableInfo {
        let mut t = TableInfo::new("orders", TableKind::Table);
        t.columns = vec![
            ColumnInfo {
                name: "id".into(),
                data_type: "integer".into(),
                nullable: false,
                default: Some("nextval('orders_id_seq'::regclass)".into()),
            },
            ColumnInfo {
                name: "customer_id".into(),
                data_type: "integer".into(),
                nullable: true,
                default: None,
            },
        ];
        t.primary_key = vec!["id".into()];
        t.foreign_keys = vec![ForeignKey {
            name: "orders_customer_fk".into(),
            columns: vec!["customer_id".into()],
            ref_schema: "public".into(),
            ref_table: "customers".into(),
            ref_columns: vec!["id".into()],
        }];
        t.indexes = vec![
            IndexInfo {
                name: "orders_pkey".into(),
                columns: vec!["id".into()],
                unique: true,
                primary: true,
            },
            IndexInfo {
                name: "orders_lower".into(),
                columns: vec!["lower((note)::text)".into()],
                unique: false,
                primary: false,
            },
        ];
        t
    }

    #[test]
    fn postgres_table() {
        assert_eq!(
            create_table(Engine::Postgres, "public", &orders()),
            "CREATE TABLE \"public\".\"orders\" (\n  \
             \"id\" integer NOT NULL DEFAULT nextval('orders_id_seq'::regclass),\n  \
             \"customer_id\" integer,\n  \
             CONSTRAINT \"orders_pkey\" PRIMARY KEY (\"id\"),\n  \
             CONSTRAINT \"orders_customer_fk\" FOREIGN KEY (\"customer_id\") REFERENCES \"public\".\"customers\" (\"id\")\n\
             );\n\
             CREATE INDEX \"orders_lower\" ON \"public\".\"orders\" (lower((note)::text));\n"
        );
    }

    #[test]
    fn mysql_primary_key_has_no_name() {
        let ddl = create_table(Engine::Mysql, "shop", &orders());
        assert!(ddl.contains("  PRIMARY KEY (`id`),\n"), "{ddl}");
    }

    #[test]
    fn views_list_columns() {
        let mut view = orders();
        view.kind = TableKind::View;
        assert!(
            create_table(Engine::Postgres, "public", &view)
                .starts_with("-- View \"public\".\"orders\". Its definition isn't loaded")
        );
    }
}
