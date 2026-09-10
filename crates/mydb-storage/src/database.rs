//! The local database holding MYDB's audit log and recovery bin.
//!
//! docs/02-architecture.md calls for both to live in a local database rather
//! than a flat file, and they share one file so that an audit entry's
//! reference to a recovery entry is a real foreign key rather than a number
//! that might point at nothing.
//!
//! Schema changes go through [`migrate`], which is versioned, because this
//! file holds the only copy of data a user may later need to recover. A
//! migration that silently failed to apply would be discovered at the worst
//! possible moment.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// Errors from the local database.
///
/// Messages name the file and the failure and nothing else. Entries in this
/// database describe operations on a user's data, so an error rendered into a
/// log or the interface must not carry that data with it.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not open the local database at {path}")]
    Open { path: PathBuf },

    #[error("could not prepare the local database at {path}: {detail}")]
    Migrate { path: PathBuf, detail: String },

    #[error("the local database rejected the request: {detail}")]
    Query { detail: String },

    #[error("could not determine a config directory for this user")]
    NoConfigDirectory,

    #[error("no {kind} entry with id {id}")]
    NotFound { kind: &'static str, id: i64 },
}

/// The schema version this build expects.
const SCHEMA_VERSION: i64 = 1;

/// Opens the local database, creating and migrating it as needed.
pub fn open(path: impl AsRef<Path>) -> Result<Connection, StoreError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StoreError::Open {
            path: path.to_path_buf(),
        })?;
    }

    let connection = Connection::open(path).map_err(|_| StoreError::Open {
        path: path.to_path_buf(),
    })?;

    restrict_to_owner(path)?;
    migrate(&connection, path)?;
    Ok(connection)
}

/// Opens an in-memory database, for tests.
pub fn open_in_memory() -> Result<Connection, StoreError> {
    let connection = Connection::open_in_memory().map_err(|_| StoreError::Open {
        path: PathBuf::from(":memory:"),
    })?;
    migrate(&connection, Path::new(":memory:"))?;
    Ok(connection)
}

/// The default location, alongside the connection list.
pub fn default_path() -> Result<PathBuf, StoreError> {
    let base = dirs::config_dir().ok_or(StoreError::NoConfigDirectory)?;
    Ok(base.join("MYDB").join("mydb.sqlite3"))
}

/// Brings the database up to [`SCHEMA_VERSION`].
fn migrate(connection: &Connection, path: &Path) -> Result<(), StoreError> {
    let fail = |detail: String| StoreError::Migrate {
        path: path.to_path_buf(),
        detail,
    };

    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| fail(error.to_string()))?;

    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| fail(error.to_string()))?;

    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    connection
        .execute_batch(SCHEMA_V1)
        .map_err(|error| fail(error.to_string()))?;
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| fail(error.to_string()))?;

    Ok(())
}

/// Version 1: the audit log.
///
/// The recovery bin table arrives with the recovery bin itself, in its own
/// migration. `recovery_entry_id` is present now but unconstrained until
/// then, so an audit entry written before the bin exists cannot claim a
/// reference it could not have.
///
/// The two triggers are the point of this schema. docs/07-audit-log-and-recovery-bin.md
/// says the audit log is append-only and that nothing in it is ever edited or
/// deleted by MYDB itself, including during a purge. Enforcing that only in
/// Rust would leave it true by convention; enforcing it in the database means
/// a future code path that tries cannot succeed, whatever it intended.
const SCHEMA_V1: &str = r#"
CREATE TABLE audit_log (
    id                INTEGER PRIMARY KEY,
    connection_id     TEXT    NOT NULL,
    -- The connection's name as it stood when the write ran. Kept so an entry
    -- still reads sensibly after the connection is deleted; per
    -- docs/15-data-model.md entries detach rather than disappear, and whether
    -- a connection still exists is decided at read time so that no entry has
    -- to be edited to record it.
    connection_name   TEXT    NOT NULL,
    recorded_at       TEXT    NOT NULL,
    operation         TEXT    NOT NULL,
    intent_summary    TEXT    NOT NULL,
    result            TEXT    NOT NULL CHECK (result IN ('success', 'failure')),
    size_tier         TEXT    NOT NULL CHECK (size_tier IN ('small', 'large')),
    affected_count    INTEGER NOT NULL,
    -- Full state, for a small operation only. docs/07 forbids holding full
    -- detail in both the log and the recovery bin.
    before_state      TEXT,
    after_state       TEXT,
    -- For a large operation: a sample, plus the reference to the recovery
    -- entry that holds the whole thing.
    state_sample      TEXT,
    recovery_entry_id INTEGER,
    -- Set on the rows imported from the phase 1 command history, which was
    -- never able to hold before or after state. Flagged rather than dropped,
    -- so the history stays continuous and nobody mistakes an old entry's
    -- empty state for a capture that failed.
    predates_state_capture INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX audit_log_connection ON audit_log (connection_id, recorded_at);
CREATE INDEX audit_log_recorded_at ON audit_log (recorded_at);

CREATE TRIGGER audit_log_is_append_only_update
BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'the audit log is append-only: entries are never edited');
END;

CREATE TRIGGER audit_log_is_append_only_delete
BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'the audit log is append-only: entries are never deleted');
END;
"#;

/// Restricts the database file to owner read and write.
///
/// Entries describe operations on a user's data and, for small operations,
/// hold that data outright. Another account on the machine has no business
/// reading it.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|_| {
        StoreError::Open {
            path: path.to_path_buf(),
        }
    })
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}

pub(crate) fn query_error(error: rusqlite::Error) -> StoreError {
    StoreError::Query {
        detail: error.to_string(),
    }
}
