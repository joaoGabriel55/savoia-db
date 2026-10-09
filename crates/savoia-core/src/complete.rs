//! Schema-aware SQL completion, without a parser: a light scan of the
//! statement at the caret finds the tables it uses (with their aliases) and
//! what the caret follows, then suggests schemas, tables, columns or whole
//! JOIN clauses built from foreign keys.
//!
//! It works from a [`CompletionData`] snapshot of what the catalog has
//! loaded. [`needs`] says which parts are missing, so the caller can load
//! them and ask again.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use crate::sql_text::quote_ident_if_needed;
use crate::{Engine, TableInfo, split};

/// What the catalog has loaded, by schema (the database on MySQL).
#[derive(Debug, Clone, Default)]
pub struct CompletionData {
    pub engine: Option<Engine>,
    pub schemas: Vec<String>,
    /// Where unqualified names are looked up first.
    pub default_schema: Option<String>,
    /// Table and view names, for schemas whose objects are loaded.
    pub objects: HashMap<String, Vec<String>>,
    /// Columns and keys, for tables that have been described.
    pub tables: HashMap<(String, String), Arc<TableInfo>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Keyword,
    Schema,
    Table,
    Column,
    Join,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    /// Text that replaces the word at the caret.
    pub insert: String,
    pub kind: Kind,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completions {
    /// The partial word at the caret, which a pick replaces.
    pub replace: Range<usize>,
    pub items: Vec<Completion>,
}

/// Catalog parts that would improve the completions at `offset`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Needs {
    /// Schemas whose object names aren't loaded.
    pub objects: Vec<String>,
    /// Tables whose columns and keys aren't loaded, as (schema, table).
    pub tables: Vec<(String, String)>,
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "ILIKE",
    "AS",
    "JOIN",
    "LEFT JOIN",
    "INNER JOIN",
    "RIGHT JOIN",
    "ON",
    "USING",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "RETURNING",
    "WITH",
    "UNION",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "COUNT",
    "BETWEEN",
    "EXISTS",
    "ASC",
    "DESC",
];

/// Words that end a FROM list or can't be an alias.
const CLAUSE_WORDS: &[&str] = &[
    "where",
    "group",
    "order",
    "having",
    "limit",
    "offset",
    "on",
    "using",
    "set",
    "values",
    "union",
    "join",
    "left",
    "right",
    "inner",
    "outer",
    "full",
    "cross",
    "natural",
    "returning",
    "select",
    "from",
    "as",
    "lateral",
    "window",
    "for",
    "into",
    "and",
    "or",
    "fetch",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    /// A quoted identifier, unquoted.
    Ident(String),
    Dot,
    Comma,
    Open,
    Close,
    Other,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    range: Range<usize>,
}

impl Token {
    fn word(&self) -> Option<&str> {
        match &self.tok {
            Tok::Word(w) => Some(w),
            _ => None,
        }
    }

    fn name(&self) -> Option<&str> {
        match &self.tok {
            Tok::Word(w) | Tok::Ident(w) => Some(w),
            _ => None,
        }
    }

    fn is_keyword(&self, keyword: &str) -> bool {
        self.word().is_some_and(|w| w.eq_ignore_ascii_case(keyword))
    }
}

fn is_word_byte(b: u8) -> bool {
    b == b'_' || b == b'$' || b.is_ascii_alphanumeric() || b >= 0x80
}

