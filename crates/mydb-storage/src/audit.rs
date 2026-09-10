//! The audit log (docs/07-audit-log-and-recovery-bin.md).
//!
//! Every executed write is recorded here: when, against which connection,
//! what operation, the intent that produced it, and what happened. It is
//! append-only, enforced by triggers in the database rather than by the
//! discipline of this module (see `database.rs`).
//!
//! ## Size tiers
//!
//! docs/07 splits entries by size. Below the threshold, the entry holds the
//! full before and after state itself. Above it, the entry holds a row count,
//! a sample of affected records, and a reference to the recovery bin entry
//! that holds the whole thing. Full detail is never in both, so there is
//! always exactly one authoritative copy.
//!
//! ## What is never recorded
//!
//! No credential, passphrase, or recovery phrase, under any circumstance
//! (docs/07). There is no column any of them could occupy.

use mydb_core::{Intent, StateSnapshot};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::database::{query_error, StoreError};
use crate::Outcome;

/// The default row or document count above which an operation is large.
///
/// docs/07 makes this configurable with a default of 1000. The setting itself
/// arrives in T17; until then this is the value used.
pub const DEFAULT_SIZE_THRESHOLD: u64 = 1_000;

/// How many affected records a large operation's entry samples.
///
/// A sample exists so the log is readable on its own; the recovery bin holds
/// the complete set.
pub const LARGE_OPERATION_SAMPLE: usize = 20;

/// Whether an operation was small enough to hold its full state inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeTier {
    Small,
    Large,
}

impl SizeTier {
    pub fn for_count(count: u64, threshold: u64) -> Self {
        if count < threshold {
            SizeTier::Small
        } else {
            SizeTier::Large
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            SizeTier::Small => "small",
            SizeTier::Large => "large",
        }
    }
}

/// One executed write, as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: i64,
    pub connection_id: String,
    /// The connection's name when the write ran. Whether that connection
    /// still exists is a separate question, answered at read time.
    pub connection_name: String,
    pub recorded_at: String,
    pub operation: String,
    pub intent_summary: String,
    pub result: Outcome,
    pub size_tier: SizeTier,
    pub affected_count: u64,
    /// Present for a small operation.
    pub before_state: Option<StateSnapshot>,
    /// Present for a small operation.
    pub after_state: Option<StateSnapshot>,
    /// Present for a large operation: a readable sample of what was affected.
    pub state_sample: Option<StateSnapshot>,
    /// Present for a large operation: where the complete before-state lives.
    pub recovery_entry_id: Option<i64>,
    /// True for entries imported from the phase 1 command history, which had
    /// no way to capture state.
    pub predates_state_capture: bool,
}

/// What to record about a write that has already been attempted.
#[derive(Debug, Clone)]
pub struct AuditRecord<'a> {
    pub connection_id: &'a str,
    pub connection_name: &'a str,
    pub intent: &'a Intent,
    pub result: Outcome,
    pub affected_count: u64,
    /// The records as they stood before the write. Empty for an insert,
    /// which overwrites nothing.
    pub before: StateSnapshot,
    /// The records as they stand after it.
    pub after: StateSnapshot,
    /// Where the complete before-state lives, when this operation was large
    /// enough that the log holds only a sample.
    pub recovery_entry_id: Option<i64>,
}

/// Which entries to read.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub connection_id: Option<String>,
    /// Inclusive lower bound, RFC 3339.
    pub since: Option<String>,
    /// Inclusive upper bound, RFC 3339.
    pub until: Option<String>,
    pub operation: Option<String>,
    pub limit: Option<u32>,
}

/// The append-only audit log.
pub struct AuditLog<'a> {
    connection: &'a Connection,
    threshold: u64,
}

impl<'a> AuditLog<'a> {
    pub fn new(connection: &'a Connection) -> Self {
        Self {
            connection,
            threshold: DEFAULT_SIZE_THRESHOLD,
        }
    }

