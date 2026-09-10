//! The recovery bin (docs/07-audit-log-and-recovery-bin.md).
//!
//! Holds the full before-state of anything deleted or overwritten, at any
//! size, for 30 days. It is the recovery path: docs/01-prd.md rules out
//! automatic rollback of a completed operation, so this is what stands
//! between a user and a mistake.
//!
//! ## Purging clears, it does not delete
//!
//! An expired entry keeps its row and loses its payload, and is marked with
//! the date it was purged. Two reasons. The audit log holds a reference to
//! this entry for any large operation, and that reference must resolve
//! forever, answering "this existed and was purged on this date" rather than
//! pointing at nothing. And recording the loss any other way would mean
//! editing an audit entry, which docs/07 forbids and the audit log's triggers
//! physically prevent.
//!
//! ## Staging
//!
//! A large operation's before-state cannot be held in memory. Records stream
//! into a staging area while the write's transaction is open, and become a
//! real expiring entry only once the write has committed. If the write fails
//! or rolls back, the staged records are discarded: they describe something
//! that never happened, and keeping them would be worse than keeping nothing.

use mydb_core::{RecordSnapshot, StateSnapshot};
use rusqlite::{params, Connection, OptionalExtension};

use crate::clock::Clock;
use crate::database::{query_error, StoreError};

/// How long an entry is kept, in days.
///
/// docs/07 fixes this at 30 for v1 and says not to make it configurable
/// without checking docs/01-prd.md first. The per-connection cap is the
/// setting the user can change; this is not.
pub const RETENTION_DAYS: i64 = 30;

/// How much recovery data one connection may hold before the oldest entries
/// purge early.
///
/// docs/07 requires a cap and a user setting for it but names no default.
/// This one is a judgement, not a specification: large enough that ordinary
/// use never reaches it, small enough that a runaway operation cannot fill a
/// disk unnoticed. T17 exposes the setting.
pub const DEFAULT_CONNECTION_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// One recoverable operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub id: i64,
    pub connection_id: String,
    pub connection_name: String,
    /// The audit entry describing this write, once it has been written.
    pub audit_log_id: Option<i64>,
    pub operation: String,
    pub intent_summary: String,
    pub created_at: String,
    pub expires_at: String,
    /// The full before-state. `None` once purged; the entry itself remains.
    pub payload: Option<StateSnapshot>,
    pub record_count: u64,
    pub payload_bytes: u64,
    pub purged: bool,
    pub purged_at: Option<String>,
    /// True when the purge happened because the connection's cap was reached
    /// rather than because the entry expired.
    pub purged_early: bool,
}

impl RecoveryEntry {
    /// Whether this entry can still be restored from.
    ///
    /// A purged entry cannot: its payload is gone, and restoring from an
    /// entry that no longer holds the data would mean inventing it.
    pub fn is_restorable(&self) -> bool {
        !self.purged && self.payload.is_some()
    }
}

/// What to store.
#[derive(Debug, Clone)]
pub struct NewRecoveryEntry<'a> {
    pub connection_id: &'a str,
    pub connection_name: &'a str,
    pub operation: &'a str,
    pub intent_summary: &'a str,
    pub before: StateSnapshot,
}

/// What a purge did, so the user can be told when it was not routine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub entries_purged: usize,
    pub bytes_freed: u64,
    /// True when entries went because a cap was reached rather than because
    /// they expired. docs/07 requires warning the user about that case.
    pub early: bool,
}

impl PurgeReport {
    pub fn purged_anything(&self) -> bool {
        self.entries_purged > 0
    }
}

/// Which entries to read.
#[derive(Debug, Clone, Default)]
pub struct RecoveryFilter {
    pub connection_id: Option<String>,
    /// Purged entries are excluded by default: the bin is a place to recover
    /// from, and an entry with no payload cannot be recovered from.
    pub include_purged: bool,
    pub limit: Option<u32>,
}

/// The recovery bin.
pub struct RecoveryBin<'a> {
    connection: &'a Connection,
    clock: &'a dyn Clock,
    cap_bytes: u64,
}

