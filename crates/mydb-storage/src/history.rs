//! The basic command history (docs/03-phases-roadmap.md, phase 1).
//!
//! This is deliberately not the audit log. docs/07-audit-log-and-recovery-bin.md
//! describes a store holding before and after state, with a recovery bin
//! behind it and a 30-day undo window; that is phase 2 work, and building a
//! half-version of it now would be worse than having none, because a store
//! that looks like an audit log invites people to trust it as one.
//!
//! What this holds is the minimum: that a write happened, what it was meant
//! to do, and whether it worked.
//!
//! ## Two rules the format exists to serve
//!
//! It is append-only. An entry is written with the file opened for append, so
//! recording one cannot rewrite or truncate what is already there. Nothing in
//! this module edits or deletes an entry.
//!
//! It is one JSON object per line, not a JSON array, for the same reason.
//! Appending to an array means rewriting the closing bracket, which means
//! reading and rewriting the file, which is exactly the operation an
//! append-only log should not perform. It also means the file can be read
//! with `cat` or `tail`, which matters while there is no viewer screen.
//!
//! ## What is deliberately absent
//!
//! No row data, before or after. No restore action. Nothing that could be
//! mistaken for a recovery path.
//!
//! A failure records only that it failed, without the driver's message. A
//! database error can carry row values inside it, such as the key value in a
//! duplicate-key violation, so storing the message would smuggle row data
//! into a store that must not have any.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Whether a write worked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    /// The write was attempted and the database refused it. The reason is
    /// deliberately not recorded; see the module comment.
    Failure,
}

/// One executed write.
///
/// Five fields, and no more: when, which connection, what kind of operation,
/// what it was meant to do, and whether it worked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// RFC 3339, in UTC.
    pub recorded_at: String,
    /// The connection's name, which is what a person reading this file needs.
    /// Phase 2's audit log keys by connection id instead, per
    /// docs/15-data-model.md.
    pub connection: String,
    /// The operation, as the intent named it: delete, insert, update,
    /// drop_table, truncate.
    pub operation: String,
    /// The parsed intent in plain language. This is the command, not the
    /// data: it contains what the user typed, never what the table held.
    pub intent: String,
    pub result: Outcome,
}

/// Errors from reading or appending to the history.
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("could not append to the command history at {path}: {kind}")]
    Write { path: PathBuf, kind: io::ErrorKind },

    #[error("could not read the command history at {path}: {kind}")]
    Read { path: PathBuf, kind: io::ErrorKind },

    #[error("could not determine a config directory for this user")]
    NoConfigDirectory,

    #[error("could not import the command history into the audit log: {detail}")]
    Import { detail: String },
}

/// The append-only command history.
#[derive(Debug, Clone)]
pub struct CommandHistory {
    path: PathBuf,
}

