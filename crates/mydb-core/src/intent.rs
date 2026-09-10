//! The structured result of parsing a command (docs/05-confirmation-workflow.md
//! step 2, docs/22-glossary.md).
//!
//! An intent names the engine, the target, the operation, and the filter or
//! payload. Everything downstream, the preview, the confirmation screen, and
//! the execution, works from this and never from the user's raw text.
//!
//! One design rule matters more than the rest here: a filter holds structured
//! values, never a fragment of query text. An adapter binds those values as
//! parameters, so there is no place for a caller to concatenate user input
//! into a statement even if they wanted to
//! (docs/16-security-and-cybersafety-checklist.md item 3).

use serde::{Deserialize, Serialize};

use crate::Engine;

/// What a command asks MYDB to do.
///
/// Phase 1 (T6) covers reads and deletes. INSERT and UPDATE arrive in T10,
/// DROP TABLE and TRUNCATE in T11.
///
/// Deliberately NOT `#[non_exhaustive]`. Adding an operation should break
/// every `match` on this type across the workspace, forcing each one to be
/// handled on purpose. A catch-all arm is how a new destructive operation
/// quietly inherits the behaviour of a safe one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Read,
    Delete,
    Insert,
    Update,
    /// Removes a table: its records and its structure.
    DropTable,
    /// Removes every record from a table, leaving the table itself.
    Truncate,
}

impl Operation {
    /// Whether this operation changes data.
    ///
    /// The confirmation workflow branches on exactly this: a read runs
    /// directly, a write must show a preview first (docs/05 steps 4 and 5).
    /// Getting this wrong for any operation would route a write down the path
    /// that skips confirmation, so new variants must be classified here
    /// deliberately rather than falling through to a default.
    pub fn is_write(self) -> bool {
        match self {
            Operation::Read => false,
            Operation::Delete
            | Operation::Insert
            | Operation::Update
            | Operation::DropTable
            | Operation::Truncate => true,
        }
    }

    /// Whether this operation destroys data, which docs/11 requires an extra
    /// confirmation step for on a production-flagged connection.
    ///
    /// An update counts. docs/11 names "delete, drop, truncate, or similar",
    /// and an update overwrites values that were there before:
    /// docs/07-audit-log-and-recovery-bin.md treats overwritten data as
    /// something the recovery bin must hold, which is to say as data loss. An
    /// insert creates a row and destroys nothing, so it does not count.
    pub fn is_destructive(self) -> bool {
        match self {
            Operation::Read | Operation::Insert => false,
            Operation::Delete | Operation::Update | Operation::DropTable | Operation::Truncate => {
                true
            }
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Operation::Read => "read",
            Operation::Delete => "delete",
            Operation::Insert => "insert",
            Operation::Update => "update",
            Operation::DropTable => "drop_table",
            Operation::Truncate => "truncate",
        }
    }

    /// Whether this acts on the table itself rather than on a set of
    /// records, which decides what shape its preview takes (docs/04).
    pub fn is_schema_change(self) -> bool {
        match self {
            Operation::DropTable | Operation::Truncate => true,
            Operation::Read | Operation::Delete | Operation::Insert | Operation::Update => false,
        }
    }
}

/// A literal value in a filter, typed according to the column it compares
/// against so the adapter can bind it as a parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub enum Value {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    /// An ISO 8601 date, kept as text so this crate needs no date library and
    /// each adapter can cast it with its own engine's syntax.
    Date(String),
    Null,
}

impl Value {
    /// How this value should read on the confirmation screen.
    ///
    /// docs/12-ui-ux-guidelines.md requires the parsed intent to be shown in
    /// plain language, so values need a rendering that is not query syntax.
    pub fn display(&self) -> String {
        match self {
            Value::Text(text) => format!("\"{text}\""),
            Value::Integer(number) => number.to_string(),
            Value::Float(number) => number.to_string(),
            Value::Boolean(value) => value.to_string(),
            Value::Date(date) => date.clone(),
            Value::Null => "nothing".to_string(),
        }
    }
}

