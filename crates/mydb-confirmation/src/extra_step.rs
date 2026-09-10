//! The extra confirmation step a production-flagged connection requires
//! (docs/11-production-safety-flag.md).
//!
//! The step is chosen to match what is actually at risk, not to be uniform.
//! A record count is a reasonable thing to make someone retype when records
//! are what will be lost, and a poor one otherwise:
//!
//! - A schema change destroys the table's structure as well as its records,
//!   which no record count conveys, and the count is nothing at all when the
//!   table is empty. The table's name is meaningful either way, so that is
//!   what gets typed.
//! - For a delete or an update affecting no records or one, typing "0" or
//!   "1" is not friction, it is a keystroke. Those require the word instead.

use mydb_core::Intent;

/// The word a user types when a count would not be meaningful.
pub const CONFIRM_WORD: &str = "CONFIRM";

/// What the user must type before the confirm action becomes available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraStep {
    /// Nothing extra. The connection is not flagged production, or the
    /// operation destroys nothing.
    None,

    /// Type the table's name. Used for a schema change, where the structure
    /// is at stake and a record count would describe only part of the loss.
    TableName { table: String },

    /// Type the affected record count, or the confirm word.
    CountOrConfirm { count: u64 },

    /// Type the confirm word. Used where a count exists but is too small to
    /// be real friction.
    ConfirmWord,
}

impl ExtraStep {
    /// Works out which step this write needs.
    ///
    /// Takes the count from the preview the user is actually looking at, so
    /// the number they are asked to type is the number they were shown.
    pub fn required_for(intent: &Intent, production: bool, affected_count: u64) -> Self {
        // The step exists to add friction to destructive commands on
        // higher-stakes connections. An insert destroys nothing, and an
        // unflagged connection asks for no extra deliberation.
        if !production || !intent.is_destructive() {
            return ExtraStep::None;
        }

        if intent.operation.is_schema_change() {
            return ExtraStep::TableName {
                table: intent.qualified_table(),
            };
        }

        match affected_count {
            0 | 1 => ExtraStep::ConfirmWord,
            count => ExtraStep::CountOrConfirm { count },
        }
    }

    /// Whether what the user typed satisfies this step.
    ///
    /// Comparison ignores surrounding space and letter case. The friction is
    /// in having to type the right thing at all; making someone match the
    /// capitalisation of a table name on top of that would be arbitrary, and
    /// MYDB matches text case-insensitively everywhere else.
    pub fn accepts(&self, typed: &str) -> bool {
        let typed = typed.trim();
        match self {
            ExtraStep::None => true,
            ExtraStep::TableName { table } => {
                !typed.is_empty() && typed.eq_ignore_ascii_case(table)
            }
            ExtraStep::CountOrConfirm { count } => {
                typed == count.to_string() || typed.eq_ignore_ascii_case(CONFIRM_WORD)
            }
            ExtraStep::ConfirmWord => typed.eq_ignore_ascii_case(CONFIRM_WORD),
        }
    }

    pub fn is_required(&self) -> bool {
        !matches!(self, ExtraStep::None)
    }

    /// What to ask the user for, in plain language.
    pub fn prompt(&self) -> Option<String> {
        match self {
            ExtraStep::None => None,
            ExtraStep::TableName { table } => Some(format!(
                "This connection is flagged production. Type the table's name, {table}, to continue."
            )),
            ExtraStep::CountOrConfirm { count } => Some(format!(
                "This connection is flagged production. Type {count} or the word {CONFIRM_WORD} to continue."
            )),
            ExtraStep::ConfirmWord => Some(format!(
                "This connection is flagged production. Type the word {CONFIRM_WORD} to continue."
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mydb_core::{Engine, Filter, Operation};

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

    #[test]
    fn nothing_extra_is_asked_of_a_connection_that_is_not_production() {
        for operation in [
            Operation::Delete,
            Operation::Update,
            Operation::DropTable,
            Operation::Truncate,
        ] {
            assert_eq!(
                ExtraStep::required_for(&intent(operation), false, 5),
                ExtraStep::None,
                "{operation:?} on an unflagged connection"
            );
        }
    }

    #[test]
    fn an_insert_needs_no_extra_step_even_on_production() {
        // It creates a record and destroys nothing.
        assert_eq!(
            ExtraStep::required_for(&intent(Operation::Insert), true, 1),
            ExtraStep::None
        );
    }

    #[test]
    fn a_read_needs_no_extra_step() {
        assert_eq!(
            ExtraStep::required_for(&intent(Operation::Read), true, 100),
            ExtraStep::None
        );
    }

    #[test]
    fn a_schema_change_asks_for_the_table_name_whatever_the_row_count() {
        for count in [0, 1, 5_000] {
            for operation in [Operation::DropTable, Operation::Truncate] {
                assert_eq!(
                    ExtraStep::required_for(&intent(operation), true, count),
                    ExtraStep::TableName {
                        table: "users".to_string()
                    },
                    "{operation:?} with {count} records"
                );
            }
        }
    }

    #[test]
    fn a_delete_or_update_of_many_records_accepts_the_count_or_the_word() {
        for operation in [Operation::Delete, Operation::Update] {
            let step = ExtraStep::required_for(&intent(operation), true, 42);
            assert_eq!(step, ExtraStep::CountOrConfirm { count: 42 });
            assert!(step.accepts("42"));
            assert!(step.accepts("CONFIRM"));
            assert!(!step.accepts("41"));
            assert!(!step.accepts(""));
        }
    }

    #[test]
    fn a_tiny_delete_or_update_will_not_accept_the_count() {
        // Typing "0" or "1" is a keystroke, not a moment of thought.
        for count in [0, 1] {
            for operation in [Operation::Delete, Operation::Update] {
                let step = ExtraStep::required_for(&intent(operation), true, count);
                assert_eq!(step, ExtraStep::ConfirmWord);
                assert!(
                    !step.accepts(&count.to_string()),
                    "typing {count} must not be enough"
                );
                assert!(step.accepts("CONFIRM"));
            }
        }
    }

    #[test]
    fn the_table_name_must_actually_match() {
        let step = ExtraStep::TableName {
            table: "users".to_string(),
        };
        assert!(step.accepts("users"));
        assert!(
            step.accepts("  Users  "),
            "space and case are not the point"
        );
        assert!(!step.accepts("user"));
        assert!(!step.accepts("orders"));
        assert!(!step.accepts(""));
        assert!(
            !step.accepts(CONFIRM_WORD),
            "the word is not a substitute for naming the table"
        );
    }

    #[test]
    fn a_qualified_table_is_asked_for_in_full() {
        let intent = Intent {
            namespace: "analytics".to_string(),
            ..intent(Operation::DropTable)
        };
        assert_eq!(
            ExtraStep::required_for(&intent, true, 3),
            ExtraStep::TableName {
                table: "analytics.users".to_string()
            }
        );
    }

    #[test]
    fn nothing_typed_satisfies_a_step_that_is_not_required() {
        assert!(ExtraStep::None.accepts(""));
        assert!(!ExtraStep::None.is_required());
    }
}