    /// Overrides the size threshold. The setting behind this arrives in T17.
    pub fn with_threshold(mut self, threshold: u64) -> Self {
        self.threshold = threshold;
        self
    }

    /// Appends an entry for a write that has already been attempted.
    ///
    /// Called after the operation finishes, never before
    /// (docs/16-security-and-cybersafety-checklist.md item 7). A write that
    /// the database refused is recorded as a failure; it was attempted, and
    /// an attempt is a fact worth keeping.
    pub fn append(&self, record: AuditRecord<'_>) -> Result<AuditEntry, StoreError> {
        let tier = SizeTier::for_count(record.affected_count, self.threshold);

        // Full detail lives in exactly one place. For a small operation that
        // is the entry itself; for a large one it is the recovery bin, and
        // the entry keeps a sample and a reference (docs/07).
        let (before_state, after_state, state_sample) = match tier {
            SizeTier::Small => (
                Some(record.before.clone()),
                Some(record.after.clone()),
                None,
            ),
            SizeTier::Large => {
                let sample = record
                    .before
                    .records()
                    .iter()
                    .take(LARGE_OPERATION_SAMPLE)
                    .cloned()
                    .collect();
                (
                    None,
                    None,
                    Some(StateSnapshot::sample(sample, record.affected_count)),
                )
            }
        };

        let encode = |state: &Option<StateSnapshot>| -> Result<Option<String>, StoreError> {
            match state {
                None => Ok(None),
                Some(state) => {
                    serde_json::to_string(state)
                        .map(Some)
                        .map_err(|error| StoreError::Query {
                            detail: format!("could not encode captured state: {error}"),
                        })
                }
            }
        };

        let recorded_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let operation = record.intent.operation.verb().to_string();
        let intent_summary = record.intent.describe();

        self.connection
            .execute(
                "INSERT INTO audit_log (
                     connection_id, connection_name, recorded_at, operation,
                     intent_summary, result, size_tier, affected_count,
                     before_state, after_state, state_sample, recovery_entry_id,
                     predates_state_capture
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0)",
                params![
                    record.connection_id,
                    record.connection_name,
                    recorded_at,
                    operation,
                    intent_summary,
                    result_as_str(record.result),
                    tier.as_str(),
                    record.affected_count as i64,
                    encode(&before_state)?,
                    encode(&after_state)?,
                    encode(&state_sample)?,
                    record.recovery_entry_id,
                ],
            )
            .map_err(query_error)?;

