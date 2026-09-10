//! The confirmation engine: the mandatory preview then confirm/edit/cancel
//! sequence from docs/05-confirmation-workflow.md.
//!
//! This is the single enforcement point for the rule the whole product exists
//! to guarantee: no write reaches an adapter's execute without a successful
//! preview for that same intent having run first.
//!
//! The sequence is expressed as types rather than as a status field, because
//! a status field can be ignored. Reaching execution requires holding a
//! [`PendingWrite`], which [`begin`] only returns after a preview succeeded,
//! and which is consumed by whichever of confirm, edit, or cancel is chosen.
//! There is no way to write code that skips a step, and no state to forget to
//! check.

mod error;

pub use error::WorkflowError;

use mydb_adapters::{Adapter, ExecutionOutcome, Preview, RecordSet};
use mydb_core::Intent;

/// Where a command lands after being submitted.
#[derive(Debug)]
pub enum Step {
    /// A read. docs/05 step 4: it ran directly, and here is the result. Reads
    /// do not need confirmation because they change nothing.
    ReadComplete(RecordSet),

    /// A write. docs/05 step 5: here is what it would affect, and nothing has
    /// happened yet.
    AwaitingConfirmation(Box<PendingWrite>),
}

impl Step {
    pub fn awaiting_confirmation(&self) -> Option<&PendingWrite> {
        match self {
            Step::AwaitingConfirmation(pending) => Some(pending),
            Step::ReadComplete(_) => None,
        }
    }
}

/// What a cancelled command leaves behind.
///
/// docs/05 step 9: on cancel the intent is discarded and nothing is logged
/// beyond an optional note that a command was cancelled. Deliberately carries
/// no records and no filter, so a cancelled command cannot leak into a log
/// through this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancelled {
    /// A short description of what was abandoned, for an optional note.
    pub summary: String,
}

/// A write that has been previewed and is waiting on the user.
///
/// Holding one of these means, and can only mean, that a preview succeeded.
#[derive(Debug)]
pub struct PendingWrite {
    preview: Preview,
}

/// A write that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completed {
    pub outcome: ExecutionOutcome,
    /// What the user was shown before confirming, described in plain language.
    pub description: String,
}

/// Submits a command.
///
/// docs/05 steps 4 and 5. A read runs and returns its result. A write is
/// previewed and returns something the user must act on. A failed preview
/// returns an error and, critically, no [`PendingWrite`], so there is nothing
/// to confirm: docs/17-coding-standards.md's rule that a failed preview must
/// stop the workflow before confirm is not a check to perform but a value
/// that does not exist.
pub async fn begin(adapter: &dyn Adapter, intent: Intent) -> Result<Step, WorkflowError> {
    if intent.is_write() {
        let preview = adapter
            .build_preview(&intent)
            .await
            .map_err(WorkflowError::Preview)?;
        Ok(Step::AwaitingConfirmation(Box::new(PendingWrite {
            preview,
        })))
    } else {
        let records = adapter
            .run_read(&intent)
            .await
            .map_err(WorkflowError::Read)?;
        Ok(Step::ReadComplete(records))
    }
}

impl PendingWrite {
    /// What the user is being asked to confirm.
    pub fn preview(&self) -> &Preview {
        &self.preview
    }

    pub fn intent(&self) -> &Intent {
        self.preview.intent()
    }

    /// The pending write in plain language (docs/12-ui-ux-guidelines.md).
    pub fn description(&self) -> String {
        self.preview.intent().describe()
    }

    /// Runs the write. docs/05 step 10.
    ///
    /// Consumes the pending write, so one confirmation cannot be replayed
    /// into two executions.
    pub async fn confirm(self, adapter: &dyn Adapter) -> Result<Completed, WorkflowError> {
        let description = self.description();
        let outcome = adapter
            .execute(self.preview.approve())
            .await
            .map_err(WorkflowError::Execution)?;
        Ok(Completed {
            outcome,
            description,
        })
    }

    /// Revises the command and returns to the preview step. docs/05 step 8.
    ///
    /// Explicitly not a path to execution: this returns a [`Step`], which for
    /// a write is a fresh [`PendingWrite`] built from a fresh preview of the
    /// new intent. An edited command is a new command, and gets looked at
    /// again before anything happens.
    pub async fn edit(self, adapter: &dyn Adapter, revised: Intent) -> Result<Step, WorkflowError> {
        // The previous pending write is dropped here. Its approval, had it
        // been given, cannot survive into the revised command.
        begin(adapter, revised).await
    }

    /// Discards the command. docs/05 step 9.
    pub fn cancel(self) -> Cancelled {
        Cancelled {
            summary: self.description(),
        }
    }
}
