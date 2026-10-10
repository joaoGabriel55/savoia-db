//! CSV into an existing table. Fields go in as string literals and the server
//! converts them to the column types. An unquoted empty field is NULL and a
//! quoted one (`""`) the empty string, matching what our exporters write.

use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};

use savoia_core::sql_text::{quote_ident, quote_literal};
use savoia_core::{AppError, AppResult, ColumnInfo, Connection, Engine};

use super::{
    ImportError, ImportEvent, ImportOptions, ImportSummary, Input, Tracker, abort, end, preview,
    run, start,
};

/// Rows per `INSERT`...
const BATCH_ROWS: usize = 500;
/// ...unless the statement grows past this many bytes first.
const BATCH_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvOptions {
    pub delimiter: u8,
    /// The first record names the columns.
    pub has_header: bool,
    /// Unquoted empty fields are NULL; otherwise they are empty strings too.
    pub empty_is_null: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            has_header: true,
            empty_is_null: true,
        }
    }
}

/// The first rows of a file, for the mapping step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvPreview {
    /// The header's names, or `column1`, `column2`, … without a header.
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

/// Which CSV column fills which table column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMapping {
    /// Index of the CSV column.
    pub source: usize,
    /// Name of the table column.
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct CsvImport {
    /// A `.csv` file, optionally gzip-compressed.
    pub path: PathBuf,
    pub csv: CsvOptions,
    /// The Postgres schema, or the MySQL database, of `table`.
    pub schema: String,
    pub table: String,
    pub mapping: Vec<ColumnMapping>,
    pub options: ImportOptions,
}

/// Reads the header and up to `rows` records of `path`.
pub fn preview_csv(path: &Path, csv: CsvOptions, rows: usize) -> AppResult<CsvPreview> {
    let input = Input::open(path)?;
    let mut reader = Records::new(BufReader::new(input.reader), csv);
    let storage = |err: io::Error| AppError::storage(format!("{}: {err}", path.display()));
    let mut records = Vec::new();
    let mut headers = None;
    while records.len() < rows {
        let Some((_, record)) = reader.next().map_err(storage)? else {
            break;
        };
        if csv.has_header && headers.is_none() {
            headers = Some(record.into_iter().map(Option::unwrap_or_default).collect());
        } else {
            records.push(record);
        }
    }
    let headers = headers.unwrap_or_else(|| {
        let width = records.iter().map(Vec::len).max().unwrap_or(0);
        (1..=width).map(|i| format!("column{i}")).collect()
    });
    Ok(CsvPreview {
        headers,
        rows: records,
    })
}

/// Maps each header to the table column of the same name, ignoring case
/// and surrounding spaces. Unmatched headers are left out.
pub fn auto_map(headers: &[String], columns: &[ColumnInfo]) -> Vec<ColumnMapping> {
    headers
        .iter()
        .enumerate()
        .filter_map(|(source, header)| {
            columns
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(header.trim()))
                .map(|c| ColumnMapping {
                    source,
                    target: c.name.clone(),
                })
        })
        .collect()
}

/// Inserts the rows of `request.path` into its table.
///
/// Rows go in batches; when a batch fails, its rows are retried one by one
/// so the error names the row. `Err` means the import couldn't go on at all.
pub async fn import_csv(
    conn: &dyn Connection,
    engine: Engine,
    request: &CsvImport,
    progress: impl FnMut(ImportEvent) + Send,
) -> AppResult<ImportSummary> {
    if request.mapping.is_empty() {
        return Err(AppError::invalid(
            "map at least one CSV column to a table column",
        ));
    }
    let input = Input::open(&request.path)?;
    let mut tracker = Tracker::new(request.options, &input, progress);
    start(conn, engine, request.options).await?;
    if let Err(err) = insert_rows(conn, engine, request, input, &mut tracker).await {
        abort(conn, request.options).await;
        return Err(err);
    }
    let mut summary = tracker.summary;
    end(conn, request.options, &mut summary).await?;
    Ok(summary)
}