/// Words, quoted identifiers and punctuation; strings, numbers and comments
/// become `Other` or are skipped.
fn tokens(sql: &str, engine: Engine) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let start = i;
        let tok = if b.is_ascii_whitespace() {
            i += 1;
            continue;
        } else if sql[i..].starts_with("--") || (engine == Engine::Mysql && b == b'#') {
            i = sql[i..].find('\n').map_or(bytes.len(), |p| i + p);
            continue;
        } else if sql[i..].starts_with("/*") {
            i = sql[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |p| i + 2 + p + 2);
            continue;
        } else if b == b'\'' {
            i = skip(bytes, i, b'\'');
            Tok::Other
        } else if b == b'"' && engine == Engine::Postgres || b == b'`' && engine == Engine::Mysql {
            i = skip(bytes, i, b);
            let inner = &sql[start + 1..i.saturating_sub(1).max(start + 1)];
            let quote = b as char;
            Tok::Ident(inner.replace(&format!("{quote}{quote}"), &quote.to_string()))
        } else if b == b'"' {
            i = skip(bytes, i, b'"');
            Tok::Other
        } else if b.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            Tok::Other
        } else if is_word_byte(b) {
            while i < bytes.len() && is_word_byte(bytes[i]) {
                i += 1;
            }
            Tok::Word(sql[start..i].to_owned())
        } else {
            i += 1;
            while !sql.is_char_boundary(i) {
                i += 1;
            }
            match b {
                b'.' => Tok::Dot,
                b',' => Tok::Comma,
                b'(' => Tok::Open,
                b')' => Tok::Close,
                _ => Tok::Other,
            }
        };
        out.push(Token {
            tok,
            range: start..i,
        });
    }
    out
}

fn skip(bytes: &[u8], i: usize, quote: u8) -> usize {
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == quote {
            if bytes.get(j + 1) == Some(&quote) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    bytes.len()
}

/// A table the statement uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TableRef {
    schema: Option<String>,
    table: String,
    alias: Option<String>,
    /// Where the reference ends, to tell which JOIN came last.
    end: usize,
}

impl TableRef {
    /// What the statement calls it: the alias, else the table name.
    fn handle(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.table)
    }
}

fn is_clause_word(token: &Token) -> bool {
    token
        .word()
        .is_some_and(|w| CLAUSE_WORDS.iter().any(|k| w.eq_ignore_ascii_case(k)))
}

/// The tables after FROM (and its commas), JOIN, UPDATE and INTO.
fn table_refs(tokens: &[Token]) -> Vec<TableRef> {
    let mut refs = Vec::new();
    let mut in_from = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        let starts = ["from", "join", "update", "into"]
            .iter()
            .any(|k| t.is_keyword(k));
        let continues = in_from && t.tok == Tok::Comma;
        if t.is_keyword("from") {
            in_from = true;
        } else if is_clause_word(t) && !t.is_keyword("join") || t.tok == Tok::Open {
            in_from = false;
        }
        if !(starts || continues) {
            i += 1;
            continue;
        }
        i += 1;
        let Some(first) = tokens
            .get(i)
            .filter(|t| !is_clause_word(t))
            .and_then(Token::name)
        else {
            continue;
        };
        let mut r = TableRef {
            schema: None,
            table: first.to_owned(),
            alias: None,
            end: tokens[i].range.end,
        };
        i += 1;
        if tokens.get(i).is_some_and(|t| t.tok == Tok::Dot) {
            if let Some(name) = tokens.get(i + 1).and_then(Token::name) {
                r.schema = Some(std::mem::replace(&mut r.table, name.to_owned()));
                r.end = tokens[i + 1].range.end;
                i += 2;
            } else {
                // `schema.` with the caret after the dot.
                i += 1;
                continue;
            }
        }
        if tokens.get(i).is_some_and(|t| t.is_keyword("as")) {
            i += 1;
        }
        if let Some(alias) = tokens
            .get(i)
            .filter(|t| !is_clause_word(t))
            .and_then(Token::name)
        {
            r.alias = Some(alias.to_owned());
            r.end = tokens[i].range.end;
            i += 1;
        }
        refs.push(r);
    }
    refs
}

impl CompletionData {
    fn engine(&self) -> Engine {
        self.engine.unwrap_or(Engine::Postgres)
    }

    fn quote(&self, ident: &str) -> String {
        quote_ident_if_needed(self.engine(), ident)
    }

    fn schema_named(&self, name: &str) -> Option<&str> {
        self.schemas
            .iter()
            .find(|s| s.as_str() == name)
            .or_else(|| self.schemas.iter().find(|s| s.eq_ignore_ascii_case(name)))
            .map(String::as_str)
    }

