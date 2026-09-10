//! Building SQL for the Postgres-family adapters.
//!
//! Three rules hold everywhere in this module:
//!
//! - Every value from a user's command becomes a bound parameter. No value is
//!   ever formatted into the statement text
//!   (docs/16-security-and-cybersafety-checklist.md item 3).
//! - Every identifier comes from the schema MYDB read out of the database
//!   itself, never from what the user typed, and is quoted on the way in.
//! - Every parameter is bound or cast according to the target column's real
//!   type, taken from that same schema. Binding a value without knowing what
//!   it will be compared against is how the date bug of T3 and the integer
//!   bug after T9 both happened; typing each parameter from the schema fixes
//!   the whole class rather than one type at a time.

use mydb_core::{Assignment, Column, Comparison, Filter, Value};
use tokio_postgres::types::ToSql;

/// An owned parameter value, ready to bind.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParam {
    Text(String),
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Real(f32),
    Double(f64),
    Boolean(bool),
}

impl SqlParam {
    fn as_to_sql(&self) -> &(dyn ToSql + Sync) {
        match self {
            SqlParam::Text(value) => value,
            SqlParam::SmallInt(value) => value,
            SqlParam::Integer(value) => value,
            SqlParam::BigInt(value) => value,
            SqlParam::Real(value) => value,
            SqlParam::Double(value) => value,
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

/// How a parameter should reach a column of a given type.
#[derive(Debug, Clone, PartialEq)]
enum Binding {
    /// The driver has an exact Rust type for this column, so bind natively.
    Native,
    /// Text, compared case-insensitively for equality.
    Text,
    /// Bind text and cast the parameter to this SQL type.
    ///
    /// The cast is on the parameter, not the column, so an index on the
    /// column can still be used.
    CastTo(&'static str),
    /// An exotic or unrecognised type. Compare both sides as text, which
    /// always works, at the cost of not using an index.
    CompareAsText,
}

/// Decides how to bind against a column, from the type the database reported.
///
/// `information_schema.columns.data_type` uses a fixed SQL-standard
/// vocabulary, so these are matched exactly rather than guessed at. Anything
/// not listed falls through to a comparison that is always correct even if it
/// is not the fastest, which is the right way round for a tool whose job is
/// showing people the truth about their data.
fn binding_for(data_type: &str) -> Binding {
    match data_type {
        "smallint" | "integer" | "bigint" | "real" | "double precision" | "boolean" => {
            Binding::Native
        }
        "text" | "character varying" | "character" | "name" | "citext" => Binding::Text,
        "numeric" => Binding::CastTo("numeric"),
        "date" => Binding::CastTo("date"),
        "timestamp without time zone" => Binding::CastTo("timestamp without time zone"),
        "timestamp with time zone" => Binding::CastTo("timestamp with time zone"),
        "time without time zone" => Binding::CastTo("time without time zone"),
        "time with time zone" => Binding::CastTo("time with time zone"),
        "interval" => Binding::CastTo("interval"),
        "uuid" => Binding::CastTo("uuid"),
        "json" | "jsonb" => Binding::CompareAsText,
        _ => Binding::CompareAsText,
    }
}

/// The value as text, for the bindings that send text.
///
/// This is a rendering for the database, not a change to the value: the
/// stored data and the text the user typed are both untouched.
fn as_text(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        Value::Integer(number) => number.to_string(),
        Value::Float(number) => number.to_string(),
        Value::Boolean(flag) => flag.to_string(),
        Value::Date(date) => date.clone(),
        Value::Null => String::new(),
    }
}

/// Binds a value natively for a column the driver has an exact type for.
///
/// Returns `None` when the value cannot be represented in that type, for
/// instance a number too large for the column. The caller then falls back to
/// a text cast, so the database reports a clear type error rather than the
/// driver failing with an opaque serialisation message.
fn bind_native(data_type: &str, value: &Value) -> Option<SqlParam> {
    let text = as_text(value);
    match data_type {
        // The width matters: binding an i64 against an int4 column is exactly
        // what produced "error serializing parameter 0".
        "smallint" => text.parse::<i16>().ok().map(SqlParam::SmallInt),
        "integer" => text.parse::<i32>().ok().map(SqlParam::Integer),
        "bigint" => text.parse::<i64>().ok().map(SqlParam::BigInt),
        "real" => text.parse::<f32>().ok().map(SqlParam::Real),
        "double precision" => text.parse::<f64>().ok().map(SqlParam::Double),
        "boolean" => match text.as_str() {
            "true" | "t" | "yes" | "on" | "1" => Some(SqlParam::Boolean(true)),
            "false" | "f" | "no" | "off" | "0" => Some(SqlParam::Boolean(false)),
            _ => None,
        },
        _ => None,
    }
}

/// The parameter side of a comparison or assignment, and the value to bind.
///
/// Shared by filters, inserts, and updates so all three type a value the same
/// way. They must agree: a preview that matched a row one way and an update
/// that wrote it another would be two different statements wearing the same
/// description.
struct BoundValue {
    /// The SQL to place where the value goes, such as `$1` or `($1::text)::date`.
    expression: String,
    param: SqlParam,
}

/// Types a single value against the column it is destined for.
///
/// Note this deliberately does not fold case. Case folding belongs to a
/// comparison, and only to a comparison: applying it here would lowercase the
/// data an insert or update actually writes.
fn bind_value(data_type: &str, value: &Value, placeholder: usize) -> BoundValue {
    match binding_for(data_type) {
        Binding::Native => match bind_native(data_type, value) {
            Some(param) => BoundValue {
                expression: format!("${placeholder}"),
                param,
            },
            // The value does not fit the column. Send it as text and let the
            // database explain why, which is a far clearer error than a
            // driver-level serialisation failure.
            None => BoundValue {
                expression: format!("(${placeholder}::text)::{data_type}"),
                param: SqlParam::Text(as_text(value)),
            },
        },
        Binding::Text => BoundValue {
            expression: format!("${placeholder}"),
            param: SqlParam::Text(as_text(value)),
        },
        Binding::CastTo(sql_type) => BoundValue {
            expression: format!("(${placeholder}::text)::{sql_type}"),
            param: SqlParam::Text(as_text(value)),
        },
        Binding::CompareAsText => BoundValue {
            expression: format!("${placeholder}"),
            param: SqlParam::Text(as_text(value)),
        },
    }
}

/// Looks up a column's type, or an empty string when the column is unknown.
///
/// An unknown column produces a statement the database will reject by name,
/// which is more useful than failing here with less context.
fn type_of<'a>(columns: &'a [Column], name: &str) -> &'a str {
    columns
        .iter()
        .find(|column| column.name == name)
        .map(|column| column.data_type.as_str())
        .unwrap_or("")
}

/// Whether writing to this column type is supported yet.
///
/// An exotic type can be read and compared as text, but writing one needs a
/// cast MYDB cannot construct without knowing the underlying type name.
/// Refusing is better than writing something subtly wrong.
pub fn can_write_to(data_type: &str) -> bool {
    !matches!(binding_for(data_type), Binding::CompareAsText)
}

/// A rendered INSERT statement and its parameters.
pub struct InsertStatement {
    pub sql: String,
    pub params: Vec<SqlParam>,
}

/// Builds `INSERT INTO table (cols) VALUES (...)`.
pub fn build_insert(
    table: &str,
    assignments: &[Assignment],
    columns: &[Column],
) -> InsertStatement {
    let mut names = Vec::with_capacity(assignments.len());
    let mut values = Vec::with_capacity(assignments.len());
    let mut params = Vec::with_capacity(assignments.len());

    for (index, assignment) in assignments.iter().enumerate() {
        let bound = bind_value(
            type_of(columns, &assignment.column),
            &assignment.value,
            index + 1,
        );
        names.push(quote_identifier(&assignment.column));
        values.push(bound.expression);
        params.push(bound.param);
    }

    InsertStatement {
        sql: format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            names.join(", "),
            values.join(", ")
        ),
        params,
    }
}

