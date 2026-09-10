//! What the desktop shell holds between commands.
//!
//! The pending write lives here rather than in the frontend. That matters:
//! confirming a write must act on the preview the backend actually produced,
//! not on something the frontend hands back, or the safety guarantee would
//! only be as strong as the frontend's honesty.

use mydb_adapters::Adapter;
use mydb_confirmation::PendingWrite;
use mydb_core::Schema;
use mydb_storage::ConnectionStore;
use tokio::sync::Mutex;

/// A connection the user has opened.
pub struct ActiveConnection {
    pub id: String,
    pub name: String,
    pub production: bool,
    pub adapter: Box<dyn Adapter>,
    /// Read once on connect. The parser needs it to resolve table and column
    /// names, and the connection manager shows it.
    pub schema: Schema,
}

/// Application state, shared across commands.
#[derive(Default)]
pub struct AppState {
    pub connections: Mutex<Option<ConnectionStore>>,
    pub active: Mutex<Option<ActiveConnection>>,
    /// The write awaiting the user's decision, if any.
    ///
    /// Only ever one: a second command replaces it, which is correct, because
    /// a preview the user has moved on from should not remain confirmable.
    pub pending: Mutex<Option<PendingWrite>>,
    /// The local database holding the audit log and the recovery bin.
    ///
    /// Shared rather than owned by one place: a write's capture streams into
    /// it from the sink handed to the adapter, while the audit entry is
    /// appended from the command that ran the write. A standard mutex rather
    /// than an async one, because every critical section here is a single
    /// statement and the guard is never held across an await.
    pub records: crate::recording::Records,
}
