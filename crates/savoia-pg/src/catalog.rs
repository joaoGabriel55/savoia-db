//! Catalog reads from `pg_catalog`, every query qualified by schema.

use savoia_core::{
    AppError, AppResult, ColumnInfo, ForeignKey, IndexInfo, ObjectCounts, SchemaNode,
    SchemaObjects, TableInfo, TableKind, attach,
};
use tokio_postgres::Client;
use tokio_postgres::types::ToSql;

use crate::query_error;

pub(crate) const SYSTEM_SCHEMA_FILTER: &str =
    "n.nspname NOT LIKE 'pg\\_%' AND n.nspname <> 'information_schema'";

/// Relations the explorer lists as tables or views. Partitions show under
/// their parent only.
const RELATION_FILTER: &str = "c.relkind IN ('r','p','f','v','m') AND NOT c.relispartition";

/// The schemas of the current database, with object counts.
pub(crate) async fn schemas(client: &Client) -> AppResult<Vec<SchemaNode>> {
    let rows = client
        .query(
            &format!(
                "SELECT n.nspname, \
                   count(c.oid) FILTER (WHERE c.relkind IN ('r','p','f')), \
                   count(c.oid) FILTER (WHERE c.relkind IN ('v','m')), \
                   count(c.oid) FILTER (WHERE c.relkind = 'S'), \
                   (SELECT count(DISTINCT p.proname) FROM pg_proc p \
                    WHERE p.pronamespace = n.oid AND p.prokind IN ('f','p')) \
                 FROM pg_namespace n \
                 LEFT JOIN pg_class c ON c.relnamespace = n.oid AND NOT c.relispartition \
                 WHERE {SYSTEM_SCHEMA_FILTER} \
                 GROUP BY n.oid, n.nspname ORDER BY 1"
            ),
            &[],
        )
        .await
        .map_err(query_error)?;
    let count = |row: &tokio_postgres::Row, i| row.get::<_, i64>(i) as usize;
    Ok(rows
        .iter()
        .map(|row| SchemaNode {
            name: row.get(0),
            counts: ObjectCounts {
                tables: count(row, 1),
                views: count(row, 2),
                sequences: count(row, 3),
                functions: count(row, 4),
            },
            objects: None,
        })
        .collect())
}

pub(crate) async fn objects(client: &Client, schema: &str) -> AppResult<SchemaObjects> {
    let relations = client
        .query(
            "SELECT c.relname, c.relkind::text FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = $1 AND c.relkind IN ('r','p','v','m','S','f') \
               AND NOT c.relispartition \
             ORDER BY 1",
            &[&schema],
        )
        .await
        .map_err(query_error)?;
    let functions = client
        .query(
            "SELECT DISTINCT p.proname FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = $1 AND p.prokind IN ('f','p') ORDER BY 1",
            &[&schema],
        )
        .await
        .map_err(query_error)?;

    let mut objects = SchemaObjects {
        functions: functions.iter().map(|r| r.get(0)).collect(),
        ..SchemaObjects::default()
    };
    for row in relations {
        let name = row.get(0);
        match row.get::<_, &str>(1) {
            "r" | "p" | "f" => objects.tables.push(name),
            "v" | "m" => objects.views.push(name),
            "S" => objects.sequences.push(name),
            _ => {}
        }
    }
    Ok(objects)
}

/// Tables and views of `schema` with their columns and keys: all of them, or
/// only `table`.
pub(crate) async fn describe(
    client: &Client,
    schema: &str,
    table: Option<&str>,
) -> AppResult<Vec<TableInfo>> {
    let params: [&(dyn ToSql + Sync); 2] = [&schema, &table];
    let scope = "n.nspname = $1 AND ($2::text IS NULL OR c.relname = $2)";
    let query = |sql: String| async move { client.query(&sql, &params).await.map_err(query_error) };

    let mut tables: Vec<TableInfo> = query(format!(
        "SELECT c.relname, c.relkind::text FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {scope} AND {RELATION_FILTER} ORDER BY 1"
    ))
    .await?
    .iter()
    .map(|row| {
        let kind = match row.get::<_, &str>(1) {
            "v" | "m" => TableKind::View,
            _ => TableKind::Table,
        };
        TableInfo::new(row.get::<_, String>(0), kind)
    })
    .collect();

    let columns = query(format!(
        "SELECT c.relname, a.attname, format_type(a.atttypid, a.atttypmod), \
           NOT a.attnotnull, pg_get_expr(d.adbin, d.adrelid) \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
         WHERE {scope} AND {RELATION_FILTER} AND a.attnum > 0 AND NOT a.attisdropped \
         ORDER BY c.relname, a.attnum"
    ))
    .await?;
    attach(
        &mut tables,
        columns
            .iter()
            .map(|row| {
                let column = ColumnInfo {
                    name: row.get(1),
                    data_type: row.get(2),
                    nullable: row.get(3),
                    default: row.get(4),
                };
                (row.get(0), column)
            })
            .collect(),
        |t, column| t.columns.push(column),
    );

    // Column names of a key, in key order.
    let key_columns = |rel: &str, keys: &str| {
        format!(
            "ARRAY(SELECT a.attname::text FROM unnest(con.{keys}) WITH ORDINALITY k(num, ord) \
             JOIN pg_attribute a ON a.attrelid = con.{rel} AND a.attnum = k.num ORDER BY k.ord)"
        )
    };
    let constraints = query(format!(
        "SELECT c.relname, con.conname, con.contype::text, {}, fn.nspname, fc.relname, {} \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_class fc ON fc.oid = con.confrelid \
         LEFT JOIN pg_namespace fn ON fn.oid = fc.relnamespace \
         WHERE {scope} AND con.contype IN ('p','f') \
         ORDER BY c.relname, con.conname",
        key_columns("conrelid", "conkey"),
        key_columns("confrelid", "confkey"),
    ))
    .await?;
    for row in &constraints {
        let table: String = row.get(0);
        let Some(info) = tables.iter_mut().find(|t| t.name == table) else {
            continue;
        };
        let columns: Vec<String> = row.get(3);
        if row.get::<_, &str>(2) == "p" {
            info.primary_key = columns;
        } else if let (Some(ref_schema), Some(ref_table)) = (row.get(4), row.get(5)) {
            info.foreign_keys.push(ForeignKey {
                name: row.get(1),
                columns,
                ref_schema,
                ref_table,
                ref_columns: row.get(6),
            });
        }
    }

    let indexes = query(format!(
        "SELECT c.relname, ic.relname, i.indisunique, i.indisprimary, \
           ARRAY(SELECT pg_get_indexdef(i.indexrelid, k, true) \
                 FROM generate_series(1, i.indnkeyatts::int) k ORDER BY k) \
         FROM pg_index i \
         JOIN pg_class ic ON ic.oid = i.indexrelid \
         JOIN pg_class c ON c.oid = i.indrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {scope} ORDER BY 1, 2"
    ))
    .await?;
    attach(
        &mut tables,
        indexes
            .iter()
            .map(|row| {
                let index = IndexInfo {
                    name: row.get(1),
                    unique: row.get(2),
                    primary: row.get(3),
                    columns: row.get(4),
                };
                (row.get(0), index)
            })
            .collect(),
        |t, index| t.indexes.push(index),
    );

    if let Some(table) = table
        && tables.is_empty()
    {
        return Err(AppError::query(format!(
            "table \"{schema}\".\"{table}\" does not exist"
        )));
    }
    Ok(tables)
}
