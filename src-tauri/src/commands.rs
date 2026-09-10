//! The commands the interface can call.
//!
//! This is the whole surface between the frontend and everything else. The
//! frontend has no database driver and no way to reach one
//! (docs/17-coding-standards.md), so every rule enforced below is enforced
//! for real rather than by the interface choosing to behave.

use mydb_adapters::postgres::{PostgresAdapter, PostgresConnectionDetails};
use mydb_adapters::{Adapter, Preview, PreviewBody, RecordSet};
use mydb_core::{Engine, Schema, Secret};
use mydb_storage::{Connection, ConnectionStore};
use tauri::State;

use crate::dto::{
    ActiveConnectionInfo, ColumnView, CommandOutcome, ConnectionInput, ConnectionSummary,
    ExecutionSummary, PreviewView, RecordsView, TableSummary, TableView,
};
use crate::state::{ActiveConnection, AppState};

/// An error the interface can display.
///
/// A plain string, because every error reaching this point has already been
/// shaped into something safe to show: the storage and adapter layers keep
/// credentials out of their messages, and the parser's errors are written to
/// be read by the person who typed the command.
type UiResult<T> = Result<T, String>;

fn summarise(connection: &Connection) -> ConnectionSummary {
    ConnectionSummary {
        id: connection.id.clone(),
        name: connection.name.clone(),
        engine: connection.engine.display_name().to_string(),
        host: connection.host.clone(),
        port: connection.port,
        database: connection.database.clone(),
        username: connection.username.clone(),
        production: connection.production,
    }
}

fn view(records: &RecordSet) -> RecordsView {
    RecordsView {
        columns: records.columns().to_vec(),
        rows: records
            .records()
            .iter()
            .map(|record| record.cells.clone())
            .collect(),
        total_count: records.total_count(),
        truncated: records.is_truncated(),
        statement: records.statement().to_string(),
    }
}

/// Renders a preview for the interface, keeping the two shapes distinct.
fn preview_view(preview: &Preview) -> PreviewView {
    match preview.body() {
        PreviewBody::Records(records) => PreviewView::Records(view(records)),
        PreviewBody::Table(outline) => PreviewView::Table(TableView {
            columns: outline
                .columns()
                .iter()
                .map(|column| ColumnView {
                    name: column.name.clone(),
                    data_type: column.data_type.clone(),
                    nullable: column.nullable,
                })
                .collect(),
            row_count: outline.row_count(),
            statement: outline.statement().to_string(),
        }),
    }
}

/// Opens the connection store, creating it on first use.
async fn with_store<T>(
    state: &AppState,
    action: impl FnOnce(&mut ConnectionStore) -> Result<T, String>,
) -> UiResult<T> {
    let mut guard = state.connections.lock().await;
    if guard.is_none() {
        let path = ConnectionStore::default_path().map_err(|error| error.to_string())?;
        *guard = Some(ConnectionStore::load(path).map_err(|error| error.to_string())?);
    }
    let store = guard
        .as_mut()
        .ok_or_else(|| "connection store unavailable".to_string())?;
    action(store)
}

#[tauri::command]
pub async fn list_connections(state: State<'_, AppState>) -> UiResult<Vec<ConnectionSummary>> {
    with_store(&state, |store| {
        Ok(store.list().iter().map(summarise).collect())
    })
    .await
}

#[tauri::command]
pub async fn save_connection(
    state: State<'_, AppState>,
    input: ConnectionInput,
) -> UiResult<ConnectionSummary> {
    with_store(&state, |store| {
        let saved = match input.id.as_deref().and_then(|id| store.get(id)).cloned() {
            Some(existing) => {
                let updated = Connection {
                    name: input.name,
                    host: input.host,
                    port: input.port,
                    database: input.database,
                    username: input.username,
                    // An empty password field means "leave it alone", so
                    // editing a connection's name does not wipe its password.
                    password: if input.password.is_empty() {
                        existing.password.clone()
                    } else {
                        Secret::new(input.password)
                    },
                    production: input.production,
                    ..existing
                };
                store
                    .update(updated.clone())
                    .map_err(|error| error.to_string())?;
                updated
            }
            None => {
                let mut created = Connection::new(
                    input.name,
                    Engine::Postgres,
                    input.host,
                    input.port,
                    input.database,
                    input.username,
                    Secret::new(input.password),
                );
                created.production = input.production;
                store
                    .add(created.clone())
                    .map_err(|error| error.to_string())?;
                created
            }
        };
        Ok(summarise(&saved))
    })
    .await
}

#[tauri::command]
pub async fn delete_connection(state: State<'_, AppState>, id: String) -> UiResult<()> {
    with_store(&state, |store| {
        store.remove(&id).map_err(|error| error.to_string())
    })
    .await
}