    /// The schema a reference lives in: its own, else the default schema,
    /// else any loaded schema that has a table of that name.
    fn resolve(&self, r: &TableRef) -> Option<String> {
        if let Some(schema) = &r.schema {
            return self.schema_named(schema).map(str::to_owned);
        }
        let has = |schema: &str| {
            self.objects
                .get(schema)
                .is_some_and(|names| names.iter().any(|n| n == &r.table))
        };
        let default = self.default_schema.clone();
        match &default {
            Some(d) if has(d) || !self.objects.contains_key(d) => default,
            _ => self.objects.keys().find(|s| has(s)).cloned().or(default),
        }
    }

    fn details(&self, r: &TableRef) -> Option<&Arc<TableInfo>> {
        let schema = self.resolve(r)?;
        self.tables.get(&(schema, r.table.clone()))
    }
}

/// The partial word ending at `offset`, and the qualifier before its dot.
fn word_at(sql: &str, offset: usize, engine: Engine) -> (Range<usize>, Option<String>) {
    let bytes = sql.as_bytes();
    let mut start = offset;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut qualifier = None;
    if start > 0 && bytes[start - 1] == b'.' {
        let end = start - 1;
        let quote = match engine {
            Engine::Postgres => b'"',
            Engine::Mysql => b'`',
        };
        if end > 0 && bytes[end - 1] == quote {
            if let Some(open) = sql[..end - 1].rfind(quote as char) {
                qualifier = Some(sql[open + 1..end - 1].to_owned());
            }
        } else {
            let mut q = end;
            while q > 0 && is_word_byte(bytes[q - 1]) {
                q -= 1;
            }
            if q < end {
                qualifier = Some(sql[q..end].to_owned());
            }
        }
    }
    (start..offset, qualifier)
}

/// What the catalog is missing for good completions at `offset`.
pub fn needs(sql: &str, offset: usize, data: &CompletionData) -> Needs {
    let engine = data.engine();
    let Some(range) = split::statement_at(sql, engine, offset).or(Some(offset..offset)) else {
        return Needs::default();
    };
    let statement = &sql[range.start..range.end.max(offset)];
    let toks = tokens(statement, engine);
    let mut needs = Needs::default();
    let want_objects = |schema: &str, needs: &mut Needs| {
        if !data.objects.contains_key(schema) && !needs.objects.iter().any(|s| s == schema) {
            needs.objects.push(schema.to_owned());
        }
    };
    if let Some(d) = &data.default_schema {
        want_objects(d, &mut needs);
    }
    let (_, qualifier) = word_at(sql, offset, engine);
    if let Some(schema) = qualifier.as_deref().and_then(|q| data.schema_named(q)) {
        want_objects(schema, &mut needs);
    }
    for r in table_refs(&toks) {
        if let Some(schema) = r.schema.as_deref().and_then(|s| data.schema_named(s)) {
            want_objects(schema, &mut needs);
        }
        if let Some(schema) = data.resolve(&r) {
            let key = (schema, r.table.clone());
            let listed = data
                .objects
                .get(&key.0)
                .is_none_or(|names| names.contains(&key.1));
            if listed && !data.tables.contains_key(&key) && !needs.tables.contains(&key) {
                needs.tables.push(key);
            }
        }
    }
    needs
}

