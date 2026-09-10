//! One adapter per database engine, each implementing the same interface:
//! connect, describe schema, build preview, execute, report health
//! (docs/02-architecture.md, docs/04-database-adapters.md).
//!
//! No adapter reaches into another adapter's code, and engine-specific naming
//! stays inside that engine's module (docs/17-coding-standards.md). Preview
//! and execute arrive in T7, and are deliberately absent from the trait until
//! then: an adapter that cannot execute cannot execute unsafely.

mod error;
mod health;
pub mod postgres;
mod preview;
mod records;
mod sql;

pub use error::AdapterError;
pub use health::{Health, HealthStatus};
pub use preview::{ApprovedWrite, ExecutionOutcome, Preview};
pub use records::{Record, RecordSet, SAMPLE_LIMIT};

use mydb_core::{Intent, Schema};

/// What every database adapter must provide.
///
/// Fallible operations return a typed result rather than panicking, per
/// docs/17-coding-standards.md, so a driver failure surfaces as something the
/// UI can render instead of tearing down the process.
#[async_trait::async_trait]
pub trait Adapter: Send + Sync {
    /// Reads the structure of the connected database.
    async fn describe_schema(&self) -> Result<Schema, AdapterError>;

    /// Reports whether the connection is currently usable.
    ///
    /// docs/13-dashboard-and-health-monitoring.md requires this to be
    /// lightweight and strictly read-only: never a query that could be
    /// mistaken for a data-changing operation.
    async fn report_health(&self) -> Health;

    /// Runs the engine's read equivalent of a write intent and returns what
    /// the write would affect (docs/04-database-adapters.md).
    ///
    /// The mechanic differs per engine, which is why this is named for what
    /// it does rather than how: a SELECT for SQL engines, a find() for
    /// MongoDB in phase 2 (docs/17-coding-standards.md).
    async fn build_preview(&self, intent: &Intent) -> Result<Preview, AdapterError>;

    /// Runs a read and returns its records.
    ///
    /// docs/05-confirmation-workflow.md step 4: a read runs directly and its
    /// result is shown. Reads do not need confirmation, because they change
    /// nothing.
    async fn run_read(&self, intent: &Intent) -> Result<RecordSet, AdapterError>;

    /// Runs a write the user has confirmed.
    ///
    /// Takes an [`ApprovedWrite`] rather than an intent. That token can only
    /// be produced from a [`Preview`], which only an adapter can create by
    /// actually querying the database, so there is no way to reach this
    /// function without the user's preview having run first. See the
    /// `preview` module for why this is a type and not a convention.
    async fn execute(&self, approved: ApprovedWrite) -> Result<ExecutionOutcome, AdapterError>;
}
