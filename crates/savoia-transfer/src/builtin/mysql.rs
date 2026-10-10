//! MySQL and MariaDB DDL from `SHOW CREATE`, which is exact. Names are left
//! unqualified, like `mysqldump` without `--databases`, so the dump restores
//! into whichever database the importing session uses.

use savoia_core::sql_text::{quote_ident, quote_literal};
use savoia_core::{AppResult, Connection, Engine};

use super::{ExportRequest, Plan, TablePlan, query, text};

/// Times are read and written in UTC, as `mysqldump` does; the session's
/// zone is put back afterwards.
pub(super) const BEGIN: &str = "SET @savoia_time_zone = @@session.time_zone; \
     SET time_zone = '+00:00'; \
     SET TRANSACTION ISOLATION LEVEL REPEATABLE READ; \
     START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY";
pub(super) const END: &str = "COMMIT; SET time_zone = @savoia_time_zone";

fn ident(name: &str) -> String {
    quote_ident(Engine::Mysql, name)
}

pub(super) async fn plan(conn: &dyn Connection, request: &ExportRequest) -> AppResult<Plan> {
    let db = ident(&request.schema);
    let schema = quote_literal(Engine::Mysql, &request.schema);
    let mut plan = Plan {
        header: vec![
            "SET NAMES utf8mb4;".into(),
            "SET time_zone = '+00:00';".into(),
            // Known modes: no NO_BACKSLASH_ESCAPES, and explicit zeros stay zeros.
            "SET SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO';".into(),
            "SET FOREIGN_KEY_CHECKS = 0;".into(),
            "SET UNIQUE_CHECKS = 0;".into(),
        ],
        footer: vec![
            "SET FOREIGN_KEY_CHECKS = 1;".into(),
            "SET UNIQUE_CHECKS = 1;".into(),
        ],
        ..Plan::default()
    };

    let objects = query(
        conn,
        &format!(
            "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = {schema} ORDER BY TABLE_NAME"
        ),
    )
    .await?;
    // Generated columns can't be inserted into.
    let columns = query(
        conn,
        &format!(
            "SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = {schema} AND COALESCE(GENERATION_EXPRESSION, '') = '' \
             ORDER BY TABLE_NAME, ORDINAL_POSITION"
        ),
    )
    .await?;

    let mut views = Vec::new();
    let mut table_drops = Vec::new();
    for row in objects.iter().filter(|row| request.includes(text(row, 0))) {
        let (name, kind) = (text(row, 0), text(row, 1));
        let qualified = format!("{db}.{}", ident(name));
        match kind {
            "BASE TABLE" => {
                let create = query(conn, &format!("SHOW CREATE TABLE {qualified}")).await?;
                let insertable: Vec<String> = columns
                    .iter()
                    .filter(|c| text(c, 0) == name)
                    .map(|c| ident(text(c, 1)))
                    .collect();
                table_drops.push(format!("DROP TABLE IF EXISTS {};", ident(name)));
                plan.tables.push(TablePlan {
                    name: name.to_owned(),
                    create: Some(format!("{};", text(&create[0], 1))),
                    select: format!("SELECT {} FROM {qualified}", insertable.join(", ")),
                    insert: format!(
                        "INSERT INTO {} ({}) VALUES",
                        ident(name),
                        insertable.join(", ")
                    ),
                });
            }
            "VIEW" => {
                let create = query(conn, &format!("SHOW CREATE VIEW {qualified}")).await?;
                // Restore the session collation the view was created under.
                let literal = |text: &str| quote_literal(Engine::Mysql, text);
                views.push((
                    name.to_owned(),
                    format!(
                        "SET character_set_client = {};\nSET collation_connection = {};\n{};",
                        literal(text(&create[0], 2)),
                        literal(text(&create[0], 3)),
                        strip_definer(text(&create[0], 1))
                    ),
                ));
                plan.tables.push(TablePlan {
                    name: name.to_owned(),
                    create: None,
                    select: format!("SELECT * FROM {qualified}"),
                    insert: String::new(),
                });
            }
            other => plan
                .skipped
                .push(format!("{name} ({})", other.to_lowercase())),
        }
    }

    let views = order_views(views);
    for (_, create) in &views {
        plan.after.push(create.clone());
    }
    if !views.is_empty() {
        plan.after.push("SET NAMES utf8mb4;".into());
    }
    plan.drops = views
        .iter()
        .rev()
        .map(|(name, _)| format!("DROP VIEW IF EXISTS {};", ident(name)))
        .chain(table_drops)
        .collect();
    Ok(plan)
}

/// Drops the `DEFINER=`user`@`host`` clause, which names an account the
/// target server may not have; the importing user becomes the definer.
fn strip_definer(create: &str) -> String {
    let Some(start) = create.find(" DEFINER=") else {
        return create.to_owned();
    };
    let rest = &create[start + " DEFINER=".len()..];
    let Some(after_user) = skip_quoted(rest) else {
        return create.to_owned();
    };
    let Some(after_host) = after_user.strip_prefix('@').and_then(skip_quoted) else {
        return create.to_owned();
    };
    format!("{}{after_host}", &create[..start])
}

/// The text after one backtick-quoted identifier at the start of `text`.
fn skip_quoted(text: &str) -> Option<&str> {
    let mut chars = text.strip_prefix('`')?.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '`' {
            if chars.peek().is_some_and(|(_, next)| *next == '`') {
                chars.next();
                continue;
            }
            return Some(&text[i + 2..]);
        }
    }
    None
}

/// Orders views so each comes after the views its body names.
fn order_views(mut pending: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut ordered: Vec<(String, String)> = Vec::new();
    while !pending.is_empty() {
        let ready = pending.iter().position(|(name, body)| {
            !pending
                .iter()
                .any(|(other, _)| other != name && mentions(body, other))
        });
        // A cycle can't be created in MySQL, but don't loop forever on a misread.
        let next = pending.remove(ready.unwrap_or(0));
        ordered.push(next);
    }
    ordered
}

/// Whether `body` references view `name` anywhere but its own header.
fn mentions(body: &str, name: &str) -> bool {
    let quoted = ident(name);
    let body = body.split_once(" AS ").map_or(body, |(_, select)| select);
    body.contains(&quoted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_definer() {
        assert_eq!(
            strip_definer(
                "CREATE ALGORITHM=UNDEFINED DEFINER=`app``x`@`%` SQL SECURITY DEFINER VIEW `v` AS select 1"
            ),
            "CREATE ALGORITHM=UNDEFINED SQL SECURITY DEFINER VIEW `v` AS select 1"
        );
        assert_eq!(
            strip_definer("CREATE VIEW `v` AS select 1"),
            "CREATE VIEW `v` AS select 1"
        );
    }

    #[test]
    fn views_come_after_the_views_they_read() {
        let views = vec![
            (
                "a_top".to_owned(),
                "CREATE VIEW `a_top` AS select * from `b_base`".to_owned(),
            ),
            (
                "b_base".to_owned(),
                "CREATE VIEW `b_base` AS select * from `t`".to_owned(),
            ),
        ];
        let names: Vec<String> = order_views(views)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["b_base", "a_top"]);
    }
}
