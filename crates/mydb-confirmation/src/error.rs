//! Why a command could not be carried through the workflow.

use mydb_adapters::AdapterError;

/// A failure somewhere in the confirmation workflow.
///
/// The stage is part of the type rather than only the message, because the
/// stages differ in what they mean. A failed preview means nothing happened
/// and nothing will. A failed execution means a write was attempted; whether
/// anything changed is the adapter's transaction guarantee to answer, not
/// something to infer from an error string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowError {
    #[error("could not read: {0}")]
    Read(#[source] AdapterError),

    #[error("could not preview the change, so nothing was run: {0}")]
    Preview(#[source] AdapterError),

    #[error("the change was confirmed but could not be completed: {0}")]
    Execution(#[source] AdapterError),
}
