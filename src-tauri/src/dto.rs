//! What crosses the boundary to the frontend.
//!
//! These types exist separately from the domain types for one reason above
//! all: a connection's password must never be sent to the interface, and the
//! surest way to guarantee that is for the type the interface receives to
//! have no field it could travel in (docs/16 item 2).

use serde::{Deserialize, Serialize};

/// A saved connection, as the interface sees it. Note the absence of a
/// password field; this is deliberate and should stay that way.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionSummary {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub production: bool,
}

/// The details needed to save a connection.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub production: bool,
}

/// A table, for the schema list on the connection screen.
#[derive(Debug, Clone, Serialize)]
pub struct TableSummary {
    pub name: String,
    pub columns: Vec<String>,
}

/// The connection currently open.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveConnectionInfo {
    pub id: String,
    pub name: String,
    pub production: bool,
    pub tables: Vec<TableSummary>,
}

/// A set of records, for display.
#[derive(Debug, Clone, Serialize)]
pub struct RecordsView {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total_count: u64,
    pub truncated: bool,
    /// The query that produced this, shown only as expandable secondary
    /// detail (docs/12-ui-ux-guidelines.md).
    pub statement: String,
}

/// One column of a table, for a schema operation's preview.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnView {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

/// A table about to be emptied or dropped: what is in it, and how much.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableView {
    pub columns: Vec<ColumnView>,
    pub row_count: u64,
    pub statement: String,
}

/// What a preview is showing.
///
/// Two shapes, because a schema operation and a record operation are
/// genuinely different questions (docs/04-database-adapters.md). The
/// interface renders them differently rather than pretending a dropped
/// table is a list of rows.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "previewKind", rename_all = "camelCase")]
pub enum PreviewView {
    Records(RecordsView),
    Table(TableView),
}

/// What the user must type before a write on a production-flagged
/// connection can run (docs/11-production-safety-flag.md).
///
/// The interface uses this to disable the confirm action and to say what is
/// wanted. It is not the enforcement point: the engine checks the typed value
/// again, because a gate enforced only in the interface is not a gate.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExtraStepView {
    None,
    #[serde(rename_all = "camelCase")]
    TableName {
        table: String,
        prompt: String,
    },
    #[serde(rename_all = "camelCase")]
    CountOrConfirm {
        count: u64,
        prompt: String,
    },
    #[serde(rename_all = "camelCase")]
    ConfirmWord {
        prompt: String,
    },
}

/// What came back from submitting a command.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandOutcome {
    /// A read ran and here is the result. No confirmation was needed.
    #[serde(rename_all = "camelCase")]
    ReadComplete {
        description: String,
        records: RecordsView,
    },

    /// A write is waiting. Nothing has happened yet.
    #[serde(rename_all = "camelCase")]
    NeedsConfirmation {
        /// The intent in plain language (docs/12).
        description: String,
        /// Which operation this is, so the interface can say "will be
        /// created" rather than "will be affected" for a record that does
        /// not exist yet.
        operation: String,
        preview: PreviewView,
        extra_step: ExtraStepView,
        /// Whether this destroys data, which decides how loudly the interface
        /// says so.
        destructive: bool,
        /// Whether the filter matches the entire table. Called out separately
        /// because an unfiltered delete is the most damaging thing a user can
        /// confirm by accident.
        affects_everything: bool,
        production: bool,
    },
}

/// The result of confirming a write.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub description: String,
    pub rows_affected: u64,
    /// Set when the write succeeded but could not be recorded in the audit
    /// log. Surfaced rather than swallowed: a user who believes their writes
    /// are being recorded should be told when one was not.
    pub record_warning: Option<String>,
}