/// A rendered SET clause and its parameters.
pub struct SetClause {
    /// Includes the leading " SET ".
    pub sql: String,
    pub params: Vec<SqlParam>,
    /// The next free placeholder number, for the WHERE clause that follows.
    pub next_placeholder: usize,
}

/// Builds `SET col = value, ...` for an update.
pub fn build_set(
    assignments: &[Assignment],
    columns: &[Column],
    first_placeholder: usize,
) -> SetClause {
    let mut fragments = Vec::with_capacity(assignments.len());
    let mut params = Vec::with_capacity(assignments.len());
    let mut placeholder = first_placeholder;

    for assignment in assignments {
        let bound = bind_value(
            type_of(columns, &assignment.column),
            &assignment.value,
            placeholder,
        );
        fragments.push(format!(
            "{} = {}",
            quote_identifier(&assignment.column),
            bound.expression
        ));
        params.push(bound.param);
        placeholder += 1;
    }

    SetClause {
        sql: format!(" SET {}", fragments.join(", ")),
        params,
        next_placeholder: placeholder,
    }
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
/// `columns` is the target table's real schema. It is what makes each
/// parameter correctly typed, and it is why this function is given the table
/// rather than just the filter.
///
/// The `$n` placeholders are numbered from `first_placeholder`, so a caller
/// that has already bound parameters earlier in the statement can continue
/// the sequence.
pub fn build_where(filter: &Filter, columns: &[Column], first_placeholder: usize) -> WhereClause {
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
        let quoted = quote_identifier(&condition.column);

        // NULL needs IS / IS NOT rather than an equality operator, which
        // would silently match nothing instead of matching null rows.
        if matches!(condition.value, Value::Null) {
            let test = match condition.comparison {
                Comparison::NotEquals => "IS NOT NULL",
                _ => "IS NULL",
            };
            fragments.push(format!("{quoted} {test}"));
            continue;
        }

        let operator = condition.comparison.operator();
        let data_type = type_of(columns, &condition.column);
        let bound = bind_value(data_type, &condition.value, placeholder);
        let is_texty = matches!(
            binding_for(data_type),
            Binding::Text | Binding::CompareAsText
        );
        let comparable = match binding_for(data_type) {
            // An exotic type has no cast MYDB can rely on, so both sides are
            // compared as text. Always correct, at the cost of an index.
            Binding::CompareAsText => format!("{quoted}::text"),
            _ => quoted.clone(),
        };

        match (is_texty, condition.comparison) {
            // Case-insensitive, because nobody typing a command in plain
            // language should have to guess the capitalisation a database
            // happens to store. Only the comparison folds case: the stored
            // value and the typed value are both left exactly as they are.
            (true, Comparison::Equals | Comparison::NotEquals) => fragments.push(format!(
                "lower({comparable}) {operator} lower({})",
                bound.expression
            )),
            // Ordering comparisons keep the database's own collation. Folding
            // case there would change which rows sort where, which is a
            // different question from whether two names are the same name.
            _ => fragments.push(format!("{comparable} {operator} {}", bound.expression)),
        }
        params.push(bound.param);

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

    fn column(name: &str, data_type: &str) -> Column {
        Column {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: false,
        }
    }

    fn users() -> Vec<Column> {
        vec![
            column("id", "integer"),
            column("email", "text"),
            column("full_name", "text"),
            column("signup_date", "date"),
            column("active", "boolean"),
            column("score", "bigint"),
            column("rating", "smallint"),
            column("total", "numeric"),
            column("ratio", "double precision"),
            column("tags", "ARRAY"),
        ]
    }

    fn condition(name: &str, comparison: Comparison, value: Value) -> Filter {
        Filter {
            conditions: vec![Condition {
                column: name.to_string(),
                comparison,
                value,
            }],
        }
    }

    #[test]
    fn an_empty_filter_produces_no_where_clause() {
        let clause = build_where(&Filter::everything(), &users(), 1);
        assert!(clause.sql.is_empty());
        assert!(clause.params.is_empty());
    }

    #[test]
    fn values_become_parameters_never_literals() {
        let clause = build_where(
            &condition(
                "email",
                Comparison::Equals,
                Value::Text("x'; DROP TABLE users; --".to_string()),
            ),
            &users(),
            1,
        );

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

    // --- each column type binds to the width the column actually is ---

    #[test]
    fn an_integer_column_binds_a_four_byte_integer() {
        // Binding an i64 here is what produced "error serializing parameter
        // 0": int8 is not int4, and the driver will not guess.
        let clause = build_where(
            &condition("id", Comparison::Equals, Value::Integer(5)),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"id\" = $1");
        assert_eq!(clause.params, vec![SqlParam::Integer(5)]);
    }

    #[test]
    fn smallint_and_bigint_bind_to_their_own_widths() {
        let small = build_where(
            &condition("rating", Comparison::Equals, Value::Integer(3)),
            &users(),
            1,
        );
        assert_eq!(small.params, vec![SqlParam::SmallInt(3)]);

        let big = build_where(
            &condition("score", Comparison::Equals, Value::Integer(3)),
            &users(),
            1,
        );
        assert_eq!(big.params, vec![SqlParam::BigInt(3)]);
    }

    #[test]
    fn a_boolean_column_binds_a_boolean() {
        let clause = build_where(
            &condition("active", Comparison::Equals, Value::Boolean(true)),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"active\" = $1");
        assert_eq!(clause.params, vec![SqlParam::Boolean(true)]);
    }

    #[test]
    fn a_double_column_binds_a_double() {
        let clause = build_where(
            &condition("ratio", Comparison::GreaterThan, Value::Float(1.5)),
            &users(),
            1,
        );
        assert_eq!(clause.params, vec![SqlParam::Double(1.5)]);
    }

    #[test]
    fn a_date_column_casts_the_parameter_not_the_column() {
        // Casting the column instead would prevent an index being used.
        let clause = build_where(
            &condition(
                "signup_date",
                Comparison::LessThan,
                Value::Date("2024-01-01".to_string()),
            ),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"signup_date\" < ($1::text)::date");
        assert_eq!(
            clause.params,
            vec![SqlParam::Text("2024-01-01".to_string())]
        );
    }

    #[test]
    fn a_numeric_column_casts_rather_than_going_through_a_float() {
        // numeric is exact; routing it through f64 would lose that.
        let clause = build_where(
            &condition("total", Comparison::GreaterThan, Value::Float(50.0)),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"total\" > ($1::text)::numeric");
    }

    #[test]
    fn an_unrecognised_column_type_still_produces_a_valid_comparison() {
        let clause = build_where(
            &condition("tags", Comparison::Equals, Value::Text("a".to_string())),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE lower(\"tags\"::text) = lower($1)");
    }

    #[test]
    fn a_value_that_does_not_fit_its_column_falls_back_to_a_cast() {
        // Rather than failing inside the driver with an opaque message, the
        // database gets a chance to say what is wrong.
        let clause = build_where(
            &condition(
                "id",
                Comparison::Equals,
                Value::Text("not-a-number".to_string()),
            ),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"id\" = ($1::text)::integer");
    }

    // --- text comparison folds case, and only the comparison ---

    #[test]
    fn text_equality_is_case_insensitive() {
        let clause = build_where(
            &condition(
                "full_name",
                Comparison::Equals,
                Value::Text("alan turing".to_string()),
            ),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE lower(\"full_name\") = lower($1)");
        assert_eq!(
            clause.params,
            vec![SqlParam::Text("alan turing".to_string())],
            "the typed value must be passed through exactly as typed"
        );
    }

    #[test]
    fn text_inequality_is_also_case_insensitive() {
        let clause = build_where(
            &condition(
                "email",
                Comparison::NotEquals,
                Value::Text("Ada@Example.com".to_string()),
            ),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE lower(\"email\") <> lower($1)");
        assert_eq!(
            clause.params,
            vec![SqlParam::Text("Ada@Example.com".to_string())],
            "case folding belongs in the comparison, never in the value"
        );
    }

    #[test]
    fn text_ordering_keeps_the_databases_own_collation() {
        let clause = build_where(
            &condition(
                "full_name",
                Comparison::LessThan,
                Value::Text("M".to_string()),
            ),
            &users(),
            1,
        );
        assert_eq!(
            clause.sql, " WHERE \"full_name\" < $1",
            "whether two names are the same is a different question from how they sort"
        );
    }

    #[test]
    fn null_uses_is_null_rather_than_an_equality_test() {
        let clause = build_where(
            &condition("email", Comparison::Equals, Value::Null),
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"email\" IS NULL");
        assert!(clause.params.is_empty());

        let negated = build_where(
            &condition("email", Comparison::NotEquals, Value::Null),
            &users(),
            1,
        );
        assert_eq!(negated.sql, " WHERE \"email\" IS NOT NULL");
    }

    #[test]
    fn placeholders_number_sequentially_across_conditions() {
        let clause = build_where(
            &Filter {
                conditions: vec![
                    Condition {
                        column: "active".to_string(),
                        comparison: Comparison::Equals,
                        value: Value::Boolean(false),
                    },
                    Condition {
                        column: "id".to_string(),
                        comparison: Comparison::GreaterThan,
                        value: Value::Integer(2),
                    },
                ],
            },
            &users(),
            1,
        );
        assert_eq!(clause.sql, " WHERE \"active\" = $1 AND \"id\" > $2");
        assert_eq!(
            clause.params,
            vec![SqlParam::Boolean(false), SqlParam::Integer(2)]
        );
    }

    #[test]
    fn identifiers_are_quoted_and_embedded_quotes_escaped() {
        assert_eq!(quote_identifier("users"), "\"users\"");
        assert_eq!(quote_identifier("order"), "\"order\"");
        assert_eq!(quote_identifier("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(quote_table("public", "users"), "\"public\".\"users\"");
    }
}