        let id = self.connection.last_insert_rowid();
        self.get(id)?
            .ok_or(StoreError::NotFound { kind: "audit", id })
    }

    /// Appends an entry imported from the phase 1 command history.
    ///
    /// Those entries could never hold before or after state, so they are
    /// flagged rather than dropped: the history stays continuous, and nobody
    /// mistakes an old entry's absent state for a capture that failed.
    pub fn import_legacy(
        &self,
        connection_name: &str,
        recorded_at: &str,
        operation: &str,
        intent_summary: &str,
        result: Outcome,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                "INSERT INTO audit_log (
                     connection_id, connection_name, recorded_at, operation,
                     intent_summary, result, size_tier, affected_count,
                     predates_state_capture
                 ) VALUES ('', ?1, ?2, ?3, ?4, ?5, 'small', 0, 1)",
                params![
                    connection_name,
                    recorded_at,
                    operation,
                    intent_summary,
                    result_as_str(result)
                ],
            )
            .map_err(query_error)?;
        Ok(())
    }

    pub fn get(&self, id: i64) -> Result<Option<AuditEntry>, StoreError> {
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
    pub fn list(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, StoreError> {
        // Conditions are added as bound parameters; nothing from the filter
        // is formatted into the statement text.
        let mut sql = String::from(SELECT_COLUMNS);
        let mut clauses: Vec<&str> = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(id) = &filter.connection_id {
            clauses.push("connection_id = ?");
            values.push(Box::new(id.clone()));
        }
        if let Some(since) = &filter.since {
            clauses.push("recorded_at >= ?");
            values.push(Box::new(since.clone()));
        }
        if let Some(until) = &filter.until {
            clauses.push("recorded_at <= ?");
            values.push(Box::new(until.clone()));
        }
        if let Some(operation) = &filter.operation {
            clauses.push("operation = ?");
            values.push(Box::new(operation.clone()));
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

    pub fn count(&self) -> Result<u64, StoreError> {
        let count: i64 = self
            .connection
            .query_row("SELECT count(*) FROM audit_log", [], |row| row.get(0))
            .map_err(query_error)?;
        Ok(count.max(0) as u64)
    }
}

const SELECT_COLUMNS: &str = "SELECT id, connection_id, connection_name, recorded_at, operation, \
     intent_summary, result, size_tier, affected_count, before_state, after_state, \
     state_sample, recovery_entry_id, predates_state_capture FROM audit_log";

fn result_as_str(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Success => "success",
        Outcome::Failure => "failure",
    }
}

type RowResult = Result<AuditEntry, StoreError>;

fn read_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RowResult> {
    let decode = |text: Option<String>| -> Result<Option<StateSnapshot>, StoreError> {
        match text {
            None => Ok(None),
            Some(text) => {
                serde_json::from_str(&text)
                    .map(Some)
                    .map_err(|error| StoreError::Query {
                        detail: format!("could not decode captured state: {error}"),
                    })
            }
        }
    };

    let build = || -> RowResult {
        Ok(AuditEntry {
            id: row.get(0).map_err(query_error)?,
            connection_id: row.get(1).map_err(query_error)?,
            connection_name: row.get(2).map_err(query_error)?,
            recorded_at: row.get(3).map_err(query_error)?,
            operation: row.get(4).map_err(query_error)?,
            intent_summary: row.get(5).map_err(query_error)?,
            result: match row.get::<_, String>(6).map_err(query_error)?.as_str() {
                "failure" => Outcome::Failure,
                _ => Outcome::Success,
            },
            size_tier: match row.get::<_, String>(7).map_err(query_error)?.as_str() {
                "large" => SizeTier::Large,
                _ => SizeTier::Small,
            },
            affected_count: row.get::<_, i64>(8).map_err(query_error)?.max(0) as u64,
            before_state: decode(row.get(9).map_err(query_error)?)?,
            after_state: decode(row.get(10).map_err(query_error)?)?,
            state_sample: decode(row.get(11).map_err(query_error)?)?,
            recovery_entry_id: row.get(12).map_err(query_error)?,
            predates_state_capture: row.get::<_, i64>(13).map_err(query_error)? != 0,
        })
    };

    Ok(build())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use mydb_core::{Engine, Filter, Operation, RecordSnapshot};

    fn intent(operation: Operation) -> Intent {
        Intent {
            engine: Engine::Postgres,
            namespace: "public".to_string(),
            table: "users".to_string(),
            operation,
            filter: Filter::everything(),
            assignments: Vec::new(),
        }
    }

    fn record(id: i64) -> RecordSnapshot {
        let mut fields = serde_json::Map::new();
        fields.insert("id".to_string(), serde_json::json!(id));
        fields.insert(
            "email".to_string(),
            serde_json::json!(format!("user{id}@example.com")),
        );
        RecordSnapshot::new(fields)
    }

    fn database() -> Connection {
        crate::database::open_in_memory().unwrap()
    }

    fn append(
        log: &AuditLog<'_>,
        operation: Operation,
        count: u64,
        before: StateSnapshot,
    ) -> AuditEntry {
        log.append(AuditRecord {
            connection_id: "c1",
            connection_name: "Local test database",
            intent: &intent(operation),
            result: Outcome::Success,
            affected_count: count,
            before,
            after: StateSnapshot::empty(),
            recovery_entry_id: None,
        })
        .unwrap()
    }

    #[test]
    fn a_small_operation_holds_its_full_state_in_the_entry() {
        let db = database();
        let log = AuditLog::new(&db);

        let before = StateSnapshot::complete(vec![record(1), record(2)]);
        let entry = append(&log, Operation::Delete, 2, before.clone());

        assert_eq!(entry.size_tier, SizeTier::Small);
        assert_eq!(entry.before_state, Some(before));
        assert!(entry.after_state.is_some());
        assert!(
            entry.state_sample.is_none(),
            "a small entry needs no sample; it has the whole thing"
        );
        assert!(entry.recovery_entry_id.is_none());
    }

    #[test]
    fn a_large_operation_holds_a_sample_and_a_reference_but_not_full_detail() {
        // docs/07: do not duplicate full detail in both the log and the bin.
        let db = database();
        let log = AuditLog::new(&db).with_threshold(10);

        let records: Vec<RecordSnapshot> = (1..=50).map(record).collect();
        let entry = log
            .append(AuditRecord {
                connection_id: "c1",
                connection_name: "Local test database",
                intent: &intent(Operation::Delete),
                result: Outcome::Success,
                affected_count: 50,
                before: StateSnapshot::complete(records),
                after: StateSnapshot::empty(),
                recovery_entry_id: Some(77),
            })
            .unwrap();

        assert_eq!(entry.size_tier, SizeTier::Large);
        assert!(
            entry.before_state.is_none() && entry.after_state.is_none(),
            "full detail belongs in the recovery bin, not here as well"
        );
        assert_eq!(entry.recovery_entry_id, Some(77));

        let sample = entry.state_sample.unwrap();
        assert_eq!(sample.records().len(), LARGE_OPERATION_SAMPLE);
        assert_eq!(
            sample.total(),
            50,
            "the total must be the real count, not the sample's length"
        );
        assert!(sample.is_sample());
    }

    #[test]
    fn the_tier_boundary_is_the_threshold_itself() {
        assert_eq!(SizeTier::for_count(999, 1000), SizeTier::Small);
        assert_eq!(SizeTier::for_count(1000, 1000), SizeTier::Large);
        assert_eq!(SizeTier::for_count(0, 1000), SizeTier::Small);
    }

    // --- docs/07: append-only, enforced by the database ---

    #[test]
    fn the_database_itself_refuses_to_edit_an_entry() {
        let db = database();
        let log = AuditLog::new(&db);
        let entry = append(
            &log,
            Operation::Delete,
            1,
            StateSnapshot::complete(vec![record(1)]),
        );

        let edited = db.execute(
            "UPDATE audit_log SET intent_summary = 'something else' WHERE id = ?1",
            params![entry.id],
        );
        assert!(
            edited.is_err(),
            "a trigger must refuse this, so no future code path can succeed at it"
        );
        assert!(format!("{:?}", edited.unwrap_err()).contains("append-only"));

        // And the entry is unchanged.
        assert_eq!(log.get(entry.id).unwrap().unwrap(), entry);
    }

    #[test]
    fn the_database_itself_refuses_to_delete_an_entry() {
        let db = database();
        let log = AuditLog::new(&db);
        let entry = append(&log, Operation::Delete, 1, StateSnapshot::empty());

        let deleted = db.execute("DELETE FROM audit_log WHERE id = ?1", params![entry.id]);
        assert!(
            deleted.is_err(),
            "docs/07: entries are never deleted, including during a purge"
        );
        assert_eq!(log.count().unwrap(), 1);
    }

    #[test]
    fn a_failed_write_is_recorded_as_a_failure() {
        let db = database();
        let log = AuditLog::new(&db);
        let entry = log
            .append(AuditRecord {
                connection_id: "c1",
                connection_name: "Local test database",
                intent: &intent(Operation::Delete),
                result: Outcome::Failure,
                affected_count: 0,
                before: StateSnapshot::empty(),
                after: StateSnapshot::empty(),
                recovery_entry_id: None,
            })
            .unwrap();
        assert_eq!(entry.result, Outcome::Failure);
    }

    #[test]
    fn every_operation_type_records_its_own_name() {
        let db = database();
        let log = AuditLog::new(&db);
        for (operation, expected) in [
            (Operation::Delete, "delete"),
            (Operation::Insert, "insert"),
            (Operation::Update, "update"),
            (Operation::DropTable, "drop_table"),
            (Operation::Truncate, "truncate"),
        ] {
            let entry = append(&log, operation, 1, StateSnapshot::empty());
            assert_eq!(entry.operation, expected);
        }
    }

    // --- reading ---

    #[test]
    fn entries_read_back_newest_first_and_filter_by_connection() {
        let db = database();
        let log = AuditLog::new(&db);
        for id in ["c1", "c2", "c1"] {
            log.append(AuditRecord {
                connection_id: id,
                connection_name: id,
                intent: &intent(Operation::Delete),
                result: Outcome::Success,
                affected_count: 1,
                before: StateSnapshot::empty(),
                after: StateSnapshot::empty(),
                recovery_entry_id: None,
            })
            .unwrap();
        }

        let all = log.list(&AuditFilter::default()).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all[0].id > all[2].id, "newest first");

        let filtered = log
            .list(&AuditFilter {
                connection_id: Some("c1".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|entry| entry.connection_id == "c1"));
    }

    #[test]
    fn filtering_by_operation_and_limit_works() {
        let db = database();
        let log = AuditLog::new(&db);
        append(&log, Operation::Delete, 1, StateSnapshot::empty());
        append(&log, Operation::Insert, 1, StateSnapshot::empty());

        let deletes = log
            .list(&AuditFilter {
                operation: Some("delete".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(deletes.len(), 1);

        let limited = log
            .list(&AuditFilter {
                limit: Some(1),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn a_filter_value_cannot_become_part_of_the_statement() {
        let db = database();
        let log = AuditLog::new(&db);
        append(&log, Operation::Delete, 1, StateSnapshot::empty());

        // Every filter value is bound, so this matches nothing rather than
        // altering the query.
        let entries = log
            .list(&AuditFilter {
                connection_id: Some("c1' OR '1'='1".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert!(entries.is_empty());
        assert_eq!(log.count().unwrap(), 1, "the log is intact");
    }

    // --- decision G: phase 1 history imported, flagged ---

    #[test]
    fn imported_history_is_flagged_as_predating_state_capture() {
        let db = database();
        let log = AuditLog::new(&db);
        log.import_legacy(
            "Local test database",
            "2026-09-10T07:27:22Z",
            "delete",
            "Delete records in users where active is false",
            Outcome::Success,
        )
        .unwrap();

        let entry = &log.list(&AuditFilter::default()).unwrap()[0];
        assert!(entry.predates_state_capture);
        assert!(
            entry.before_state.is_none() && entry.after_state.is_none(),
            "the phase 1 history never held state; the flag says so rather \
             than the absence implying a failed capture"
        );
        assert_eq!(entry.recorded_at, "2026-09-10T07:27:22Z");
    }

    #[test]
    fn a_newly_appended_entry_is_not_flagged_as_legacy() {
        let db = database();
        let log = AuditLog::new(&db);
        let entry = append(&log, Operation::Delete, 1, StateSnapshot::empty());
        assert!(!entry.predates_state_capture);
    }

    // --- docs/07: nothing sensitive has anywhere to live ---

    #[test]
    fn the_schema_has_no_column_a_credential_could_occupy() {
        let db = database();
        let mut statement = db
            .prepare("SELECT name FROM pragma_table_info('audit_log')")
            .unwrap();
        let columns: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|c| c.unwrap())
            .collect();

        for forbidden in [
            "password",
            "secret",
            "credential",
            "passphrase",
            "recovery_phrase",
        ] {
            assert!(
                !columns.iter().any(|c| c.contains(forbidden)),
                "{forbidden} must have nowhere to live in the audit log"
            );
        }
        assert!(columns.contains(&"intent_summary".to_string()));
    }
}