impl CommandHistory {
    /// The default location: alongside the connection list.
    pub fn default_path() -> Result<PathBuf, HistoryError> {
        let base = dirs::config_dir().ok_or(HistoryError::NoConfigDirectory)?;
        Ok(base.join("MYDB").join("command-history.jsonl"))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn open_default() -> Result<Self, HistoryError> {
        Ok(Self::at(Self::default_path()?))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Records a write that has already been attempted.
    ///
    /// Call this only after the operation has finished, whichever way it
    /// went. docs/16-security-and-cybersafety-checklist.md item 7 forbids
    /// writing to a log before the real operation completes, because an entry
    /// for something that never happened is worse than no entry at all.
    pub fn record(
        &self,
        connection: &str,
        operation: &str,
        intent: &str,
        result: Outcome,
    ) -> Result<HistoryEntry, HistoryError> {
        let entry = HistoryEntry {
            recorded_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            connection: connection.to_string(),
            operation: operation.to_string(),
            intent: intent.to_string(),
            result,
        };
        self.append(&entry)?;
        Ok(entry)
    }

    /// Records an attempted write from the intent that produced it.
    ///
    /// The operation and the summary are both derived from the intent, so
    /// they cannot disagree with each other or with what the user was shown
    /// on the confirmation screen.
    pub fn record_intent(
        &self,
        connection: &str,
        intent: &mydb_core::Intent,
        result: Outcome,
    ) -> Result<HistoryEntry, HistoryError> {
        self.record(
            connection,
            intent.operation.verb(),
            &intent.describe(),
            result,
        )
    }

    fn append(&self, entry: &HistoryEntry) -> Result<(), HistoryError> {
        let write_error = |error: io::Error| HistoryError::Write {
            path: self.path.clone(),
            kind: error.kind(),
        };

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(write_error)?;
        }

        let line = serde_json::to_string(entry).map_err(|_| HistoryError::Write {
            path: self.path.clone(),
            kind: io::ErrorKind::InvalidData,
        })?;

        // Opened for append, never for write or truncate, so an existing
        // entry cannot be overwritten even by a mistake in this file.
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(write_error)?;

        restrict_to_owner(&self.path)?;

        writeln!(file, "{line}").map_err(write_error)?;
        file.flush().map_err(write_error)
    }

    /// Reads every entry, oldest first.
    ///
    /// A missing file is an empty history, which is the normal state before
    /// the first write.
    pub fn read_all(&self) -> Result<Vec<HistoryEntry>, HistoryError> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(HistoryError::Read {
                    path: self.path.clone(),
                    kind: error.kind(),
                })
            }
        };

        Ok(contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            // A line that will not parse is skipped rather than failing the
            // whole read. A truncated final line from an interrupted write
            // should not make the rest of the history unreadable.
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    pub fn len(&self) -> Result<usize, HistoryError> {
        Ok(self.read_all()?.len())
    }

    pub fn is_empty(&self) -> Result<bool, HistoryError> {
        Ok(self.len()? == 0)
    }

    /// Moves this file's entries into the audit log, then stops using it.
    ///
    /// The phase 1 history could never hold before or after state, so the
    /// imported rows are flagged as predating state capture rather than
    /// dropped: the record of what a user did stays continuous, and nobody
    /// mistakes an old entry's absent state for a capture that failed.
    ///
    /// The file itself is left on disk untouched. It is append-only, and
    /// deleting it to tidy up would destroy the only original copy of
    /// something this import might have got wrong.
    pub fn import_into(&self, log: &crate::AuditLog<'_>) -> Result<usize, HistoryError> {
        let entries = self.read_all()?;
        for entry in &entries {
            log.import_legacy(
                &entry.connection,
                &entry.recorded_at,
                &entry.operation,
                &entry.intent,
                entry.result,
            )
            .map_err(|error| HistoryError::Import {
                detail: error.to_string(),
            })?;
        }
        Ok(entries.len())
    }
}

