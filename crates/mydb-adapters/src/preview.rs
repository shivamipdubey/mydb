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

use mydb_core::Intent;
use serde::{Deserialize, Serialize};

/// How many matching records a preview lists before it starts summarising.
///
/// docs/12-ui-ux-guidelines.md requires affected records to be listed or
/// clearly counted, never merely implied. Above this many, the full count is
/// still exact; only the sample shown is capped, so a user deleting fifty
/// thousand rows sees that number rather than a scrolling wall.
pub const PREVIEW_SAMPLE_LIMIT: usize = 200;

/// One record a write would affect, rendered for display.
///
/// Values are strings because this exists to be shown to a person. Cells that
/// are SQL NULL are `None`, so the UI can distinguish an empty string from an
/// absent value rather than printing both as blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewRow {
    pub cells: Vec<Option<String>>,
}

/// What a write would do, shown to the user before anything happens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    intent: Intent,
    columns: Vec<String>,
    rows: Vec<PreviewRow>,
    affected_count: u64,
    statement: String,
    truncated: bool,
}

impl Preview {
    /// Builds a preview. Crate-private on purpose: see the module comment.
    pub(crate) fn new(
        intent: Intent,
        columns: Vec<String>,
        rows: Vec<PreviewRow>,
        affected_count: u64,
        statement: String,
    ) -> Self {
        let truncated = (rows.len() as u64) < affected_count;
        Self {
            intent,
            columns,
            rows,
            affected_count,
            statement,
            truncated,
        }
    }

    pub fn intent(&self) -> &Intent {
        &self.intent
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The sample of affected records, capped at [`PREVIEW_SAMPLE_LIMIT`].
    pub fn rows(&self) -> &[PreviewRow] {
        &self.rows
    }

    /// Exactly how many records the write will affect. Never an estimate, and
    /// never just the length of the sample above.
    pub fn affected_count(&self) -> u64 {
        self.affected_count
    }

    /// Whether more records are affected than are listed.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// The read that produced this preview, for the expandable raw-syntax
    /// detail docs/12-ui-ux-guidelines.md allows as secondary information.
    pub fn statement(&self) -> &str {
        &self.statement
    }

    /// Whether this write would affect nothing at all.
    ///
    /// Worth surfacing distinctly: a user who expected to delete something and
    /// is shown zero rows has almost certainly written the wrong filter.
    pub fn affects_nothing(&self) -> bool {
        self.affected_count == 0
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

/// What happened when a confirmed write ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    pub rows_affected: u64,
}
