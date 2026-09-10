//! The preview, and the token that makes execution depend on it.
//!
//! docs/05-confirmation-workflow.md requires that no write ever reach a
//! database without the user first seeing what it will affect. Enforcing that
//! by convention would mean every future adapter, on every future engine, has
//! to remember. Instead it is enforced by the type system:
//!
//! 1. [`Preview`] has private fields and a crate-private constructor, so the
//!    only way to obtain one is for an adapter to have actually run a preview
//!    against the database.
//! 2. [`Adapter::execute`](crate::Adapter::execute) accepts an
//!    [`ApprovedWrite`], which can only be produced by consuming a `Preview`.
//! 3. The intent that executes is read back out of the preview, not passed in
//!    alongside it, so the statement that runs is necessarily the one the user
//!    was shown.
//!
//! The result is that "execute without a preview" is not a bug to be caught in
//! review. It does not compile.

use mydb_core::{Column, Intent, StateSnapshot};
use serde::{Deserialize, Serialize};

use crate::records::{Record, RecordSet};

/// A table as it stands, for an operation that acts on the table itself
/// rather than on a set of records.
///
/// docs/04-database-adapters.md requires a schema change to preview the
/// current schema and row count of the affected table. Listing matching
/// records would be the wrong answer twice over: a DROP TABLE removes the
/// structure as well as the data, and neither operation has a filter to match
/// records against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableOutline {
    columns: Vec<Column>,
    row_count: u64,
    statement: String,
}

impl TableOutline {
    pub(crate) fn new(columns: Vec<Column>, row_count: u64, statement: String) -> Self {
        Self {
            columns,
            row_count,
            statement,
        }
    }

    /// Builds an outline without a database, for tests only.
    #[cfg(feature = "test-support")]
    pub fn for_testing(columns: Vec<Column>, row_count: u64, statement: String) -> Self {
        Self::new(columns, row_count, statement)
    }

    /// The table's current shape, which a DROP TABLE would also remove.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// How many records the table holds right now.
    pub fn row_count(&self) -> u64 {
        self.row_count
    }

    pub fn statement(&self) -> &str {
        &self.statement
    }
}

/// What a preview is showing.
///
/// An enum rather than one shape with unused fields, because the two are
/// genuinely different questions: "which records will this touch" and "what
/// is in this table that is about to be destroyed".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "detail")]
pub enum PreviewBody {
    /// The records a write will create, change, or remove.
    Records(RecordSet),
    /// The table a schema operation will empty or destroy.
    Table(TableOutline),
}

/// What a write would do, shown to the user before anything happens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    intent: Intent,
    body: PreviewBody,
}

impl Preview {
    /// Builds a preview of records. Crate-private on purpose: see the module
    /// comment.
    pub(crate) fn new(intent: Intent, affected: RecordSet) -> Self {
        Self {
            intent,
            body: PreviewBody::Records(affected),
        }
    }

    /// Builds a preview of a whole table, for a schema operation.
    pub(crate) fn of_table(intent: Intent, outline: TableOutline) -> Self {
        Self {
            intent,
            body: PreviewBody::Table(outline),
        }
    }

    /// Builds a preview without touching a database, for tests only.
    ///
    /// This does not weaken the guarantee above. It is behind the
    /// `test-support` feature, which the application crate never enables, so
    /// no shipped build can reach it. It exists because
    /// docs/18-testing-strategy.md requires the confirmation state machine to
    /// have unit tests, and a state machine that can only be tested against a
    /// live database would be tested less.
    #[cfg(feature = "test-support")]
    pub fn for_testing(intent: Intent, affected: RecordSet) -> Self {
        Self::new(intent, affected)
    }

    /// Builds a table preview without a database, for tests only.
    #[cfg(feature = "test-support")]
    pub fn of_table_for_testing(intent: Intent, outline: TableOutline) -> Self {
        Self::of_table(intent, outline)
    }

    pub fn intent(&self) -> &Intent {
        &self.intent
    }

    /// What this preview is showing. Match on this when the two cases need
    /// to be told apart, which the interface does.
    pub fn body(&self) -> &PreviewBody {
        &self.body
    }

