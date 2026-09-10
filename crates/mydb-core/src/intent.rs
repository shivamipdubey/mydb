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
            Operation::Delete => true,
        }
    }

    /// Whether this operation destroys data, which docs/11 requires an extra
    /// confirmation step for on a production-flagged connection.
    pub fn is_destructive(self) -> bool {
        match self {
            Operation::Read => false,
            Operation::Delete => true,
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Operation::Read => "read",
            Operation::Delete => "delete",
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
        }
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
        }
    }

    #[test]
    fn a_delete_is_a_write_and_a_read_is_not() {
        assert!(Operation::Delete.is_write());
        assert!(Operation::Delete.is_destructive());
        assert!(!Operation::Read.is_write());
        assert!(!Operation::Read.is_destructive());
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
