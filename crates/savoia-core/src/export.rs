//! Result rows as text to copy or save: TSV, CSV, JSON or INSERT statements.
//! NULL and the empty string always come out different.

use crate::sql_text::{quote_ident, quote_literal};
use crate::{ColumnMeta, Engine, Row, ValueKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Tab-separated with a header; NULL is `\N`, and tabs, newlines and
    /// backslashes are escaped, as in Postgres `COPY`.
    Tsv,
    /// RFC 4180 with a header; NULL is an empty field, the empty string is `""`.
    Csv,
    /// An array of objects; numbers, booleans and JSON values stay unquoted.
    Json,
    /// One `INSERT` per row.
    Insert,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Tsv, Format::Csv, Format::Json, Format::Insert];

    pub fn label(self) -> &'static str {
        match self {
            Format::Tsv => "TSV",
            Format::Csv => "CSV",
            Format::Json => "JSON",
            Format::Insert => "SQL INSERT",
        }
    }
}

/// Writes `rows` in `format`. `table` names the target of INSERTs.
pub fn write<'a>(
    format: Format,
    engine: Engine,
    table: &str,
    columns: &[ColumnMeta],
    rows: impl IntoIterator<Item = &'a Row>,
) -> String {
    let mut out = String::new();
    match format {
        Format::Tsv => {
            line(&mut out, columns.iter().map(|c| tsv(Some(&c.name))), "\t");
            for row in rows {
                line(&mut out, row.iter().map(|v| tsv(v.as_deref())), "\t");
            }
        }
        Format::Csv => {
            line(&mut out, columns.iter().map(|c| csv(Some(&c.name))), ",");
            for row in rows {
                line(&mut out, row.iter().map(|v| csv(v.as_deref())), ",");
            }
        }
        Format::Json => {
            let objects: Vec<String> = rows
                .into_iter()
                .map(|row| {
                    let fields: Vec<String> = columns
                        .iter()
                        .zip(row.iter())
                        .map(|(c, v)| format!("{}: {}", json_string(&c.name), json(c, v)))
                        .collect();
                    format!("  {{{}}}", fields.join(", "))
                })
                .collect();
            if objects.is_empty() {
                out.push_str("[]\n");
            } else {
                out.push_str(&format!("[\n{}\n]\n", objects.join(",\n")));
            }
        }
        Format::Insert => {
            let names: Vec<String> = columns
                .iter()
                .map(|c| quote_ident(engine, &c.name))
                .collect();
            let head = format!("INSERT INTO {table} ({}) VALUES", names.join(", "));
            for row in rows {
                let values: Vec<String> = columns
                    .iter()
                    .zip(row.iter())
                    .map(|(c, v)| sql_value(engine, c, v.as_deref()))
                    .collect();
                out.push_str(&format!("{head} ({});\n", values.join(", ")));
            }
        }
    }
    out
}

fn line(out: &mut String, fields: impl Iterator<Item = String>, sep: &str) {
    out.push_str(&fields.collect::<Vec<_>>().join(sep));
    out.push('\n');
}

fn tsv(value: Option<&str>) -> String {
    match value {
        None => "\\N".into(),
        Some(v) => v
            .replace('\\', "\\\\")
            .replace('\t', "\\t")
            .replace('\n', "\\n")
            .replace('\r', "\\r"),
    }
}

fn csv(value: Option<&str>) -> String {
    match value {
        None => String::new(),
        Some(v) if v.is_empty() || v.contains([',', '"', '\n', '\r']) => {
            format!("\"{}\"", v.replace('"', "\"\""))
        }
        Some(v) => v.to_owned(),
    }
}

fn json(column: &ColumnMeta, value: &Option<Box<str>>) -> String {
    let Some(v) = value.as_deref() else {
        return "null".into();
    };
    match column.kind {
        ValueKind::Integer | ValueKind::Decimal | ValueKind::Float if is_json_number(v) => {
            v.to_owned()
        }
        ValueKind::Bool => match v {
            "t" | "true" | "1" => "true".into(),
            "f" | "false" | "0" => "false".into(),
            _ => json_string(v),
        },
        // The server rendered it, so it is valid JSON.
        ValueKind::Json => v.to_owned(),
        _ => json_string(v),
    }
}

fn is_json_number(v: &str) -> bool {
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let v = v.strip_prefix('-').unwrap_or(v);
    let (mantissa, exp) = match v.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e.strip_prefix(['+', '-']).unwrap_or(e))),
        None => (v, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (mantissa, None),
    };
    all_digits(int)
        && (int == "0" || !int.starts_with('0'))
        && frac.is_none_or(all_digits)
        && exp.is_none_or(all_digits)
}

fn json_string(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn sql_value(engine: Engine, column: &ColumnMeta, value: Option<&str>) -> String {
    match value {
        None => "NULL".into(),
        Some(v) if column.kind.is_numeric() && is_json_number(v) => v.to_owned(),
        Some(v) => quote_literal(engine, v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta {
                name: "id".into(),
                type_name: "int4".into(),
                kind: ValueKind::Integer,
            },
            ColumnMeta {
                name: "note".into(),
                type_name: "text".into(),
                kind: ValueKind::Text,
            },
        ]
    }

    fn rows() -> Vec<Row> {
        vec![
            Box::new([Some("1".into()), None]),
            Box::new([Some("2".into()), Some("".into())]),
            Box::new([Some("3".into()), Some("a,\"b\"\tc\nd's".into())]),
        ]
    }

    fn out(format: Format, engine: Engine) -> String {
        write(format, engine, "\"t\"", &columns(), &rows())
    }

    #[test]
    fn tsv_escapes_and_keeps_null_apart() {
        assert_eq!(
            out(Format::Tsv, Engine::Postgres),
            "id\tnote\n1\t\\N\n2\t\n3\ta,\"b\"\\tc\\nd's\n"
        );
    }

    #[test]
    fn csv_quotes_and_keeps_null_apart() {
        assert_eq!(
            out(Format::Csv, Engine::Postgres),
            "id,note\n1,\n2,\"\"\n3,\"a,\"\"b\"\"\tc\nd's\"\n"
        );
    }

    #[test]
    fn json_types_values() {
        assert_eq!(
            out(Format::Json, Engine::Postgres),
            "[\n  {\"id\": 1, \"note\": null},\n  {\"id\": 2, \"note\": \"\"},\n  \
             {\"id\": 3, \"note\": \"a,\\\"b\\\"\\tc\\nd's\"}\n]\n"
        );
        assert_eq!(
            write(Format::Json, Engine::Mysql, "t", &columns(), &[]),
            "[]\n"
        );
    }

    #[test]
    fn inserts_quote_per_engine() {
        assert_eq!(
            out(Format::Insert, Engine::Mysql).lines().next(),
            Some("INSERT INTO \"t\" (`id`, `note`) VALUES (1, NULL);")
        );
        assert_eq!(
            out(Format::Insert, Engine::Postgres).lines().nth(1),
            Some("INSERT INTO \"t\" (\"id\", \"note\") VALUES (2, '');")
        );
    }

    #[test]
    fn json_numbers() {
        for good in ["0", "-1", "1.50", "2e10", "-0.5E-3"] {
            assert!(is_json_number(good), "{good}");
        }
        for bad in ["01", "1.", ".5", "NaN", "Infinity", "1e", "", "-"] {
            assert!(!is_json_number(bad), "{bad}");
        }
    }
}
