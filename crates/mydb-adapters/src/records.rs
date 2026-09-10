//! Records read out of a database, rendered for display.

use serde::{Deserialize, Serialize};

/// How many records a read or preview lists before it starts summarising.
///
/// docs/12-ui-ux-guidelines.md requires affected records to be listed or
/// clearly counted, never merely implied. Above this many, the total is still
/// exact; only the sample shown is capped, so a user deleting fifty thousand
/// rows sees that number rather than a scrolling wall.
pub const SAMPLE_LIMIT: usize = 200;

/// One record, as text.
///
/// Values are strings because this exists to be shown to a person. A cell
/// that is SQL NULL is `None`, so the interface can tell an absent value from
/// an empty one rather than printing both as blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub cells: Vec<Option<String>>,
}

/// A set of records, with an exact count that does not depend on how many
/// were listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordSet {
    columns: Vec<String>,
    records: Vec<Record>,
    total_count: u64,
    truncated: bool,
    statement: String,
}

impl RecordSet {
    pub(crate) fn new(
        columns: Vec<String>,
        records: Vec<Record>,
        total_count: u64,
        statement: String,
    ) -> Self {
        let truncated = (records.len() as u64) < total_count;
        Self {
            columns,
            records,
            total_count,
            truncated,
            statement,
        }
    }

    /// Builds a record set without a database, for tests only. Behind the
    /// `test-support` feature, which the application never enables.
    #[cfg(feature = "test-support")]
    pub fn for_testing(
        columns: Vec<String>,
        records: Vec<Record>,
        total_count: u64,
        statement: String,
    ) -> Self {
        Self::new(columns, records, total_count, statement)
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The sample of records, capped at [`SAMPLE_LIMIT`].
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Exactly how many records there are. Counted independently, never just
    /// the length of the sample above: counting the sample would understate a
    /// large operation, which is the case where being wrong matters most.
    pub fn total_count(&self) -> u64 {
        self.total_count
    }

    /// Whether there are more records than are listed.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// The statement that produced this, for the expandable raw-syntax detail
    /// docs/12-ui-ux-guidelines.md allows as secondary information.
    pub fn statement(&self) -> &str {
        &self.statement
    }

    pub fn is_empty(&self) -> bool {
        self.total_count == 0
    }
}