/// Sets or clears a connection's production flag.
///
/// Ungated: docs/11-production-safety-flag.md restricts this to a
/// connection's admin only once roles exist in phase 4.
#[tauri::command]
pub async fn set_production_flag(
    state: State<'_, AppState>,
    id: String,
    production: bool,
) -> UiResult<()> {
    with_store(&state, |store| {
        store
            .set_production(&id, production)
            .map_err(|error| error.to_string())
    })
    .await?;

    // Keep an open connection's indicator honest if its flag just changed.
    if let Some(active) = state.active.lock().await.as_mut() {
        if active.id == id {
            active.production = production;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn connect(state: State<'_, AppState>, id: String) -> UiResult<ActiveConnectionInfo> {
    let connection = with_store(&state, |store| {
        store
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no connection with id {id}"))
    })
    .await?;

    let adapter = PostgresAdapter::connect(PostgresConnectionDetails {
        host: &connection.host,
        port: connection.port,
        database: &connection.database,
        username: &connection.username,
        // The only place the credential is read, and it goes straight to the
        // driver (docs/06-credential-vault.md).
        password: connection.password.expose(),
    })
    .await
    .map_err(|error| error.to_string())?;

    let schema = adapter
        .describe_schema()
        .await
        .map_err(|error| error.to_string())?;

    let info = describe_active(
        &connection.id,
        &connection.name,
        connection.production,
        &schema,
    );

    *state.active.lock().await = Some(ActiveConnection {
        id: connection.id,
        name: connection.name,
        production: connection.production,
        adapter: Box::new(adapter),
        schema,
    });

    // Switching connections must not leave a preview from the old one
    // confirmable.
    *state.pending.lock().await = None;

    Ok(info)
}

fn describe_active(
    id: &str,
    name: &str,
    production: bool,
    schema: &Schema,
) -> ActiveConnectionInfo {
    ActiveConnectionInfo {
        id: id.to_string(),
        name: name.to_string(),
        production,
        tables: schema
            .tables
            .iter()
            .map(|table| TableSummary {
                name: table.display_name(),
                columns: table.columns.iter().map(|c| c.name.clone()).collect(),
            })
            .collect(),
    }
}

#[tauri::command]
pub async fn active_connection(
    state: State<'_, AppState>,
) -> UiResult<Option<ActiveConnectionInfo>> {
    Ok(state
        .active
        .lock()
        .await
        .as_ref()
        .map(|active| describe_active(&active.id, &active.name, active.production, &active.schema)))
}

/// Parses and submits a command (docs/05 steps 1 to 5).
#[tauri::command]
pub async fn submit_command(state: State<'_, AppState>, text: String) -> UiResult<CommandOutcome> {
    run_command(&state, text).await
}

/// Revises a pending write and returns to the preview step (docs/05 step 8).
///
/// The pending write is discarded first, so a revised command can never be
/// confirmed against the previous preview.
#[tauri::command]
pub async fn edit_command(state: State<'_, AppState>, text: String) -> UiResult<CommandOutcome> {
    *state.pending.lock().await = None;
    run_command(&state, text).await
}

async fn run_command(state: &AppState, text: String) -> UiResult<CommandOutcome> {
    let active = state.active.lock().await;
    let active = active
        .as_ref()
        .ok_or_else(|| "connect to a database first".to_string())?;

    let intent = mydb_parser::parse(&text, &active.schema, Engine::Postgres)
        .map_err(|error| error.to_string())?;

    let step = mydb_confirmation::begin(active.adapter.as_ref(), intent.clone())
        .await
        .map_err(|error| error.to_string())?;

    match step {
        mydb_confirmation::Step::ReadComplete(records) => {
            // A read leaves nothing pending: there is nothing to confirm.
            *state.pending.lock().await = None;
            Ok(CommandOutcome::ReadComplete {
                description: intent.describe(),
                records: view(&records),
            })
        }
        mydb_confirmation::Step::AwaitingConfirmation(pending) => {
            let outcome = CommandOutcome::NeedsConfirmation {
                description: pending.description(),
                operation: pending.intent().operation.verb().to_string(),
                preview: preview_view(pending.preview()),
                destructive: pending.intent().is_destructive(),
                affects_everything: pending.intent().filter.matches_everything(),
                production: active.production,
            };
            *state.pending.lock().await = Some(*pending);
            Ok(outcome)
        }
    }
}

/// Runs the pending write (docs/05 step 10).
///
/// Takes the pending write out of state, so a second confirmation has nothing
/// to act on. Combined with `confirm` consuming it, a double-click cannot run
/// a delete twice.
#[tauri::command]
pub async fn confirm_command(state: State<'_, AppState>) -> UiResult<ExecutionSummary> {
    let pending = state
        .pending
        .lock()
        .await
        .take()
        .ok_or_else(|| "there is nothing waiting to be confirmed".to_string())?;

    let active = state.active.lock().await;
    let active = active
        .as_ref()
        .ok_or_else(|| "the connection was closed before this could run".to_string())?;

    let completed = pending
        .confirm(active.adapter.as_ref())
        .await
        .map_err(|error| error.to_string())?;

    Ok(ExecutionSummary {
        description: completed.description,
        rows_affected: completed.outcome.rows_affected,
    })
}

/// Discards the pending write (docs/05 step 9).
#[tauri::command]
pub async fn cancel_command(state: State<'_, AppState>) -> UiResult<String> {
    let pending = state
        .pending
        .lock()
        .await
        .take()
        .ok_or_else(|| "there is nothing waiting to be cancelled".to_string())?;

    Ok(pending.cancel().summary)
}