impl<'a> RecoveryBin<'a> {
    pub fn new(connection: &'a Connection, clock: &'a dyn Clock) -> Self {
        Self {
            connection,
            clock,
            cap_bytes: DEFAULT_CONNECTION_CAP_BYTES,
        }
    }

    /// Overrides the per-connection cap. The setting behind this is T17.
    pub fn with_cap(mut self, cap_bytes: u64) -> Self {
        self.cap_bytes = cap_bytes;
        self
    }

    /// Stores a before-state that is already in memory.
    ///
    /// For an operation small enough to have been captured whole. A large one
    /// goes through [`RecoveryBin::stage`] instead.
    pub fn store(&self, entry: NewRecoveryEntry<'_>) -> Result<RecoveryEntry, StoreError> {
        let payload = serde_json::to_string(&entry.before).map_err(|error| StoreError::Query {
            detail: format!("could not encode captured state: {error}"),
        })?;
        self.insert(
            entry.connection_id,
            entry.connection_name,
            entry.operation,
            entry.intent_summary,
            &payload,
            entry.before.total(),
        )
    }

    fn insert(
        &self,
        connection_id: &str,
        connection_name: &str,
        operation: &str,
        intent_summary: &str,
        payload: &str,
        record_count: u64,
    ) -> Result<RecoveryEntry, StoreError> {
        let created = self.clock.now();
        let expires = created + chrono::Duration::days(RETENTION_DAYS);

        self.connection
            .execute(
                "INSERT INTO recovery_bin (
                     connection_id, connection_name, operation, intent_summary,
                     created_at, expires_at, payload, record_count, payload_bytes
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    connection_id,
                    connection_name,
                    operation,
                    intent_summary,
                    created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    payload,
                    record_count as i64,
                    payload.len() as i64,
                ],
            )
            .map_err(query_error)?;