async fn insert_rows(
    conn: &dyn Connection,
    engine: Engine,
    request: &CsvImport,
    input: Input,
    tracker: &mut Tracker<impl FnMut(ImportEvent) + Send>,
) -> AppResult<()> {
    let q = |name: &str| quote_ident(engine, name);
    let columns: Vec<String> = request.mapping.iter().map(|m| q(&m.target)).collect();
    let head = format!(
        "INSERT INTO {}.{} ({}) VALUES",
        q(&request.schema),
        q(&request.table),
        columns.join(", ")
    );
    let storage = |err: io::Error| AppError::storage(format!("{}: {err}", request.path.display()));
    let mut reader = Records::new(BufReader::new(input.reader), request.csv);
    if request.csv.has_header {
        reader.next().map_err(storage)?;
    }

    let mut batch: Vec<(u64, String)> = Vec::new();
    let mut bytes = 0;
    loop {
        let record = reader.next().map_err(storage)?;
        let last = record.is_none();
        if let Some((line, record)) = record {
            let values: Vec<String> = request
                .mapping
                .iter()
                .map(|m| match record.get(m.source).cloned().flatten() {
                    Some(value) => quote_literal(engine, &value),
                    None => "NULL".into(),
                })
                .collect();
            let values = format!("({})", values.join(", "));
            bytes += values.len();
            batch.push((line, values));
            if batch.len() < BATCH_ROWS && bytes < BATCH_BYTES {
                continue;
            }
        }
        if !batch.is_empty() && !insert(conn, &head, &batch, tracker).await {
            return Ok(());
        }
        batch.clear();
        bytes = 0;
        if last {
            return Ok(());
        }
    }
}

/// Inserts `rows`, retrying them one by one if the batch fails (except in a
/// single transaction, which the failure has already doomed). False when the
/// import must stop.
async fn insert(
    conn: &dyn Connection,
    head: &str,
    rows: &[(u64, String)],
    tracker: &mut Tracker<impl FnMut(ImportEvent) + Send>,
) -> bool {
    let values: Vec<&str> = rows.iter().map(|(_, v)| v.as_str()).collect();
    let error = match run(conn, format!("{head}\n{}", values.join(",\n"))).await {
        Ok(()) => {
            tracker.succeeded(rows.len() as u64);
            return true;
        }
        Err(message) => message,
    };
    if rows.len() == 1 || tracker.options.single_transaction {
        return !tracker.failed(ImportError {
            line: rows[0].0,
            what: preview(&rows[0].1, 200),
            message: error,
        });
    }
    for (line, values) in rows {
        match run(conn, format!("{head} {values}")).await {
            Ok(()) => tracker.succeeded(1),
            Err(message) => {
                let error = ImportError {
                    line: *line,
                    what: preview(values, 200),
                    message,
                };
                if tracker.failed(error) {
                    return false;
                }
            }
        }
    }
    true
}

/// A CSV field: `None` for NULL.
type Field = Option<String>;

/// Reads CSV records (RFC 4180, any delimiter), with the line each starts on.
struct Records<R> {
    reader: R,
    csv: CsvOptions,
    /// Lines consumed so far.
    line: u64,
}

impl<R: BufRead> Records<R> {
    fn new(reader: R, csv: CsvOptions) -> Self {
        Self {
            reader,
            csv,
            line: 0,
        }
    }

