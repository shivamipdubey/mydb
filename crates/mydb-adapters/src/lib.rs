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

pub use error::AdapterError;
pub use health::{Health, HealthStatus};

use mydb_core::Schema;

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
}