    /// The records this write would affect, when it affects records.
    ///
    /// `None` for a schema operation, which acts on the table itself.
    pub fn records(&self) -> Option<&RecordSet> {
        match &self.body {
            PreviewBody::Records(records) => Some(records),
            PreviewBody::Table(_) => None,
        }
    }

    /// The columns of the affected records, or an empty slice for a schema
    /// operation. Use [`Preview::body`] when the difference matters.
    pub fn columns(&self) -> &[String] {
        self.records().map(RecordSet::columns).unwrap_or(&[])
    }

    /// The sample of affected records, or an empty slice for a schema
    /// operation.
    pub fn rows(&self) -> &[Record] {
        self.records().map(RecordSet::records).unwrap_or(&[])
    }

    /// Exactly how many records this will destroy or change. Never an
    /// estimate, and never just the length of a sample.
    ///
    /// For a schema operation this is the table's current row count, which is
    /// what a DROP TABLE or TRUNCATE will remove.
    pub fn affected_count(&self) -> u64 {
        match &self.body {
            PreviewBody::Records(records) => records.total_count(),
            PreviewBody::Table(outline) => outline.row_count(),
        }
    }

    /// Whether more records are affected than are listed.
    pub fn is_truncated(&self) -> bool {
        self.records().is_some_and(RecordSet::is_truncated)
    }

    /// The read that produced this preview, for the expandable raw-syntax
    /// detail docs/12-ui-ux-guidelines.md allows as secondary information.
    pub fn statement(&self) -> &str {
        match &self.body {
            PreviewBody::Records(records) => records.statement(),
            PreviewBody::Table(outline) => outline.statement(),
        }
    }

    /// Whether this write would affect nothing at all.
    ///
    /// Worth surfacing distinctly: a user who expected to delete something and
    /// is shown zero rows has almost certainly written the wrong filter.
    pub fn affects_nothing(&self) -> bool {
        self.affected_count() == 0
    }

    /// Marks this preview as confirmed by the user, producing the token
    /// `execute` requires.
    ///
    /// Consuming the preview means one approval cannot be replayed into two
    /// executions. The confirmation engine (docs/05) is the only thing that
    /// should call this, and only once the user has actually confirmed; that
    /// sequencing is its job, while this type's job is making sure no
    /// execution can happen without having reached this point at all.
    pub fn approve(self) -> ApprovedWrite {
        ApprovedWrite { preview: self }
    }
}

/// A preview the user has confirmed. The only thing an adapter will execute.
#[derive(Debug, Clone)]
pub struct ApprovedWrite {
    preview: Preview,
}

impl ApprovedWrite {
    /// The intent to run: read back from the preview, so it is necessarily
    /// the one the user was shown.
    pub fn intent(&self) -> &Intent {
        self.preview.intent()
    }

    pub fn preview(&self) -> &Preview {
        &self.preview
    }
}

/// How many affected records a write captures for the audit log.
///
/// Matches docs/07-audit-log-and-recovery-bin.md's default size threshold: an
/// operation smaller than this has its full state recorded, and a larger one
/// needs only a sample here because the recovery bin holds the whole thing.
/// T17 makes the threshold configurable and this follows it.
pub const STATE_CAPTURE_LIMIT: usize = 1_000;

/// What happened when a confirmed write ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    pub rows_affected: u64,
    /// The affected records as they stood before the write, captured inside
    /// the same transaction so nothing can change between reading them and
    /// writing over them.
    pub before: StateSnapshot,
    /// The same records afterwards.
    ///
    /// `None` means the state could not be captured, not that it was empty.
    /// An update needs to match records back to the ones it changed, which
    /// requires a single-column primary key; a table without one, or with a
    /// composite key, cannot be re-read reliably and says so rather than
    /// reporting an empty result that would read as "the records vanished".
    pub after: Option<StateSnapshot>,
}

impl ExecutionOutcome {
    /// A write whose records are gone afterwards, such as a delete.
    pub fn removing(rows_affected: u64, before: StateSnapshot) -> Self {
        Self {
            rows_affected,
            before,
            after: Some(StateSnapshot::empty()),
        }
    }
}