        let id = self.connection.last_insert_rowid();
        self.get(id)?.ok_or(StoreError::NotFound {
            kind: "recovery",
            id,
        })
    }

    /// Links an entry to the audit entry that describes the same write.
    ///
    /// Done after the audit entry exists, because the audit log is
    /// append-only and cannot be updated later to add the reverse reference.
    /// The recovery bin is mutable, so this direction is the one that can be
    /// filled in afterwards.
    pub fn link_audit_entry(&self, recovery_id: i64, audit_id: i64) -> Result<(), StoreError> {
        self.connection
            .execute(
                "UPDATE recovery_bin SET audit_log_id = ?2 WHERE id = ?1",
                params![recovery_id, audit_id],
            )
            .map_err(query_error)?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<RecoveryEntry>, StoreError> {
        self.connection
            .query_row(
                &format!("{SELECT_COLUMNS} WHERE id = ?1"),
                params![id],
                read_entry,
            )
            .optional()
            .map_err(query_error)?
            .transpose()
    }

    /// Reads entries, newest first.
    pub fn list(&self, filter: &RecoveryFilter) -> Result<Vec<RecoveryEntry>, StoreError> {
        let mut sql = String::from(SELECT_COLUMNS);
        let mut clauses: Vec<&str> = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !filter.include_purged {
            clauses.push("purged = 0");
        }
        if let Some(id) = &filter.connection_id {
            clauses.push("connection_id = ?");
            values.push(Box::new(id.clone()));
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY id DESC");
        if let Some(limit) = filter.limit {
            sql.push_str(" LIMIT ?");
            values.push(Box::new(limit));
        }

        let mut statement = self.connection.prepare(&sql).map_err(query_error)?;
        let borrowed: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        let rows = statement
            .query_map(borrowed.as_slice(), read_entry)
            .map_err(query_error)?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(query_error)??);
        }
        Ok(entries)
    }

    /// How many bytes of recoverable payload one connection currently holds.
    pub fn bytes_for_connection(&self, connection_id: &str) -> Result<u64, StoreError> {
        let bytes: i64 = self
            .connection
            .query_row(
                "SELECT coalesce(sum(payload_bytes), 0) FROM recovery_bin \
                 WHERE connection_id = ?1 AND purged = 0",
                params![connection_id],
                |row| row.get(0),
            )
            .map_err(query_error)?;
        Ok(bytes.max(0) as u64)
    }

    /// Purges everything past its retention window.
    ///
    /// Clears payloads and marks the entries; the rows stay so the audit
    /// log's references to them keep resolving.
    pub fn purge_expired(&self) -> Result<PurgeReport, StoreError> {
        let now = self.clock.now_rfc3339();
        let doomed: Vec<(i64, i64)> = self.select_ids(
            "SELECT id, payload_bytes FROM recovery_bin \
             WHERE purged = 0 AND expires_at <= ?1",
            params![now],
        )?;
        self.clear(&doomed, false)
    }

    /// Purges a connection's oldest entries until it is under its cap.
    ///
    /// docs/07: when the cap is hit, the oldest entries purge early and the
    /// user is warned. The report says whether anything went, so the warning
    /// is the caller's to show rather than something buried here.
    pub fn enforce_cap(&self, connection_id: &str) -> Result<PurgeReport, StoreError> {
        let mut held = self.bytes_for_connection(connection_id)?;
        if held <= self.cap_bytes {
            return Ok(PurgeReport::default());
        }

        // Oldest first: the newest entry is the one most likely to be needed.
        let candidates: Vec<(i64, i64)> = self.select_ids(
            "SELECT id, payload_bytes FROM recovery_bin \
             WHERE connection_id = ?1 AND purged = 0 ORDER BY id ASC",
            params![connection_id],
        )?;

        let mut doomed = Vec::new();
        for (id, bytes) in candidates {
            if held <= self.cap_bytes {
                break;
            }
            held = held.saturating_sub(bytes.max(0) as u64);
            doomed.push((id, bytes));
        }

        self.clear(&doomed, true)
    }

    fn select_ids(
        &self,
        sql: &str,
        parameters: impl rusqlite::Params,
    ) -> Result<Vec<(i64, i64)>, StoreError> {
        let mut statement = self.connection.prepare(sql).map_err(query_error)?;
        let rows = statement
            .query_map(parameters, |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(query_error)?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(query_error)?);
        }
        Ok(ids)
    }

    fn clear(&self, doomed: &[(i64, i64)], early: bool) -> Result<PurgeReport, StoreError> {
        let now = self.clock.now_rfc3339();
        let mut freed = 0u64;
        for (id, bytes) in doomed {
            self.connection
                .execute(
                    "UPDATE recovery_bin \
                        SET payload = NULL, purged = 1, purged_at = ?2, purged_early = ?3 \
                      WHERE id = ?1",
                    params![id, now, early as i64],
                )
                .map_err(query_error)?;
            freed += (*bytes).max(0) as u64;
        }
        Ok(PurgeReport {
            entries_purged: doomed.len(),
            bytes_freed: freed,
            early: early && !doomed.is_empty(),
        })
    }
}

const SELECT_COLUMNS: &str = "SELECT id, connection_id, connection_name, audit_log_id, operation, \
     intent_summary, created_at, expires_at, payload, record_count, payload_bytes, purged, \
     purged_at, purged_early FROM recovery_bin";

type RowResult = Result<RecoveryEntry, StoreError>;

fn read_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RowResult> {
    let build = || -> RowResult {
        let payload: Option<String> = row.get(8).map_err(query_error)?;
        Ok(RecoveryEntry {
            id: row.get(0).map_err(query_error)?,
            connection_id: row.get(1).map_err(query_error)?,
            connection_name: row.get(2).map_err(query_error)?,
            audit_log_id: row.get(3).map_err(query_error)?,
            operation: row.get(4).map_err(query_error)?,
            intent_summary: row.get(5).map_err(query_error)?,
            created_at: row.get(6).map_err(query_error)?,
            expires_at: row.get(7).map_err(query_error)?,
            payload: match payload {
                None => None,
                Some(text) => {
                    Some(
                        serde_json::from_str(&text).map_err(|error| StoreError::Query {
                            detail: format!("could not decode captured state: {error}"),
                        })?,
                    )
                }
            },
            record_count: row.get::<_, i64>(9).map_err(query_error)?.max(0) as u64,
            payload_bytes: row.get::<_, i64>(10).map_err(query_error)?.max(0) as u64,
            purged: row.get::<_, i64>(11).map_err(query_error)? != 0,
            purged_at: row.get(12).map_err(query_error)?,
            purged_early: row.get::<_, i64>(13).map_err(query_error)? != 0,
        })
    };
    Ok(build())
}