/// Suggestions for the caret at byte `offset` of `sql`.
pub fn complete(sql: &str, offset: usize, data: &CompletionData) -> Completions {
    let engine = data.engine();
    let (replace, qualifier) = word_at(sql, offset, engine);
    let prefix = sql[replace.clone()].to_lowercase();
    let statement = split::statement_at(sql, engine, offset)
        .filter(|r| r.start <= replace.start)
        .unwrap_or(replace.start..replace.start);
    let base = statement.start;
    let text = &sql[base..statement.end.max(offset)];
    let toks = tokens(text, engine);
    let refs = table_refs(&toks);
    // The last significant token before the word at the caret.
    let before: Vec<&Token> = toks
        .iter()
        .filter(|t| t.range.end + base <= replace.start)
        .collect();
    let previous = before.last().copied();

    let mut items = Vec::new();
    if let Some(q) = qualifier {
        if let Some(r) = refs.iter().find(|r| r.handle().eq_ignore_ascii_case(&q)) {
            columns(data, r, None, &mut items);
        } else if let Some(schema) = data.schema_named(&q) {
            tables(data, schema, false, &mut items);
        }
    } else if previous.is_some_and(|t| {
        ["from", "join", "update", "into", "table"]
            .iter()
            .any(|k| t.is_keyword(k))
    }) || previous.is_some_and(|t| t.tok == Tok::Comma) && in_from_list(&before)
    {
        if previous.is_some_and(|t| t.is_keyword("join")) {
            joins(data, &refs, &mut items);
        }
        if let Some(schema) = data.default_schema.clone() {
            tables(data, &schema, false, &mut items);
        }
        for schema in &data.schemas {
            items.push(Completion {
                label: schema.clone(),
                insert: data.quote(schema),
                kind: Kind::Schema,
                detail: Some("schema".into()),
            });
        }
    } else if previous.is_some_and(|t| t.is_keyword("on")) {
        conditions(data, &refs, &mut items);
        all_columns(data, &refs, &mut items);
    } else {
        all_columns(data, &refs, &mut items);
        if !prefix.is_empty() {
            for k in KEYWORDS {
                items.push(Completion {
                    label: (*k).into(),
                    insert: (*k).into(),
                    kind: Kind::Keyword,
                    detail: None,
                });
            }
        }
    }

    let mut seen = HashSet::new();
    items.retain(|c| {
        c.label.to_lowercase().starts_with(&prefix) && seen.insert((c.kind, c.label.clone()))
    });
    Completions { replace, items }
}

/// Whether the comma before the caret belongs to a FROM list.
fn in_from_list(before: &[&Token]) -> bool {
    for t in before.iter().rev() {
        if t.is_keyword("from") {
            return true;
        }
        if is_clause_word(t) || t.tok == Tok::Open || t.tok == Tok::Close {
            return false;
        }
    }
    false
}

fn tables(data: &CompletionData, schema: &str, qualify: bool, items: &mut Vec<Completion>) {
    let Some(names) = data.objects.get(schema) else {
        return;
    };
    for name in names {
        let insert = if qualify {
            format!("{}.{}", data.quote(schema), data.quote(name))
        } else {
            data.quote(name)
        };
        items.push(Completion {
            label: name.clone(),
            insert,
            kind: Kind::Table,
            detail: Some(schema.to_owned()),
        });
    }
}

fn columns(data: &CompletionData, r: &TableRef, show: Option<&str>, items: &mut Vec<Completion>) {
    let Some(info) = data.details(r) else {
        return;
    };
    for c in &info.columns {
        items.push(Completion {
            label: c.name.clone(),
            insert: data.quote(&c.name),
            kind: Kind::Column,
            detail: Some(match show {
                Some(handle) => format!("{handle} · {}", c.data_type),
                None => c.data_type.clone(),
            }),
        });
    }
}

fn all_columns(data: &CompletionData, refs: &[TableRef], items: &mut Vec<Completion>) {
    for r in refs {
        let show = (refs.len() > 1).then(|| r.handle());
        columns(data, r, show, items);
    }
}

/// A short alias from the table's initials (`order_items` → `oi`), unique
/// among `taken`.
fn alias_for(table: &str, taken: &[String]) -> String {
    let initials: String = table
        .split('_')
        .filter_map(|part| part.chars().next())
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    let base = if initials.is_empty() {
        "t".into()
    } else {
        initials
    };
    let mut alias = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&alias))
        || CLAUSE_WORDS.contains(&alias.as_str())
    {
        alias = format!("{base}{n}");
        n += 1;
    }
    alias
}