    fn next(&mut self) -> io::Result<Option<(u64, Vec<Field>)>> {
        loop {
            let start = self.line + 1;
            let mut raw = Vec::new();
            // A record goes on while a quoted field is open (odd quote count).
            loop {
                let n = self.reader.read_until(b'\n', &mut raw)?;
                if n == 0 {
                    break;
                }
                self.line += 1;
                if raw.iter().filter(|&&b| b == b'"').count() % 2 == 0 {
                    break;
                }
            }
            if raw.is_empty() {
                return Ok(None);
            }
            if start == 1 && raw.starts_with("\u{feff}".as_bytes()) {
                raw.drain(..3);
            }
            let text = String::from_utf8(raw).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("line {start} isn't valid UTF-8"),
                )
            })?;
            let text = text.strip_suffix('\n').unwrap_or(&text);
            let text = text.strip_suffix('\r').unwrap_or(text);
            if text.is_empty() {
                continue;
            }
            return Ok(Some((start, self.fields(text))));
        }
    }

    fn fields(&self, text: &str) -> Vec<Field> {
        let delimiter = char::from(self.csv.delimiter);
        let mut fields = Vec::new();
        let mut value = String::new();
        let mut quoted = false;
        let mut in_quotes = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if in_quotes {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        value.push('"');
                        chars.next();
                    } else {
                        in_quotes = false;
                    }
                } else {
                    value.push(c);
                }
            } else if c == '"' && value.is_empty() && !quoted {
                in_quotes = true;
                quoted = true;
            } else if c == delimiter {
                fields.push(self.field(std::mem::take(&mut value), quoted));
                quoted = false;
            } else {
                value.push(c);
            }
        }
        fields.push(self.field(value, quoted));
        fields
    }

    fn field(&self, value: String, quoted: bool) -> Field {
        (quoted || !value.is_empty() || !self.csv.empty_is_null).then_some(value)
    }
}

/// Reads CSV records from any reader; for tests.
#[cfg(test)]
fn parse(text: &str, csv: CsvOptions) -> Vec<(u64, Vec<Field>)> {
    let mut records = Records::new(text.as_bytes(), csv);
    std::iter::from_fn(|| records.next().unwrap()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Field {
        Some(text.into())
    }

    #[test]
    fn parses_quotes_nulls_and_multiline_fields() {
        let records = parse(
            "\u{feff}id,note\r\n1,\n2,\"\"\n\n3,\"a,\"\"b\"\"\nnext line\"\n4,plain\n",
            CsvOptions::default(),
        );
        assert_eq!(
            records,
            [
                (1, vec![s("id"), s("note")]),
                (2, vec![s("1"), None]),
                (3, vec![s("2"), s("")]),
                (5, vec![s("3"), s("a,\"b\"\nnext line")]),
                (7, vec![s("4"), s("plain")]),
            ]
        );
    }

    #[test]
    fn honours_delimiter_and_empty_as_text() {
        let csv = CsvOptions {
            delimiter: b';',
            has_header: false,
            empty_is_null: false,
        };
        assert_eq!(parse("a;;c", csv), [(1, vec![s("a"), s(""), s("c")])]);
    }

    #[test]
    fn maps_headers_to_columns_by_name() {
        let column = |name: &str| ColumnInfo {
            name: name.into(),
            data_type: "text".into(),
            nullable: true,
            default: None,
        };
        let mapping = auto_map(
            &["ID".into(), " Name ".into(), "extra".into()],
            &[column("id"), column("name")],
        );
        assert_eq!(
            mapping,
            [
                ColumnMapping {
                    source: 0,
                    target: "id".into()
                },
                ColumnMapping {
                    source: 1,
                    target: "name".into()
                },
            ]
        );
    }

    #[test]
    fn previews_with_and_without_header() {
        let path =
            std::env::temp_dir().join(format!("savoia-preview-{}.csv", uuid::Uuid::new_v4()));
        std::fs::write(&path, "a,b\n1,2\n3,4\n5,6\n").unwrap();
        let preview = preview_csv(&path, CsvOptions::default(), 2).unwrap();
        assert_eq!(preview.headers, ["a", "b"]);
        assert_eq!(preview.rows, [vec![s("1"), s("2")], vec![s("3"), s("4")]]);
        let csv = CsvOptions {
            has_header: false,
            ..CsvOptions::default()
        };
        assert_eq!(
            preview_csv(&path, csv, 1).unwrap().headers,
            ["column1", "column2"]
        );
    }
}
