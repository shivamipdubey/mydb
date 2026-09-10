//! Building SQL for the Postgres-family adapters.
//!
//! Two rules hold everywhere in this module, and both exist to satisfy
//! docs/16-security-and-cybersafety-checklist.md item 3:
//!
//! - Every value from a user's command becomes a bound parameter. No value is
//!   ever formatted into the statement text.
//! - Every identifier comes from the schema MYDB read out of the database
//!   itself, never from what the user typed, and is quoted on the way in.
//!
//! Together those mean the only things placed into statement text are fixed
//! strings from this file and names the database already told us exist.

use mydb_core::{Comparison, Filter, Value};
use tokio_postgres::types::ToSql;

/// An owned parameter value, ready to bind.
///
/// Owned rather than borrowed because the statement and its parameters are
/// built together and handed to the driver as a unit.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParam {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl SqlParam {
    fn as_to_sql(&self) -> &(dyn ToSql + Sync) {
        match self {
            SqlParam::Text(value) => value,
            SqlParam::Integer(value) => value,
            SqlParam::Float(value) => value,
            SqlParam::Boolean(value) => value,
        }
    }
}

/// Borrows a parameter list in the shape the driver wants.
pub fn as_driver_params(params: &[SqlParam]) -> Vec<&(dyn ToSql + Sync)> {
    params.iter().map(SqlParam::as_to_sql).collect()
}

/// Quotes an identifier for Postgres.
///
/// Identifiers reaching this function come from `information_schema`, so they
/// are already real names rather than user input. Quoting anyway costs
/// nothing and means a table named `order` or `select` works, and that a
/// future caller who forgets where the name came from is still safe.
pub fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The fully qualified, quoted name of a table.
pub fn quote_table(namespace: &str, table: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(namespace),
        quote_identifier(table)
    )
}

/// A rendered WHERE clause and the parameters it expects.
pub struct WhereClause {
    /// Includes the leading " WHERE ", or is empty when the filter matches
    /// everything. Empty means the statement applies to the whole table.
    pub sql: String,
    pub params: Vec<SqlParam>,
}

/// Renders a filter into a WHERE clause with bound parameters.
///
/// The `$n` placeholders are numbered from `first_placeholder`, so a caller
/// that has already bound parameters earlier in the statement can continue the
/// sequence.
pub fn build_where(filter: &Filter, first_placeholder: usize) -> WhereClause {
    if filter.matches_everything() {
        return WhereClause {
            sql: String::new(),
            params: Vec::new(),
        };
    }

    let mut fragments = Vec::with_capacity(filter.conditions.len());
    let mut params = Vec::new();
    let mut placeholder = first_placeholder;

    for condition in &filter.conditions {
        let column = quote_identifier(&condition.column);

        // NULL needs IS / IS NOT rather than an equality operator, which would
        // silently match nothing instead of matching null rows.
        if matches!(condition.value, Value::Null) {
            let test = match condition.comparison {
                Comparison::NotEquals => "IS NOT NULL",
                _ => "IS NULL",
            };
            fragments.push(format!("{column} {test}"));
            continue;
        }

        let operator = condition.comparison.operator();
        match &condition.value {
            Value::Text(text) => {
                fragments.push(format!("{column} {operator} ${placeholder}"));
                params.push(SqlParam::Text(text.clone()));
            }
            Value::Integer(number) => {
                fragments.push(format!("{column} {operator} ${placeholder}"));
                params.push(SqlParam::Integer(*number));
            }
            Value::Float(number) => {
                // numeric columns will not accept a float bind directly, so
                // the parameter is cast rather than the column, which would
                // prevent an index from being used.
                fragments.push(format!(
                    "{column} {operator} ${placeholder}::double precision"
                ));
                params.push(SqlParam::Float(*number));
            }
            Value::Boolean(value) => {
                fragments.push(format!("{column} {operator} ${placeholder}"));
                params.push(SqlParam::Boolean(*value));
            }
            Value::Date(date) => {
                // ($n::text)::date, not $n::date. Postgres infers a parameter's
                // type from its comparison and would demand a date, rejecting
                // the text bind.
                fragments.push(format!("{column} {operator} (${placeholder}::text)::date"));
                params.push(SqlParam::Text(date.clone()));
            }
            Value::Null => unreachable!("handled above"),
        }
        placeholder += 1;
    }

    WhereClause {
        sql: format!(" WHERE {}", fragments.join(" AND ")),
        params,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;
    use mydb_core::Condition;

    fn condition(column: &str, comparison: Comparison, value: Value) -> Condition {
        Condition {
            column: column.to_string(),
            comparison,
            value,
        }
    }

    #[test]
    fn an_empty_filter_produces_no_where_clause() {
        let clause = build_where(&Filter::everything(), 1);
        assert!(clause.sql.is_empty());
        assert!(clause.params.is_empty());
    }

    #[test]
    fn values_become_parameters_never_literals() {
        let clause = build_where(
            &Filter {
                conditions: vec![condition(
                    "email",
                    Comparison::Equals,
                    Value::Text("x'; DROP TABLE users; --".to_string()),
                )],
            },
            1,
        );

        assert_eq!(clause.sql, " WHERE \"email\" = $1");
        assert!(
            !clause.sql.contains("DROP"),
            "the value must not appear in statement text: {}",
            clause.sql
        );
        assert_eq!(
            clause.params,
            vec![SqlParam::Text("x'; DROP TABLE users; --".to_string())]
        );
    }

    #[test]
    fn placeholders_number_sequentially_across_conditions() {
        let clause = build_where(
            &Filter {
                conditions: vec![
                    condition("active", Comparison::Equals, Value::Boolean(false)),
                    condition("id", Comparison::GreaterThan, Value::Integer(2)),
                ],
            },
            1,
        );
        assert_eq!(clause.sql, " WHERE \"active\" = $1 AND \"id\" > $2");
        assert_eq!(clause.params.len(), 2);
    }

    #[test]
    fn dates_are_bound_as_text_and_cast() {
        let clause = build_where(
            &Filter {
                conditions: vec![condition(
                    "signup_date",
                    Comparison::LessThan,
                    Value::Date("2024-01-01".to_string()),
                )],
            },
            1,
        );
        assert_eq!(clause.sql, " WHERE \"signup_date\" < ($1::text)::date");
    }

    #[test]
    fn null_uses_is_null_rather_than_an_equality_test() {
        let clause = build_where(
            &Filter {
                conditions: vec![condition("email", Comparison::Equals, Value::Null)],
            },
            1,
        );
        assert_eq!(clause.sql, " WHERE \"email\" IS NULL");
        assert!(clause.params.is_empty());

        let negated = build_where(
            &Filter {
                conditions: vec![condition("email", Comparison::NotEquals, Value::Null)],
            },
            1,
        );
        assert_eq!(negated.sql, " WHERE \"email\" IS NOT NULL");
    }

    #[test]
    fn identifiers_are_quoted_and_embedded_quotes_escaped() {
        assert_eq!(quote_identifier("users"), "\"users\"");
        assert_eq!(quote_identifier("order"), "\"order\"");
        assert_eq!(quote_identifier("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(quote_table("public", "users"), "\"public\".\"users\"");
    }
}