/// `a.x = b.y AND …` for the column pairs of a foreign key.
fn condition(
    data: &CompletionData,
    left: &str,
    left_cols: &[String],
    right: &str,
    right_cols: &[String],
) -> String {
    left_cols
        .iter()
        .zip(right_cols)
        .map(|(l, r)| {
            format!(
                "{}.{} = {}.{}",
                data.quote(left),
                data.quote(l),
                data.quote(right),
                data.quote(r)
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Whole `table alias ON …` clauses for tables related by a foreign key to
/// the tables already in the statement.
fn joins(data: &CompletionData, refs: &[TableRef], items: &mut Vec<Completion>) {
    let taken: Vec<String> = refs.iter().map(|r| r.handle().to_owned()).collect();
    let mut add = |schema: &str, table: &str, cond: &dyn Fn(&str) -> String| {
        let alias = alias_for(table, &taken);
        let target = match &data.default_schema {
            Some(d) if d == schema => data.quote(table),
            _ => format!("{}.{}", data.quote(schema), data.quote(table)),
        };
        let clause = format!("{target} {alias} ON {}", cond(&alias));
        items.push(Completion {
            label: clause.clone(),
            insert: clause,
            kind: Kind::Join,
            detail: Some("foreign key".into()),
        });
    };
    for r in refs {
        let Some(info) = data.details(r) else {
            continue;
        };
        let handle = r.handle();
        // Tables this one points to.
        for fk in &info.foreign_keys {
            add(&fk.ref_schema, &fk.ref_table, &|alias| {
                condition(data, alias, &fk.ref_columns, handle, &fk.columns)
            });
        }
        // Loaded tables that point to this one.
        let schema = data.resolve(r).unwrap_or_default();
        for ((other_schema, other), other_info) in &data.tables {
            for fk in other_info
                .foreign_keys
                .iter()
                .filter(|fk| fk.ref_schema == schema && fk.ref_table == r.table)
            {
                add(other_schema, other, &|alias| {
                    condition(data, alias, &fk.columns, handle, &fk.ref_columns)
                });
            }
        }
    }
}

/// Conditions after `ON` that link the last joined table to the others.
fn conditions(data: &CompletionData, refs: &[TableRef], items: &mut Vec<Completion>) {
    let Some((last, others)) = refs.split_last() else {
        return;
    };
    let mut push = |text: String| {
        items.push(Completion {
            label: text.clone(),
            insert: text,
            kind: Kind::Join,
            detail: Some("foreign key".into()),
        })
    };
    let last_schema = data.resolve(last).unwrap_or_default();
    for other in others {
        let other_schema = data.resolve(other).unwrap_or_default();
        if let Some(info) = data.details(last) {
            for fk in info
                .foreign_keys
                .iter()
                .filter(|fk| fk.ref_table == other.table && fk.ref_schema == other_schema)
            {
                push(condition(
                    data,
                    last.handle(),
                    &fk.columns,
                    other.handle(),
                    &fk.ref_columns,
                ));
            }
        }
        if let Some(info) = data.details(other) {
            for fk in info
                .foreign_keys
                .iter()
                .filter(|fk| fk.ref_table == last.table && fk.ref_schema == last_schema)
            {
                push(condition(
                    data,
                    last.handle(),
                    &fk.ref_columns,
                    other.handle(),
                    &fk.columns,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnInfo, ForeignKey, TableKind};

    fn table(name: &str, columns: &[&str], fks: &[(&str, &str, &str)]) -> Arc<TableInfo> {
        let mut t = TableInfo::new(name, TableKind::Table);
        t.columns = columns
            .iter()
            .map(|c| ColumnInfo {
                name: (*c).into(),
                data_type: "integer".into(),
                nullable: true,
                default: None,
            })
            .collect();
        t.foreign_keys = fks
            .iter()
            .map(|(col, ref_table, ref_col)| ForeignKey {
                name: format!("{name}_{col}_fk"),
                columns: vec![(*col).into()],
                ref_schema: "public".into(),
                ref_table: (*ref_table).into(),
                ref_columns: vec![(*ref_col).into()],
            })
            .collect();
        Arc::new(t)
    }

    fn data() -> CompletionData {
        let mut d = CompletionData {
            engine: Some(Engine::Postgres),
            schemas: vec!["public".into(), "Audit".into()],
            default_schema: Some("public".into()),
            ..Default::default()
        };
        d.objects.insert(
            "public".into(),
            vec!["customers".into(), "order_items".into(), "orders".into()],
        );
        d.objects.insert("Audit".into(), vec!["Log".into()]);
        let key = |t: &str| ("public".to_string(), t.to_string());
        d.tables
            .insert(key("customers"), table("customers", &["id", "name"], &[]));
        d.tables.insert(
            key("orders"),
            table(
                "orders",
                &["id", "customer_id", "total"],
                &[("customer_id", "customers", "id")],
            ),
        );
        d.tables.insert(
            key("order_items"),
            table(
                "order_items",
                &["order_id", "qty"],
                &[("order_id", "orders", "id")],
            ),
        );
        d
    }

    /// Completes at the `|` in `sql`.
    fn at(sql: &str) -> Vec<String> {
        let offset = sql.find('|').unwrap();
        let sql = sql.replace('|', "");
        complete(&sql, offset, &data())
            .items
            .into_iter()
            .map(|c| c.insert)
            .collect()
    }

    #[test]
    fn tables_and_schemas_after_from() {
        assert_eq!(
            at("SELECT * FROM |"),
            ["customers", "order_items", "orders", "public", "\"Audit\""]
        );
        assert_eq!(at("SELECT * FROM ord|"), ["order_items", "orders"]);
        assert_eq!(at("SELECT * FROM orders o, cu|"), ["customers"]);
        assert_eq!(at("SELECT * FROM \"Audit\".|"), ["\"Log\""]);
    }

    #[test]
    fn columns_by_alias_and_in_the_select_list() {
        assert_eq!(
            at("SELECT o.| FROM orders o"),
            ["id", "customer_id", "total"]
        );
        assert_eq!(at("SELECT o.cu| FROM orders AS o"), ["customer_id"]);
        assert_eq!(at("SELECT tot| FROM orders"), ["total"]);
        assert_eq!(
            at("SELECT * FROM orders WHERE customers.| "),
            Vec::<String>::new()
        );
        assert!(at("SELECT na| FROM customers").contains(&"name".to_string()));
    }

    #[test]
    fn keywords_only_with_a_prefix() {
        assert!(at("SELECT * FROM orders WH|").contains(&"WHERE".to_string()));
        assert!(!at("SELECT | FROM orders").contains(&"WHERE".to_string()));
    }

    #[test]
    fn join_clauses_from_foreign_keys() {
        assert_eq!(
            at("SELECT * FROM orders o JOIN |")[..2],
            [
                "customers c ON c.id = o.customer_id".to_string(),
                "order_items oi ON oi.order_id = o.id".to_string(),
            ]
        );
        assert_eq!(
            at("SELECT * FROM orders o JOIN customers c ON |")[0],
            "c.id = o.customer_id"
        );
    }

    #[test]
    fn aliases_avoid_clashes() {
        assert_eq!(alias_for("customers", &["c".into()]), "c2");
        assert_eq!(alias_for("order_items", &[]), "oi");
        assert_eq!(alias_for("on", &[]), "o");
        assert_eq!(alias_for("o_n", &[]), "on2");
    }

    #[test]
    fn only_the_statement_at_the_caret_counts() {
        assert_eq!(
            at("SELECT * FROM customers;\nSELECT o.| FROM orders o;"),
            ["id", "customer_id", "total"]
        );
    }

    #[test]
    fn needs_lists_missing_catalog_parts() {
        let mut d = data();
        d.tables.clear();
        d.objects.remove("Audit");
        let sql = "SELECT * FROM orders o JOIN \"Audit\".\"Log\" l ON ";
        let needs = needs(sql, sql.len(), &d);
        assert_eq!(needs.objects, ["Audit"]);
        assert_eq!(
            needs.tables,
            [
                ("public".to_string(), "orders".to_string()),
                ("Audit".to_string(), "Log".to_string())
            ]
        );
    }
}