/// A capture in progress, streaming into the staging area.
///
/// Created before a write runs and finished only after it has committed.
/// Nothing here is recovery data yet: it describes a write that may still
/// fail, and a recovery entry for something that never happened would be
/// worse than no entry at all.
///
/// Dropping this without calling [`StagedCapture::finalize`] or
/// [`StagedCapture::discard`] leaves the staged rows behind. That is why
/// [`RecoveryBin::sweep_staging`] exists: a process that dies mid-write
/// should not leave its half-capture to be mistaken for data later.
pub struct StagedCapture {
    staging_id: String,
    records: u64,
    bytes: u64,
}

impl StagedCapture {
    pub fn staging_id(&self) -> &str {
        &self.staging_id
    }

    pub fn records_staged(&self) -> u64 {
        self.records
    }
}

impl<'a> RecoveryBin<'a> {
    /// Begins a streaming capture.
    pub fn stage(&self) -> StagedCapture {
        StagedCapture {
            // Distinct per capture so two writes in flight cannot mix, and
            // random rather than sequential so a crashed capture's rows are
            // never picked up by the next one.
            staging_id: uuid::Uuid::new_v4().to_string(),
            records: 0,
            bytes: 0,
        }
    }

    /// Adds one record to a capture in progress.
    ///
    /// Called as the adapter reads, so a before-state larger than memory
    /// still reaches the bin. There is no row cap: docs/07 says the bin holds
    /// the full before-state regardless of operation size, and the
    /// per-connection byte cap is what bounds it.
    pub fn stage_record(
        &self,
        capture: &mut StagedCapture,
        record: &RecordSnapshot,
    ) -> Result<(), StoreError> {
        let encoded = serde_json::to_string(record).map_err(|error| StoreError::Query {
            detail: format!("could not encode a captured record: {error}"),
        })?;
        self.connection
            .execute(
                "INSERT INTO recovery_staging (staging_id, record) VALUES (?1, ?2)",
                params![capture.staging_id, encoded],
            )
            .map_err(query_error)?;
        capture.records += 1;
        capture.bytes += encoded.len() as u64;
        Ok(())
    }

    /// Turns a completed capture into a real, expiring recovery entry.
    ///
    /// Called only after the write has committed. The staged rows are removed
    /// in the same step, so a capture is never both staged and stored.
    pub fn finalize(
        &self,
        capture: StagedCapture,
        connection_id: &str,
        connection_name: &str,
        operation: &str,
        intent_summary: &str,
    ) -> Result<RecoveryEntry, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT record FROM recovery_staging WHERE staging_id = ?1 ORDER BY id")
            .map_err(query_error)?;
        let rows = statement
            .query_map(params![capture.staging_id], |row| row.get::<_, String>(0))
            .map_err(query_error)?;

        let mut records = Vec::new();
        for row in rows {
            let text = row.map_err(query_error)?;
            records.push(
                serde_json::from_str::<RecordSnapshot>(&text).map_err(|error| {
                    StoreError::Query {
                        detail: format!("could not decode a staged record: {error}"),
                    }
                })?,
            );
        }
        drop(statement);

        let state = StateSnapshot::complete(records);
        let payload = serde_json::to_string(&state).map_err(|error| StoreError::Query {
            detail: format!("could not encode captured state: {error}"),
        })?;

        let entry = self.insert(
            connection_id,
            connection_name,
            operation,
            intent_summary,
            &payload,
            state.total(),
        )?;