/// How a column is compared against a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Equals,
    NotEquals,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

impl Comparison {
    /// The comparison as an operator.
    ///
    /// Every variant maps to a fixed, hardcoded string. No user input reaches
    /// this, which is what lets an adapter place it into a statement safely.
    pub fn operator(self) -> &'static str {
        match self {
            Comparison::Equals => "=",
            Comparison::NotEquals => "<>",
            Comparison::LessThan => "<",
            Comparison::LessThanOrEqual => "<=",
            Comparison::GreaterThan => ">",
            Comparison::GreaterThanOrEqual => ">=",
        }
    }

    /// How this comparison reads in plain language.
    pub fn describe(self) -> &'static str {
        match self {
            Comparison::Equals => "is",
            Comparison::NotEquals => "is not",
            Comparison::LessThan => "is before",
            Comparison::LessThanOrEqual => "is on or before",
            Comparison::GreaterThan => "is after",
            Comparison::GreaterThanOrEqual => "is on or after",
        }
    }
}

/// One `column comparison value` test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// A real column name, already resolved against the schema. Never raw
    /// user text: the parser rejects a column it cannot find rather than
    /// passing something through to be interpolated later.
    pub column: String,
    pub comparison: Comparison,
    pub value: Value,
}

impl Condition {
    pub fn describe(&self) -> String {
        format!(
            "{} {} {}",
            self.column.replace('_', " "),
            self.comparison.describe(),
            self.value.display()
        )
    }
}

/// A value being written to a column, by an insert or an update.
///
/// Structured for the same reason a [`Condition`] is: the column name is
/// resolved against the schema before it gets here, and the value is bound as
/// a parameter rather than written into the statement
/// (docs/16-security-and-cybersafety-checklist.md item 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub column: String,
    pub value: Value,
}

impl Assignment {
    pub fn describe(&self) -> String {
        format!(
            "{} = {}",
            self.column.replace('_', " "),
            self.value.display()
        )
    }
}

/// Which records an operation applies to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    /// Combined with AND. An empty list means every record in the table,
    /// which for a write is the widest possible blast radius and is why
    /// [`Filter::matches_everything`] exists for the UI to warn about.
    pub conditions: Vec<Condition>,
}

impl Filter {
    pub fn everything() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    /// Whether this filter selects the entire table.
    ///
    /// The confirmation screen calls this out explicitly. An unfiltered delete
    /// is the single most damaging thing a user can confirm by accident, so it
    /// should never look like an ordinary filtered one.
    pub fn matches_everything(&self) -> bool {
        self.conditions.is_empty()
    }

    pub fn describe(&self) -> String {
        if self.matches_everything() {
            return "every record".to_string();
        }
        self.conditions
            .iter()
            .map(Condition::describe)
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

/// A parsed command, ready for preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub engine: Engine,
    /// The namespace-qualified table this targets, as resolved against the
    /// schema rather than as the user typed it.
    pub namespace: String,
    pub table: String,
    pub operation: Operation,
    pub filter: Filter,
    /// The values an insert or update writes. Empty for reads and deletes.
    #[serde(default)]
    pub assignments: Vec<Assignment>,
}

impl Intent {
    pub fn is_write(&self) -> bool {
        self.operation.is_write()
    }

    pub fn is_destructive(&self) -> bool {
        self.operation.is_destructive()
    }

    /// The table as the user should see it named.
    pub fn qualified_table(&self) -> String {
        if self.namespace == "public" {
            self.table.clone()
        } else {
            format!("{}.{}", self.namespace, self.table)
        }
    }

