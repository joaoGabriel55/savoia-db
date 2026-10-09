//! Catalog reads from `information_schema`, every query qualified by
//! database. A MySQL database is its own one schema.

use std::collections::HashMap;

use mysql_async::Conn;
use mysql_async::prelude::Queryable;
use savoia_core::{
    AppError, AppResult, ColumnInfo, ForeignKey, IndexInfo, ObjectCounts, SchemaNode,
    SchemaObjects, TableInfo, TableKind, attach,
};

use crate::query_error;

/// The one schema of each database, with object counts, keyed by database.
pub(crate) async fn schemas(conn: &mut Conn) -> AppResult<HashMap<String, SchemaNode>> {
    let relations: Vec<(String, i64, i64, i64)> = conn
        .query(
            "SELECT TABLE_SCHEMA, \
               CAST(SUM(TABLE_TYPE NOT LIKE '%VIEW' AND TABLE_TYPE <> 'SEQUENCE') AS SIGNED), \
               CAST(SUM(TABLE_TYPE LIKE '%VIEW') AS SIGNED), \
               CAST(SUM(TABLE_TYPE = 'SEQUENCE') AS SIGNED) \
             FROM information_schema.TABLES GROUP BY TABLE_SCHEMA",
        )
        .await
        .map_err(query_error)?;
    let routines: Vec<(String, i64)> = conn
        .query(
            "SELECT ROUTINE_SCHEMA, COUNT(*) FROM information_schema.ROUTINES \
             GROUP BY ROUTINE_SCHEMA",
        )
        .await
        .map_err(query_error)?;

    let mut counts: HashMap<String, ObjectCounts> = HashMap::new();
    for (db, tables, views, sequences) in relations {
        let c = counts.entry(db).or_default();
        c.tables = tables as usize;
        c.views = views as usize;
        c.sequences = sequences as usize;
    }
    for (db, functions) in routines {
        counts.entry(db).or_default().functions = functions as usize;
    }
    Ok(counts
        .into_iter()
        .map(|(name, counts)| {
            let node = SchemaNode {
                name: name.clone(),
                counts,
                objects: None,
            };
            (name, node)
        })
        .collect())
}

pub(crate) async fn objects(conn: &mut Conn, database: &str) -> AppResult<SchemaObjects> {
    let relations: Vec<(String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
        )
        .await
        .map_err(query_error)?;
    let functions: Vec<String> = conn
        .exec(
            "SELECT ROUTINE_NAME FROM information_schema.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_NAME",
            (database,),
        )
        .await
        .map_err(query_error)?;

    let mut objects = SchemaObjects {
        functions,
        ..SchemaObjects::default()
    };
    for (name, kind) in relations {
        match kind.as_str() {
            "VIEW" | "SYSTEM VIEW" => objects.views.push(name),
            "SEQUENCE" => objects.sequences.push(name),
            _ => objects.tables.push(name),
        }
    }
    Ok(objects)
}

/// Tables and views of `database` with their columns and keys: all of them,
/// or only `table`.
pub(crate) async fn describe(
    conn: &mut Conn,
    database: &str,
    table: Option<&str>,
) -> AppResult<Vec<TableInfo>> {
    const SCOPE: &str = "TABLE_SCHEMA = ? AND (? IS NULL OR TABLE_NAME = ?)";
    let params = (database, table, table);

    let relations: Vec<(String, String)> = conn
        .exec(
            format!(
                "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
                 WHERE {SCOPE} AND TABLE_TYPE <> 'SEQUENCE' ORDER BY TABLE_NAME"
            ),
            params,
        )
        .await
        .map_err(query_error)?;
    let mut tables: Vec<TableInfo> = relations
        .into_iter()
        .map(|(name, kind)| {
            let kind = if kind.contains("VIEW") {
                TableKind::View
            } else {
                TableKind::Table
            };
            TableInfo::new(name, kind)
        })
        .collect();

    let columns: Vec<(String, String, String, String, Option<String>)> = conn
        .exec(
            format!(
                "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT \
                 FROM information_schema.COLUMNS WHERE {SCOPE} \
                 ORDER BY TABLE_NAME, ORDINAL_POSITION"
            ),
            params,
        )
        .await
        .map_err(query_error)?;
    attach(
        &mut tables,
        columns
            .into_iter()
            .map(|(table, name, data_type, nullable, default)| {
                let column = ColumnInfo {
                    name,
                    data_type,
                    nullable: nullable == "YES",
                    default,
                };
                (table, column)
            })
            .collect(),
        |t, column| t.columns.push(column),
    );

    // One row per key column, in key order.
    let fk_rows: Vec<(String, String, String, String, String, String)> = conn
        .exec(
            format!(
                "SELECT TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, \
                   REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
                 FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE {SCOPE} AND REFERENCED_TABLE_NAME IS NOT NULL \
                 ORDER BY TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION"
            ),
            params,
        )
        .await
        .map_err(query_error)?;
    let mut fks: Vec<(String, ForeignKey)> = Vec::new();
    for (table, name, column, ref_schema, ref_table, ref_column) in fk_rows {
        match fks.last_mut() {
            Some((t, fk)) if *t == table && fk.name == name => {
                fk.columns.push(column);
                fk.ref_columns.push(ref_column);
            }
            _ => fks.push((
                table,
                ForeignKey {
                    name,
                    columns: vec![column],
                    ref_schema,
                    ref_table,
                    ref_columns: vec![ref_column],
                },
            )),
        }
    }
    attach(&mut tables, fks, |t, fk| t.foreign_keys.push(fk));

    // One row per index column; functional key parts have no column name.
    let index_rows: Vec<(String, String, i64, Option<String>)> = conn
        .exec(
            format!(
                "SELECT TABLE_NAME, INDEX_NAME, NON_UNIQUE, COLUMN_NAME \
                 FROM information_schema.STATISTICS WHERE {SCOPE} \
                 ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX"
            ),
            params,
        )
        .await
        .map_err(query_error)?;
    let mut indexes: Vec<(String, IndexInfo)> = Vec::new();
    for (table, name, non_unique, column) in index_rows {
        let column = column.unwrap_or_else(|| "(expression)".into());
        match indexes.last_mut() {
            Some((t, index)) if *t == table && index.name == name => index.columns.push(column),
            _ => indexes.push((
                table,
                IndexInfo {
                    primary: name == "PRIMARY",
                    name,
                    columns: vec![column],
                    unique: non_unique == 0,
                },
            )),
        }
    }
    attach(&mut tables, indexes, |t, index| {
        if index.primary {
            t.primary_key = index.columns.clone();
        }
        t.indexes.push(index);
    });

    if let Some(table) = table
        && tables.is_empty()
    {
        return Err(AppError::query(format!(
            "table `{database}`.`{table}` does not exist"
        )));
    }
    Ok(tables)
}
