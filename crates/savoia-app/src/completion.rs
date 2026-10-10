//! Completion for the console's SQL editor, from the session's catalog
//! cache. Missing object lists and table details are loaded first when the
//! session is idle; while a query runs, completion uses what is cached.

use std::sync::Arc;

use anyhow::Result;
use gpui_kit::component::input::CompletionProvider;
use gpui_kit::component::{Rope, RopeExt as _};
use gpui_kit::*;
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    TextEdit,
};
use savoia_core::complete::{self, CompletionData, Kind, Needs};
use savoia_core::{AppResult, Engine};

use crate::console::QueryConsole;
use crate::data_sources::DataSources;
use crate::session::Session;
use crate::{runtime, session};

pub struct SqlCompletion {
    pub data_sources: Entity<DataSources>,
    pub console: WeakEntity<QueryConsole>,
}

/// What completion knows about `session`, and the database it browses.
pub fn snapshot(session: &Session, engine: Engine) -> (CompletionData, String) {
    let catalog = session.catalog();
    let current = catalog
        .current()
        .or(catalog.databases.first())
        .map(|d| d.name.clone())
        .unwrap_or_default();
    let mut data = CompletionData {
        engine: Some(engine),
        ..Default::default()
    };
    let names = |objects: &savoia_core::SchemaObjects| {
        objects
            .tables
            .iter()
            .chain(&objects.views)
            .cloned()
            .collect::<Vec<_>>()
    };
    match engine {
        Engine::Postgres => {
            let schemas = catalog
                .database(&current)
                .and_then(|d| d.schemas.as_ref())
                .cloned()
                .unwrap_or_default();
            for schema in &schemas {
                data.schemas.push(schema.name.clone());
                if let Some(objects) = &schema.objects {
                    data.objects.insert(schema.name.clone(), names(objects));
                }
            }
            data.default_schema = schemas
                .iter()
                .find(|s| s.name == "public")
                .or(schemas.first())
                .map(|s| s.name.clone());
            data.tables = session.tables_in(&current).into_iter().collect();
        }
        Engine::Mysql => {
            for db in &catalog.databases {
                data.schemas.push(db.name.clone());
                if let Some(objects) = db.schemas.iter().flatten().find_map(|s| s.objects.as_ref())
                {
                    data.objects.insert(db.name.clone(), names(objects));
                }
                data.tables.extend(session.tables_in(&db.name));
            }
            data.default_schema = Some(current.clone());
        }
    }
    (data, current)
}

/// Loads what `needs` lists. On MySQL each schema is its own database.
async fn load(
    session: Arc<Session>,
    engine: Engine,
    database: String,
    needs: Needs,
) -> AppResult<()> {
    let db = |schema: &str| match engine {
        Engine::Postgres => database.clone(),
        Engine::Mysql => schema.to_owned(),
    };
    for schema in &needs.objects {
        session.load_objects(&db(schema), schema).await?;
    }
    for (schema, table) in &needs.tables {
        session.describe_table(&db(schema), schema, table).await?;
    }
    Ok(())
}

fn to_items(text: &Rope, completions: complete::Completions) -> Vec<CompletionItem> {
    let range = lsp_types::Range {
        start: text.offset_to_position(completions.replace.start),
        end: text.offset_to_position(completions.replace.end),
    };
    completions
        .items
        .into_iter()
        .map(|c| CompletionItem {
            label: c.label.clone(),
            kind: Some(match c.kind {
                Kind::Keyword => CompletionItemKind::KEYWORD,
                Kind::Schema => CompletionItemKind::MODULE,
                Kind::Table => CompletionItemKind::STRUCT,
                Kind::Column => CompletionItemKind::FIELD,
                Kind::Join => CompletionItemKind::SNIPPET,
            }),
            detail: c.detail,
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: c.insert,
            })),
            ..Default::default()
        })
        .collect()
}

impl CompletionProvider for SqlCompletion {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let empty = || Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        let Some(source) = self.console.upgrade().and_then(|c| c.read(cx).source(cx)) else {
            return empty();
        };
        let ds = self.data_sources.read(cx);
        let (Some(session), Some(engine)) = (ds.session(source), ds.get(source).map(|c| c.engine))
        else {
            return empty();
        };
        let sql = text.to_string();
        let (data, database) = snapshot(&session, engine);
        let needs = complete::needs(&sql, offset, &data);
        let text = text.clone();
        if needs == Needs::default() || session.is_busy() {
            let items = to_items(&text, complete::complete(&sql, offset, &data));
            return Task::ready(Ok(CompletionResponse::Array(items)));
        }
        cx.spawn(async move |_| {
            let loading = runtime::spawn(load(session.clone(), engine, database, needs));
            // A failed load still leaves completion with what is cached.
            drop(session::join(loading).await);
            let (data, _) = snapshot(&session, engine);
            let items = to_items(&text, complete::complete(&sql, offset, &data));
            Ok(CompletionResponse::Array(items))
        })
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text
            .chars()
            .last()
            .is_some_and(|c| c == '.' || c == '_' || c.is_alphanumeric())
    }
}
