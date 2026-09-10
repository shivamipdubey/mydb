//! What a record contained, captured before or after a write.
//!
//! This is the shape the audit log and the recovery bin store
//! (docs/07-audit-log-and-recovery-bin.md), and it is deliberately neutral
//! between a row and a document.
//!
//! A SQL row and a MongoDB document are different things, but both are a set
//! of named values, and JSON represents both without distorting either: a row
//! becomes an object of column name to value, and a document already is one,
//! including its nesting. The alternative, a rectangular grid of strings,
//! would flatten a document into a shape it never had. Losing the truth about
//! what was stored is the one thing a recovery store cannot afford, since it
//! is the only record of data that no longer exists.
//!
//! Defined here, before any document engine exists, so the store built on it
//! does not have to be retrofitted when one arrives.

use serde::{Deserialize, Serialize};

/// One record's contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecordSnapshot {
    /// Field name to value. Values keep their own shape: a nested document
    /// stays nested, a number stays a number, a null stays null rather than
    /// becoming an empty string.
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl RecordSnapshot {
    pub fn new(fields: serde_json::Map<String, serde_json::Value>) -> Self {
        Self { fields }
    }

    pub fn get(&self, field: &str) -> Option<&serde_json::Value> {
        self.fields.get(field)
    }

    /// The value a restore would use to identify this record again.
    ///
    /// Returns `None` when the engine gives the record no stable identity,
    /// which is the case for a SQL table with no primary key. A restore must
    /// refuse rather than guess: matching on every field is ambiguous the
    /// moment two records are identical, and guessing which record the user
    /// meant is the mistake the whole confirmation workflow exists to
    /// prevent.
    pub fn identity(&self, key_fields: &[String]) -> Option<Vec<serde_json::Value>> {
        if key_fields.is_empty() {
            return None;
        }
        key_fields
            .iter()
            .map(|field| self.fields.get(field).cloned())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// The records an operation affected, as they stood at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSnapshot {
    records: Vec<RecordSnapshot>,
    /// How many records the operation affected in total. May exceed the
    /// number held here, when this is a sample.
    total: u64,
}

impl StateSnapshot {
    /// A complete capture: every affected record is present.
    pub fn complete(records: Vec<RecordSnapshot>) -> Self {
        let total = records.len() as u64;
        Self { records, total }
    }

    /// A sample of a larger set, with the true total alongside it.
    ///
    /// The total is carried separately and is never derived from the sample,
    /// because a count taken from a sample would understate a large
    /// operation, which is the case where being wrong matters most.
    pub fn sample(records: Vec<RecordSnapshot>, total: u64) -> Self {
        Self { records, total }
    }

    pub fn empty() -> Self {
        Self {
            records: Vec::new(),
            total: 0,
        }
    }

    pub fn records(&self) -> &[RecordSnapshot] {
        &self.records
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// Whether this holds fewer records than the operation affected.
    pub fn is_sample(&self) -> bool {
        (self.records.len() as u64) < self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

#[cfg(test)]
mod tests {
    // Tests may panic; the workspace lints keep panics out of the
    // application, not out of assertions (docs/17-coding-standards.md).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use serde_json::json;

    fn record(id: i64, email: &str) -> RecordSnapshot {
        let mut fields = serde_json::Map::new();
        fields.insert("id".to_string(), json!(id));
        fields.insert("email".to_string(), json!(email));
        RecordSnapshot::new(fields)
    }

    #[test]
    fn a_complete_capture_reports_its_own_size() {
        let state = StateSnapshot::complete(vec![record(1, "a@example.com")]);
        assert_eq!(state.total(), 1);
        assert!(!state.is_sample());
    }

    #[test]
    fn a_sample_keeps_the_true_total_rather_than_the_sample_size() {
        let state = StateSnapshot::sample(vec![record(1, "a@example.com")], 5_000);
        assert_eq!(
            state.total(),
            5_000,
            "the count must not come from the sample"
        );
        assert_eq!(state.records().len(), 1);
        assert!(state.is_sample());
    }

    #[test]
    fn values_keep_their_own_shape_rather_than_becoming_strings() {
        let mut fields = serde_json::Map::new();
        fields.insert("id".to_string(), json!(7));
        fields.insert("active".to_string(), json!(true));
        fields.insert("note".to_string(), serde_json::Value::Null);
        fields.insert("tags".to_string(), json!(["a", "b"]));
        fields.insert("profile".to_string(), json!({"city": "Boston"}));
        let snapshot = RecordSnapshot::new(fields);

        // A recovery store is the only record of data that no longer exists,
        // so it must not blur these into text.
        assert!(snapshot.get("id").unwrap().is_number());
        assert!(snapshot.get("active").unwrap().is_boolean());
        assert!(snapshot.get("note").unwrap().is_null());
        assert!(snapshot.get("tags").unwrap().is_array());
        assert!(
            snapshot.get("profile").unwrap().is_object(),
            "a nested document must survive as a nested document"
        );
    }

    #[test]
    fn identity_comes_from_the_key_fields_the_engine_reports() {
        let snapshot = record(3, "c@example.com");
        assert_eq!(snapshot.identity(&["id".to_string()]), Some(vec![json!(3)]));
    }

    #[test]
    fn a_record_with_no_key_fields_has_no_identity() {
        // A SQL table with no primary key. A restore must refuse rather than
        // match on every field, which is ambiguous the moment two records
        // are identical.
        assert_eq!(record(1, "a@example.com").identity(&[]), None);
    }

    #[test]
    fn a_missing_key_field_yields_no_identity_rather_than_a_partial_one() {
        assert_eq!(
            record(1, "a@example.com").identity(&["uuid".to_string()]),
            None
        );
    }

    #[test]
    fn a_snapshot_round_trips_through_json_unchanged() {
        let state = StateSnapshot::sample(vec![record(1, "a@example.com")], 10);
        let json = serde_json::to_string(&state).unwrap();
        let back: StateSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, state);
    }
}