/// Restricts the file to owner read and write.
///
/// An intent summary contains the values the user typed in their command, for
/// example an email address in a filter, so another account on the machine
/// should not be able to read it.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<(), HistoryError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        HistoryError::Write {
            path: path.to_path_buf(),
            kind: error.kind(),
        }
    })
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<(), HistoryError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn history() -> (tempfile::TempDir, CommandHistory) {
        let dir = tempfile::tempdir().unwrap();
        let history = CommandHistory::at(dir.path().join("command-history.jsonl"));
        (dir, history)
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        let (_dir, history) = history();
        assert!(history.is_empty().unwrap());
    }

    #[test]
    fn an_entry_holds_exactly_the_five_phase_one_fields() {
        let (_dir, history) = history();
        history
            .record(
                "Local test database",
                "delete",
                "Delete records in users where active is false",
                Outcome::Success,
            )
            .unwrap();

        let raw = fs::read_to_string(history.path()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        let object = parsed.as_object().unwrap();

        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["connection", "intent", "operation", "recorded_at", "result"],
            "the phase 1 history must hold nothing beyond these five fields"
        );
    }

    #[test]
    fn nothing_resembling_row_data_or_a_restore_action_is_stored() {
        let (_dir, history) = history();
        history
            .record(
                "c",
                "delete",
                "Delete every record in users",
                Outcome::Success,
            )
            .unwrap();

        let raw = fs::read_to_string(history.path()).unwrap().to_lowercase();
        for absent in ["before", "after", "restore", "recover", "undo", "rows"] {
            assert!(
                !raw.contains(absent),
                "{absent:?} has no place in the phase 1 history: {raw}"
            );
        }
    }

    #[test]
    fn appending_never_rewrites_an_earlier_entry() {
        let (_dir, history) = history();
        for index in 0..5 {
            history
                .record("c", "delete", &format!("command {index}"), Outcome::Success)
                .unwrap();
        }

        let entries = history.read_all().unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].intent, "command 0", "oldest entry first");
        assert_eq!(entries[4].intent, "command 4");
    }

    #[test]
    fn a_failure_is_recorded_as_a_failure() {
        let (_dir, history) = history();
        history
            .record("c", "delete", "Delete records in users", Outcome::Failure)
            .unwrap();
        assert_eq!(history.read_all().unwrap()[0].result, Outcome::Failure);
    }

    #[test]
    fn a_truncated_final_line_does_not_make_the_rest_unreadable() {
        let (_dir, history) = history();
        history
            .record("c", "delete", "first", Outcome::Success)
            .unwrap();

        // Simulates a write interrupted partway.
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(history.path())
            .unwrap();
        writeln!(file, "{{\"recorded_at\":\"2026").unwrap();

        let entries = history.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].intent, "first");
    }

    #[test]
    #[cfg(unix)]
    fn the_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, history) = history();
        history
            .record(
                "c",
                "delete",
                "Delete users where email is ada@example.com",
                Outcome::Success,
            )
            .unwrap();

        let mode = fs::metadata(history.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "an intent summary contains the values the user typed, so another \
             account on this machine should not be able to read it"
        );
    }
}

#[cfg(test)]
mod import_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::{AuditFilter, AuditLog};

    #[test]
    fn importing_carries_every_entry_across_and_flags_it() {
        let dir = tempfile::tempdir().unwrap();
        let history = CommandHistory::at(dir.path().join("command-history.jsonl"));
        history
            .record(
                "Local test database",
                "delete",
                "Delete every record in users",
                Outcome::Success,
            )
            .unwrap();
        history
            .record(
                "Local test database",
                "update",
                "Update records in users",
                Outcome::Failure,
            )
            .unwrap();

        let db = crate::database::open_in_memory().unwrap();
        let log = AuditLog::new(&db);
        assert_eq!(history.import_into(&log).unwrap(), 2);

        let entries = log.list(&AuditFilter::default()).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.predates_state_capture));
        // Order and outcome are preserved, not flattened to success.
        assert!(entries.iter().any(|entry| entry.result == Outcome::Failure));
        assert!(entries
            .iter()
            .any(|entry| entry.intent_summary == "Delete every record in users"));
    }

    #[test]
    fn importing_leaves_the_original_file_on_disk() {
        // It is append-only, and deleting it to tidy up would destroy the
        // only original copy of anything the import got wrong.
        let dir = tempfile::tempdir().unwrap();
        let history = CommandHistory::at(dir.path().join("command-history.jsonl"));
        history
            .record("c", "delete", "Delete", Outcome::Success)
            .unwrap();

        let db = crate::database::open_in_memory().unwrap();
        history.import_into(&AuditLog::new(&db)).unwrap();

        assert!(history.path().exists());
        assert_eq!(history.len().unwrap(), 1);
    }

    #[test]
    fn importing_an_absent_history_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let history = CommandHistory::at(dir.path().join("never-written.jsonl"));
        let db = crate::database::open_in_memory().unwrap();
        assert_eq!(history.import_into(&AuditLog::new(&db)).unwrap(), 0);
    }
}