        self.discard_staged(&capture.staging_id)?;
        Ok(entry)
    }

    /// Throws away a capture whose write did not happen.
    pub fn discard(&self, capture: StagedCapture) -> Result<(), StoreError> {
        self.discard_staged(&capture.staging_id)
    }

    fn discard_staged(&self, staging_id: &str) -> Result<(), StoreError> {
        self.connection
            .execute(
                "DELETE FROM recovery_staging WHERE staging_id = ?1",
                params![staging_id],
            )
            .map_err(query_error)?;
        Ok(())
    }

    /// Removes any staged rows left behind by a capture that never finished.
    ///
    /// Run at startup. A process killed mid-write leaves rows describing a
    /// write whose outcome is unknown, and an unknown outcome is not
    /// something to offer a user as recoverable data.
    pub fn sweep_staging(&self) -> Result<usize, StoreError> {
        let removed = self
            .connection
            .execute("DELETE FROM recovery_staging", [])
            .map_err(query_error)?;
        Ok(removed)
    }

    pub fn staged_record_count(&self) -> Result<u64, StoreError> {
        let count: i64 = self
            .connection
            .query_row("SELECT count(*) FROM recovery_staging", [], |row| {
                row.get(0)
            })
            .map_err(query_error)?;
        Ok(count.max(0) as u64)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::clock::FixedClock;
    use crate::{AuditLog, AuditRecord, Outcome};
    use mydb_core::{Engine, Filter, Intent, Operation};

    fn record(id: i64) -> RecordSnapshot {
        let mut fields = serde_json::Map::new();
        fields.insert("id".to_string(), serde_json::json!(id));
        fields.insert(
            "email".to_string(),
            serde_json::json!(format!("user{id}@example.com")),
        );
        RecordSnapshot::new(fields)
    }

    fn stored<'a>(bin: &RecoveryBin<'a>, connection: &str, records: u64) -> RecoveryEntry {
        let before = StateSnapshot::complete((1..=records as i64).map(record).collect());
        bin.store(NewRecoveryEntry {
            connection_id: connection,
            connection_name: connection,
            operation: "delete",
            intent_summary: "Delete every record in users",
            before,
        })
        .unwrap()
    }

    #[test]
    fn an_entry_expires_thirty_days_after_it_was_created() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let entry = stored(&RecoveryBin::new(&db, &clock), "c1", 2);

        assert_eq!(entry.created_at, "2026-01-01T00:00:00Z");
        assert_eq!(entry.expires_at, "2026-01-31T00:00:00Z");
        assert_eq!(RETENTION_DAYS, 30);
    }

    #[test]
    fn an_entry_survives_until_its_window_is_actually_up() {
        // docs/18: proven with a manipulated clock, not a 30-day wait.
        let db = crate::database::open_in_memory().unwrap();
        let mut clock = FixedClock::epoch();
        let entry = stored(&RecoveryBin::new(&db, &clock), "c1", 2);

        clock.advance_days(29);
        let report = RecoveryBin::new(&db, &clock).purge_expired().unwrap();
        assert!(!report.purged_anything(), "29 days is not 30");
        assert!(RecoveryBin::new(&db, &clock)
            .get(entry.id)
            .unwrap()
            .unwrap()
            .is_restorable());
    }

    #[test]
    fn purging_clears_the_payload_and_keeps_the_row() {
        // Decision A: the row and the audit reference both stay valid, and
        // resolve to "existed, purged on this date".
        let db = crate::database::open_in_memory().unwrap();
        let mut clock = FixedClock::epoch();
        let entry = stored(&RecoveryBin::new(&db, &clock), "c1", 3);

        clock.advance_days(30);
        let report = RecoveryBin::new(&db, &clock).purge_expired().unwrap();
        assert_eq!(report.entries_purged, 1);
        assert!(report.bytes_freed > 0);
        assert!(!report.early, "expiry is routine, not an early purge");

        let after = RecoveryBin::new(&db, &clock)
            .get(entry.id)
            .unwrap()
            .expect("the row must still be there");
        assert!(after.purged);
        assert_eq!(after.purged_at.as_deref(), Some("2026-01-31T00:00:00Z"));
        assert!(after.payload.is_none(), "the payload is what goes");
        assert!(!after.is_restorable());
        // The description of what happened survives the data.
        assert_eq!(after.record_count, 3);
        assert_eq!(after.intent_summary, "Delete every record in users");
    }

    #[test]
    fn an_audit_entry_reference_still_resolves_after_the_payload_is_purged() {
        let db = crate::database::open_in_memory().unwrap();
        let mut clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);
        let entry = stored(&bin, "c1", 2);

        let intent = Intent {
            engine: Engine::Postgres,
            namespace: "public".to_string(),
            table: "users".to_string(),
            operation: Operation::Delete,
            filter: Filter::everything(),
            assignments: Vec::new(),
        };
        let audit = AuditLog::new(&db)
            .append(AuditRecord {
                connection_id: "c1",
                connection_name: "c1",
                intent: &intent,
                result: Outcome::Success,
                affected_count: 2,
                before: StateSnapshot::empty(),
                after: StateSnapshot::empty(),
                recovery_entry_id: Some(entry.id),
            })
            .unwrap();
        bin.link_audit_entry(entry.id, audit.id).unwrap();

        clock.advance_days(31);
        RecoveryBin::new(&db, &clock).purge_expired().unwrap();

        // The audit entry is untouched, and its reference still leads
        // somewhere that explains itself.
        let log = AuditLog::new(&db);
        let audit_after = log.get(audit.id).unwrap().unwrap();
        assert_eq!(audit_after.recovery_entry_id, Some(entry.id));

        let referenced = RecoveryBin::new(&db, &clock)
            .get(entry.id)
            .unwrap()
            .expect("the reference must not dangle");
        assert!(referenced.purged);
        assert_eq!(referenced.audit_log_id, Some(audit.id));
    }

    #[test]
    fn a_purged_entry_is_excluded_from_the_bin_unless_asked_for() {
        let db = crate::database::open_in_memory().unwrap();
        let mut clock = FixedClock::epoch();
        stored(&RecoveryBin::new(&db, &clock), "c1", 1);
        clock.advance_days(30);
        RecoveryBin::new(&db, &clock).purge_expired().unwrap();

        let bin = RecoveryBin::new(&db, &clock);
        assert!(
            bin.list(&RecoveryFilter::default()).unwrap().is_empty(),
            "the bin is a place to recover from, and this cannot be recovered from"
        );
        assert_eq!(
            bin.list(&RecoveryFilter {
                include_purged: true,
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }

    // --- the per-connection cap ---

    #[test]
    fn reaching_the_cap_purges_the_oldest_entries_and_says_so() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();

        let first = stored(&RecoveryBin::new(&db, &clock), "c1", 2);
        let second = stored(&RecoveryBin::new(&db, &clock), "c1", 2);
        let third = stored(&RecoveryBin::new(&db, &clock), "c1", 2);

        // The cap is derived from what was actually stored rather than
        // guessed at, so the test cannot pass or fail on payload sizes it
        // does not control. Room for two entries, not three.
        let held = RecoveryBin::new(&db, &clock)
            .bytes_for_connection("c1")
            .unwrap();
        let cap = held - first.payload_bytes;

        let bin = RecoveryBin::new(&db, &clock).with_cap(cap);
        let report = bin.enforce_cap("c1").unwrap();

        assert_eq!(report.entries_purged, 1);
        assert_eq!(report.bytes_freed, first.payload_bytes);
        assert!(report.early, "docs/07 requires warning about this case");

        // Oldest first: the newest entry is the one most likely to be wanted.
        assert!(!bin.get(first.id).unwrap().unwrap().is_restorable());
        assert!(bin.get(second.id).unwrap().unwrap().is_restorable());
        assert!(
            bin.get(third.id).unwrap().unwrap().is_restorable(),
            "the newest entry must be the last to go"
        );
        assert!(bin.bytes_for_connection("c1").unwrap() <= cap);
    }

    #[test]
    fn a_connection_under_its_cap_loses_nothing() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock).with_cap(1_000_000);
        let entry = stored(&bin, "c1", 2);

        let report = bin.enforce_cap("c1").unwrap();
        assert!(!report.purged_anything());
        assert!(bin.get(entry.id).unwrap().unwrap().is_restorable());
    }

    #[test]
    fn one_connections_cap_does_not_touch_another_connection() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock).with_cap(200);

        stored(&bin, "c1", 2);
        stored(&bin, "c1", 2);
        let other = stored(&bin, "c2", 2);

        bin.enforce_cap("c1").unwrap();
        assert!(
            bin.get(other.id).unwrap().unwrap().is_restorable(),
            "the cap is per connection"
        );
    }

    // --- decision D: an entry outlives its connection ---

    #[test]
    fn nothing_here_purges_an_entry_because_its_connection_was_deleted() {
        // docs/15: entries detach rather than disappear, and the 30-day
        // window runs its normal course either way. The connection's name is
        // kept so the entry still reads sensibly, and whether the connection
        // still exists is decided at read time.
        let db = crate::database::open_in_memory().unwrap();
        let mut clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);
        let entry = stored(&bin, "deleted-connection", 2);

        clock.advance_days(29);
        let bin = RecoveryBin::new(&db, &clock);
        let still = bin.get(entry.id).unwrap().unwrap();
        assert!(still.is_restorable());
        assert_eq!(still.connection_name, "deleted-connection");
        assert_eq!(still.expires_at, "2026-01-31T00:00:00Z");
    }

    // --- staging, for a capture too large to hold in memory ---

    #[test]
    fn a_staged_capture_becomes_a_real_entry_only_when_finalised() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);

        let mut capture = bin.stage();
        for id in 1..=2_500 {
            bin.stage_record(&mut capture, &record(id)).unwrap();
        }
        assert_eq!(capture.records_staged(), 2_500);
        assert!(
            bin.list(&RecoveryFilter::default()).unwrap().is_empty(),
            "staged records are not recovery data yet"
        );

        let entry = bin
            .finalize(
                capture,
                "c1",
                "c1",
                "delete",
                "Delete every record in users",
            )
            .unwrap();

        assert_eq!(entry.record_count, 2_500);
        assert!(entry.is_restorable());
        assert_eq!(
            bin.staged_record_count().unwrap(),
            0,
            "a capture is never both staged and stored"
        );
    }

    #[test]
    fn a_discarded_capture_leaves_nothing_behind() {
        // The write failed or rolled back, so this describes something that
        // never happened.
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);

        let mut capture = bin.stage();
        for id in 1..=10 {
            bin.stage_record(&mut capture, &record(id)).unwrap();
        }
        bin.discard(capture).unwrap();

        assert_eq!(bin.staged_record_count().unwrap(), 0);
        assert!(bin.list(&RecoveryFilter::default()).unwrap().is_empty());
    }

    #[test]
    fn two_captures_in_flight_do_not_mix() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);

        let mut first = bin.stage();
        let mut second = bin.stage();
        bin.stage_record(&mut first, &record(1)).unwrap();
        bin.stage_record(&mut second, &record(2)).unwrap();
        bin.stage_record(&mut first, &record(3)).unwrap();

        let entry = bin.finalize(first, "c1", "c1", "delete", "Delete").unwrap();
        assert_eq!(entry.record_count, 2, "only its own records");
        assert_eq!(
            bin.staged_record_count().unwrap(),
            1,
            "the other is untouched"
        );

        let other = bin
            .finalize(second, "c1", "c1", "delete", "Delete")
            .unwrap();
        assert_eq!(other.record_count, 1);
    }

    #[test]
    fn sweeping_removes_a_capture_left_behind_by_a_process_that_died() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);

        let mut capture = bin.stage();
        bin.stage_record(&mut capture, &record(1)).unwrap();
        std::mem::forget(capture);

        assert_eq!(bin.sweep_staging().unwrap(), 1);
        assert_eq!(bin.staged_record_count().unwrap(), 0);
    }

    #[test]
    fn a_captured_value_keeps_its_type_through_staging_and_back() {
        let db = crate::database::open_in_memory().unwrap();
        let clock = FixedClock::epoch();
        let bin = RecoveryBin::new(&db, &clock);

        let mut fields = serde_json::Map::new();
        fields.insert("id".to_string(), serde_json::json!(1));
        fields.insert("active".to_string(), serde_json::json!(true));
        fields.insert("note".to_string(), serde_json::Value::Null);
        fields.insert("profile".to_string(), serde_json::json!({"city": "Boston"}));

        let mut capture = bin.stage();
        bin.stage_record(&mut capture, &RecordSnapshot::new(fields))
            .unwrap();
        let entry = bin
            .finalize(capture, "c1", "c1", "delete", "Delete")
            .unwrap();

        let payload = entry.payload.unwrap();
        let restored = &payload.records()[0];
        assert!(restored.get("id").unwrap().is_number());
        assert!(restored.get("active").unwrap().is_boolean());
        assert!(restored.get("note").unwrap().is_null());
        assert!(restored.get("profile").unwrap().is_object());
    }
}
