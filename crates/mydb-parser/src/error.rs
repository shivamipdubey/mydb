//! Why a command could not be parsed.
//!
//! Each variant carries what the user needs in order to fix the command
//! themselves. docs/10-nlp-voice-and-local-model.md's clarifying-question tier
//! is phase 5 work; until then these messages are how the user is asked.

/// A command the parser declined to interpret.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("no command entered")]
    Empty,

    #[error("could not tell what to do from: {input}")]
    UnknownOperation { input: String },

    #[error("{operation} commands are not supported yet")]
    NotYetSupported { operation: String },

    #[error("could not tell which table this refers to")]
    NoTarget,

    #[error("no table called \"{name}\". This connection has: {}", known.join(", "))]
    UnknownTable { name: String, known: Vec<String> },

    #[error("\"{name}\" matches more than one table; say which one, as schema.table")]
    AmbiguousTable { name: String },

    #[error("{table} has no column matching \"{column}\". It has: {}", known.join(", "))]
    UnknownColumn {
        column: String,
        table: String,
        known: Vec<String>,
    },

    #[error("could not understand the condition \"{clause}\"")]
    UnparseableCondition { clause: String },

    #[error("could not understand \"{clause}\" as a column and a value to give it")]
    UnparseableAssignment { clause: String },

    #[error("this {operation} names no values. For example: {hint}")]
    MissingValues { operation: String, hint: String },

    #[error("an insert creates a new record, so it cannot have a condition selecting records")]
    FilterOnInsert,

    #[error("\"{value}\" is not a valid {expected} for the column {column}")]
    UnparseableValue {
        value: String,
        column: String,
        expected: String,
    },
}