    /// The intent in plain language, for the confirmation screen.
    ///
    /// docs/12-ui-ux-guidelines.md requires this to be shown instead of raw
    /// query syntax, with the syntax available only as secondary detail. A
    /// user should be able to catch a misparse without reading SQL.
    pub fn describe(&self) -> String {
        let table = self.qualified_table();
        match self.operation {
            Operation::Read => {
                if self.filter.matches_everything() {
                    format!("Show every record in {table}")
                } else {
                    format!("Show records in {table} where {}", self.filter.describe())
                }
            }
            Operation::Delete => {
                if self.filter.matches_everything() {
                    format!("Delete every record in {table}")
                } else {
                    format!("Delete records in {table} where {}", self.filter.describe())
                }
            }
            Operation::Insert => {
                format!(
                    "Add one record to {table}, with {}",
                    self.describe_assignments()
                )
            }
            Operation::Update => {
                if self.filter.matches_everything() {
                    format!(
                        "Update every record in {table}, setting {}",
                        self.describe_assignments()
                    )
                } else {
                    format!(
                        "Update records in {table} where {}, setting {}",
                        self.filter.describe(),
                        self.describe_assignments()
                    )
                }
            }
            Operation::DropTable => {
                format!("Drop the table {table}, removing its records and its structure")
            }
            Operation::Truncate => {
                format!("Remove every record from {table}, keeping the table itself")
            }
        }
    }

    /// The values being written, in plain language.
    fn describe_assignments(&self) -> String {
        self.assignments
            .iter()
            .map(Assignment::describe)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delete_intent(filter: Filter) -> Intent {
        Intent {
            engine: Engine::Postgres,
            namespace: "public".to_string(),
            table: "users".to_string(),
            operation: Operation::Delete,
            filter,
            assignments: Vec::new(),
        }
    }

    #[test]
    fn every_operation_is_classified_deliberately() {
        assert!(Operation::Delete.is_write() && Operation::Delete.is_destructive());
        assert!(Operation::Update.is_write() && Operation::Update.is_destructive());
        // An insert writes but destroys nothing.
        assert!(Operation::Insert.is_write() && !Operation::Insert.is_destructive());
        assert!(!Operation::Read.is_write() && !Operation::Read.is_destructive());
    }

    #[test]
    fn an_insert_describes_the_record_it_would_create() {
        let intent = Intent {
            engine: Engine::Postgres,
            namespace: "public".to_string(),
            table: "users".to_string(),
            operation: Operation::Insert,
            filter: Filter::everything(),
            assignments: vec![
                Assignment {
                    column: "email".to_string(),
                    value: Value::Text("ada@example.com".to_string()),
                },
                Assignment {
                    column: "full_name".to_string(),
                    value: Value::Text("Ada Lovelace".to_string()),
                },
            ],
        };
        assert_eq!(
            intent.describe(),
            "Add one record to users, with email = \"ada@example.com\", full name = \"Ada Lovelace\""
        );
    }

    #[test]
    fn an_unfiltered_update_says_every_record_out_loud() {
        let intent = Intent {
            engine: Engine::Postgres,
            namespace: "public".to_string(),
            table: "users".to_string(),
            operation: Operation::Update,
            filter: Filter::everything(),
            assignments: vec![Assignment {
                column: "active".to_string(),
                value: Value::Boolean(false),
            }],
        };
        assert_eq!(
            intent.describe(),
            "Update every record in users, setting active = false"
        );
    }

    #[test]
    fn an_empty_filter_is_recognised_as_matching_everything() {
        assert!(Filter::everything().matches_everything());
        assert!(!Filter {
            conditions: vec![Condition {
                column: "active".to_string(),
                comparison: Comparison::Equals,
                value: Value::Boolean(false),
            }],
        }
        .matches_everything());
    }

    #[test]
    fn plain_language_description_avoids_query_syntax() {
        let intent = delete_intent(Filter {
            conditions: vec![Condition {
                column: "signup_date".to_string(),
                comparison: Comparison::LessThan,
                value: Value::Date("2024-01-01".to_string()),
            }],
        });
        assert_eq!(
            intent.describe(),
            "Delete records in users where signup date is before 2024-01-01"
        );
    }

    #[test]
    fn an_unfiltered_delete_says_every_record_out_loud() {
        assert_eq!(
            delete_intent(Filter::everything()).describe(),
            "Delete every record in users"
        );
    }
}
