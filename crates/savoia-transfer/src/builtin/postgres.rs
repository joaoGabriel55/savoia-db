//! Postgres DDL from the server's own deparsers (`pg_get_constraintdef`,
//! `pg_get_indexdef`, `pg_get_viewdef`), read with an empty `search_path` so
//! every name they print is schema-qualified, as `pg_dump` does.

use savoia_core::sql_text::{quote_ident, quote_literal};
use savoia_core::{AppResult, Connection, Engine, Row};

use super::{ExportRequest, Plan, TablePlan, query, text, truthy};

pub(super) const BEGIN: &str =
    "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL search_path = ''";
pub(super) const END: &str = "COMMIT";

fn ident(name: &str) -> String {
    quote_ident(Engine::Postgres, name)
}

fn literal(text: &str) -> String {
    quote_literal(Engine::Postgres, text)
}

pub(super) async fn plan(conn: &dyn Connection, request: &ExportRequest) -> AppResult<Plan> {
    let schema = ident(&request.schema);
    let nsp = literal(&request.schema);
    let qualified = |name: &str| format!("{schema}.{}", ident(name));
    let mut plan = Plan {
        header: vec![
            "SET client_encoding = 'UTF8';".into(),
            "SET standard_conforming_strings = on;".into(),
            "SET check_function_bodies = false;".into(),
        ],
        before: vec![format!("CREATE SCHEMA IF NOT EXISTS {schema};")],
        ..Plan::default()
    };

    // Enum types: columns may use them.
    let enums = query(
        conn,
        &format!(
            "SELECT t.typname, string_agg(quote_literal(e.enumlabel), ', ' ORDER BY e.enumsortorder) \
             FROM pg_type t \
             JOIN pg_namespace n ON n.oid = t.typnamespace \
             JOIN pg_enum e ON e.enumtypid = t.oid \
             WHERE n.nspname = {nsp} \
             GROUP BY t.typname ORDER BY t.typname"
        ),
    )
    .await?;
    let mut type_drops = Vec::new();
    for row in &enums {
        let name = qualified(text(row, 0));
        plan.before
            .push(format!("CREATE TYPE {name} AS ENUM ({});", text(row, 1)));
        type_drops.push(format!("DROP TYPE IF EXISTS {name};"));
    }

    // Sequences, except those behind identity columns, which come with their table.
    let sequences = query(
        conn,
        &format!(
            "SELECT c.relname, format_type(s.seqtypid, NULL), s.seqstart, s.seqmin, s.seqmax, \
               s.seqincrement, s.seqcycle, ps.last_value, ot.relname, oa.attname \
             FROM pg_sequence s \
             JOIN pg_class c ON c.oid = s.seqrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_sequences ps ON ps.schemaname = n.nspname AND ps.sequencename = c.relname \
             LEFT JOIN pg_depend d ON d.objid = c.oid AND d.classid = 'pg_class'::regclass \
               AND d.refclassid = 'pg_class'::regclass AND d.deptype IN ('a', 'i') \
             LEFT JOIN pg_class ot ON ot.oid = d.refobjid AND ot.relnamespace = n.oid \
             LEFT JOIN pg_attribute oa ON oa.attrelid = ot.oid AND oa.attnum = d.refobjsubid \
             WHERE n.nspname = {nsp} AND d.deptype IS DISTINCT FROM 'i' \
             ORDER BY c.relname"
        ),
    )
    .await?;
    let mut sequence_drops = Vec::new();
    let mut sequence_after = Vec::new();
    for row in &sequences {
        let name = qualified(text(row, 0));
        plan.before.push(format!(
            "CREATE SEQUENCE {name} AS {} INCREMENT BY {} MINVALUE {} MAXVALUE {} START WITH {}{};",
            text(row, 1),
            text(row, 5),
            text(row, 3),
            text(row, 4),
            text(row, 2),
            if truthy(row, 6) { " CYCLE" } else { "" },
        ));
        sequence_drops.push(format!("DROP SEQUENCE IF EXISTS {name};"));
        if !text(row, 7).is_empty() {
            sequence_after.push(format!(
                "SELECT pg_catalog.setval({}, {}, true);",
                literal(&name),
                text(row, 7)
            ));
        }
        let (owner, column) = (text(row, 8), text(row, 9));
        if !owner.is_empty() && !column.is_empty() && request.includes(owner) {
            sequence_after.push(format!(
                "ALTER SEQUENCE {name} OWNED BY {}.{};",
                qualified(owner),
                ident(column)
            ));
        }
    }

    // What the exporter can't write.
    let unsupported = query(
        conn,
        &format!(
            "SELECT c.relname, CASE WHEN c.relkind = 'p' OR c.relispartition THEN 'partitioned table' \
               ELSE 'foreign table' END \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {nsp} AND (c.relkind IN ('p', 'f') OR c.relispartition) \
             ORDER BY c.relname"
        ),
    )
    .await?;
    plan.skipped = unsupported
        .iter()
        .filter(|row| request.includes(text(row, 0)))
        .map(|row| format!("{} ({})", text(row, 0), text(row, 1)))
        .collect();

    // Tables, one row per column.
    let columns = query(
        conn,
        &format!(
            "SELECT c.relname, a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, \
               pg_get_expr(d.adbin, d.adrelid), a.attidentity, a.attgenerated \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
             WHERE n.nspname = {nsp} AND c.relkind = 'r' AND NOT c.relispartition \
               AND a.attnum > 0 AND NOT a.attisdropped \
             ORDER BY c.relname, a.attnum"
        ),
    )
    .await?;
    let mut identity_after = Vec::new();
    let mut table_drops = Vec::new();
    for table_rows in group_by_table(&columns) {
        let table = text(table_rows[0], 0);
        if !request.includes(table) {
            continue;
        }
        let name = qualified(table);
        let mut lines = Vec::new();
        let mut insertable = Vec::new();
        let mut overriding = false;
        for row in &table_rows {
            let column = ident(text(row, 1));
            let mut line = format!("    {column} {}", text(row, 2));
            let (default, identity, generated) = (text(row, 4), text(row, 5), text(row, 6));
            match generated {
                "s" => line.push_str(&format!(" GENERATED ALWAYS AS ({default}) STORED")),
                "v" => line.push_str(&format!(" GENERATED ALWAYS AS ({default}) VIRTUAL")),
                _ if !default.is_empty() => line.push_str(&format!(" DEFAULT {default}")),
                _ => {}
            }
            match identity {
                "a" => line.push_str(" GENERATED ALWAYS AS IDENTITY"),
                "d" => line.push_str(" GENERATED BY DEFAULT AS IDENTITY"),
                _ => {}
            }
            if truthy(row, 3) {
                line.push_str(" NOT NULL");
            }
            lines.push(line);
            if generated.is_empty() {
                insertable.push(column.clone());
            }
            if identity == "a" {
                overriding = true;
            }
            if !identity.is_empty() {
                // Move the identity's sequence past the restored rows.
                identity_after.push(format!(
                    "SELECT pg_catalog.setval(pg_catalog.pg_get_serial_sequence({}, {}), \
                     coalesce(max({column}), 1), max({column}) IS NOT NULL) FROM {name};",
                    literal(&name),
                    literal(text(row, 1)),
                ));
            }
        }
        table_drops.push(format!("DROP TABLE IF EXISTS {name} CASCADE;"));
        plan.tables.push(TablePlan {
            name: table.to_owned(),
            create: Some(format!("CREATE TABLE {name} (\n{}\n);", lines.join(",\n"))),
            select: format!("SELECT {} FROM {name}", insertable.join(", ")),
            insert: format!(
                "INSERT INTO {name} ({}){} VALUES",
                insertable.join(", "),
                if overriding {
                    " OVERRIDING SYSTEM VALUE"
                } else {
                    ""
                }
            ),
        });
    }

    // Constraints after the rows: loading is faster and order doesn't matter.
    let constraints = query(
        conn,
        &format!(
            "SELECT t.relname, c.conname, pg_get_constraintdef(c.oid) \
             FROM pg_constraint c \
             JOIN pg_class t ON t.oid = c.conrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             WHERE n.nspname = {nsp} AND t.relkind = 'r' AND NOT t.relispartition \
               AND c.contype IN ('p', 'u', 'c', 'x', 'f') AND c.conislocal \
             ORDER BY c.contype = 'f', t.relname, c.conname"
        ),
    )
    .await?;
    let indexes = query(
        conn,
        &format!(
            "SELECT t.relname, pg_get_indexdef(i.indexrelid) \
             FROM pg_index i \
             JOIN pg_class t ON t.oid = i.indrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             WHERE n.nspname = {nsp} AND t.relkind = 'r' AND NOT t.relispartition \
               AND NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conindid = i.indexrelid \
                 AND c.conrelid = i.indrelid AND c.contype IN ('p', 'u', 'x')) \
             ORDER BY t.relname, 2"
        ),
    )
    .await?;
    let views = query(
        conn,
        &format!(
            "SELECT c.relname, c.relkind, pg_get_viewdef(c.oid) \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {nsp} AND c.relkind IN ('v', 'm') \
             ORDER BY c.oid"
        ),
    )
    .await?;

    plan.after.extend(sequence_after);
    plan.after.extend(identity_after);
    for row in constraints
        .iter()
        .filter(|row| request.includes(text(row, 0)))
    {
        plan.after.push(format!(
            "ALTER TABLE {} ADD CONSTRAINT {} {};",
            qualified(text(row, 0)),
            ident(text(row, 1)),
            text(row, 2)
        ));
    }
    for row in indexes.iter().filter(|row| request.includes(text(row, 0))) {
        plan.after.push(format!("{};", text(row, 1)));
    }
    let mut view_drops = Vec::new();
    for row in views.iter().filter(|row| request.includes(text(row, 0))) {
        let (view, materialized) = (text(row, 0), text(row, 1) == "m");
        let name = qualified(view);
        let body = text(row, 2).trim().trim_end_matches(';');
        let kind = if materialized {
            "MATERIALIZED VIEW"
        } else {
            "VIEW"
        };
        plan.after.push(format!("CREATE {kind} {name} AS\n{body};"));
        view_drops.push(format!("DROP {kind} IF EXISTS {name} CASCADE;"));
        plan.tables.push(TablePlan {
            name: view.to_owned(),
            create: None,
            select: format!("SELECT * FROM {name}"),
            insert: String::new(),
        });
    }

    // Dependents first.
    plan.drops = view_drops
        .into_iter()
        .rev()
        .chain(table_drops)
        .chain(sequence_drops)
        .chain(type_drops)
        .collect();
    Ok(plan)
}

/// Consecutive rows that share column 0, the table name.
fn group_by_table(rows: &[Row]) -> Vec<Vec<&Row>> {
    let mut groups: Vec<Vec<&Row>> = Vec::new();
    for row in rows {
        match groups.last_mut() {
            Some(group) if text(group[0], 0) == text(row, 0) => group.push(row),
            _ => groups.push(vec![row]),
        }
    }
    groups
}
