//! Parser behaviour, including the cases where it must refuse to guess.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_core::{Column, Comparison, Engine, Operation, Schema, Table, Value};
use mydb_parser::{parse, ParseError};

fn column(name: &str, data_type: &str) -> Column {
    Column {
        name: name.to_string(),
        data_type: data_type.to_string(),
        nullable: false,
    }
}

/// Mirrors testing/seed.sql so parser tests and adapter tests agree on shape.
fn schema() -> Schema {
    Schema {
        tables: vec![
            Table {
                namespace: "public".to_string(),
                name: "users".to_string(),
                columns: vec![
                    column("id", "integer"),
                    column("email", "text"),
                    column("full_name", "text"),
                    column("signup_date", "date"),
                    column("active", "boolean"),
                ],
            },
            Table {
                namespace: "public".to_string(),
                name: "orders".to_string(),
                columns: vec![
                    column("id", "integer"),
                    column("user_id", "integer"),
                    column("total", "numeric"),
                    column("placed_at", "date"),
                ],
            },
        ],
    }
}

fn parsed(input: &str) -> mydb_core::Intent {
    parse(input, &schema(), Engine::Postgres)
        .unwrap_or_else(|error| panic!("expected {input:?} to parse, got: {error}"))
}

fn error(input: &str) -> ParseError {
    parse(input, &schema(), Engine::Postgres)
        .expect_err(&format!("expected {input:?} to be rejected"))
}

// --- the operation must be classified correctly, every time ---

#[test]
fn a_delete_is_always_classified_as_a_write() {
    for input in [
        "delete every user who signed up before 2024",
        "delete users where active is false",
        "remove all orders",
        "erase users whose email is ada@example.com",
    ] {
        let intent = parsed(input);
        assert!(
            intent.is_write(),
            "{input:?} must be a write, or it would skip the confirmation step"
        );
        assert!(intent.is_destructive(), "{input:?} destroys data");
    }
}

#[test]
fn a_read_is_never_classified_as_a_write() {
    for input in [
        "show me all users",
        "list users where active is true",
        "find orders where total is greater than 50",
    ] {
        assert!(!parsed(input).is_write(), "{input:?} must not be a write");
    }
}

// --- the task's own acceptance example ---

#[test]
fn the_worked_example_parses_into_a_structured_filter() {
    let intent = parsed("delete every user who signed up before 2024");

    assert_eq!(intent.table, "users");
    assert_eq!(intent.namespace, "public");
    assert!(intent.is_write());

    let conditions = &intent.filter.conditions;
    assert_eq!(conditions.len(), 1);
    assert_eq!(conditions[0].column, "signup_date");
    assert_eq!(conditions[0].comparison, Comparison::LessThan);
    assert_eq!(conditions[0].value, Value::Date("2024-01-01".to_string()));

    assert_eq!(
        intent.describe(),
        "Delete records in users where signup date is before 2024-01-01"
    );
}

// --- values are typed from the column, not guessed ---

#[test]
fn values_are_typed_according_to_their_column() {
    assert_eq!(
        parsed("delete users where active is false")
            .filter
            .conditions[0]
            .value,
        Value::Boolean(false)
    );
    assert_eq!(
        parsed("delete users where id is 3").filter.conditions[0].value,
        Value::Integer(3)
    );
    assert_eq!(
        parsed("delete users where email is ada@example.com")
            .filter
            .conditions[0]
            .value,
        Value::Text("ada@example.com".to_string())
    );
    assert_eq!(
        parsed("delete orders where total is greater than 50")
            .filter
            .conditions[0]
            .value,
        Value::Float(50.0)
    );
    assert_eq!(
        parsed("delete users where signup date is before 2024-06-30")
            .filter
            .conditions[0]
            .value,
        Value::Date("2024-06-30".to_string())
    );
}

#[test]
fn multiple_conditions_combine() {
    let intent = parsed("delete users where active is false and id is greater than 2");
    assert_eq!(intent.filter.conditions.len(), 2);
    assert_eq!(
        intent.filter.conditions[1].comparison,
        Comparison::GreaterThan
    );
}

#[test]
fn a_command_without_a_filter_targets_every_record_and_says_so() {
    let intent = parsed("delete all users");
    assert!(intent.filter.matches_everything());
    assert_eq!(intent.describe(), "Delete every record in users");
}

#[test]
fn singular_and_plural_table_names_both_resolve() {
    assert_eq!(parsed("delete every user").table, "users");
    assert_eq!(parsed("delete all users").table, "users");
    assert_eq!(parsed("show me all orders").table, "orders");
}

// --- refusing to guess ---

#[test]
fn empty_input_is_rejected() {
    assert_eq!(error(""), ParseError::Empty);
    assert_eq!(error("   "), ParseError::Empty);
}

#[test]
fn an_unrecognised_command_is_rejected_rather_than_guessed() {
    assert!(matches!(
        error("please do something clever with the database"),
        ParseError::UnknownOperation { .. }
    ));
}

#[test]
fn an_unknown_table_is_named_along_with_what_does_exist() {
    match error("delete every invoice") {
        ParseError::UnknownTable { known, .. } => {
            assert!(known.contains(&"users".to_string()));
            assert!(known.contains(&"orders".to_string()));
        }
        other => panic!("expected UnknownTable, got {other:?}"),
    }
}

#[test]
fn an_unknown_column_is_named_along_with_what_does_exist() {
    match error("delete users where favourite colour is blue") {
        ParseError::UnknownColumn { known, .. } => {
            assert!(known.contains(&"email".to_string()));
        }
        other => panic!("expected UnknownColumn, got {other:?}"),
    }
}

#[test]
fn a_value_that_does_not_fit_its_column_is_rejected() {
    assert!(matches!(
        error("delete users where id is banana"),
        ParseError::UnparseableValue { .. }
    ));
    assert!(matches!(
        error("delete users where active is banana"),
        ParseError::UnparseableValue { .. }
    ));
    assert!(matches!(
        error("delete users where signup date is before yesterday"),
        ParseError::UnparseableValue { .. }
    ));
}

#[test]
fn schema_operations_are_recognised_as_acting_on_the_table() {
    for input in [
        "drop table users",
        "drop the users table",
        // "delete table users" means the table, not its rows. Reading it as
        // a row delete would show matching records instead of the structure
        // that is also about to go.
        "delete table users",
    ] {
        let intent = parsed(input);
        assert_eq!(intent.operation, Operation::DropTable, "{input:?}");
        assert_eq!(intent.table, "users");
        assert!(intent.is_destructive());
        assert!(intent.operation.is_schema_change());
    }

    for input in ["truncate users", "empty the users table", "clear users"] {
        let intent = parsed(input);
        assert_eq!(intent.operation, Operation::Truncate, "{input:?}");
        assert_eq!(intent.table, "users");
        assert!(intent.is_destructive());
    }
}

#[test]
fn a_row_delete_is_still_a_row_delete() {
    // The word "table" is what distinguishes the two, and only in the
    // subject: a row delete must not be promoted to dropping the table.
    let intent = parsed("delete users where active is false");
    assert_eq!(intent.operation, Operation::Delete);
    assert!(!intent.operation.is_schema_change());
}

#[test]
fn a_schema_operation_cannot_carry_a_condition() {
    // "truncate users where active is false" empties the whole table, not
    // part of it. Accepting the condition would make it look otherwise.
    for input in [
        "truncate users where active is false",
        "drop table users where id is 3",
    ] {
        assert!(
            matches!(error(input), ParseError::FilterNotAllowed { .. }),
            "{input:?} should be refused rather than silently widened"
        );
    }
}

#[test]
fn schema_operations_describe_what_survives_and_what_does_not() {
    assert_eq!(
        parsed("drop table users").describe(),
        "Drop the table users, removing its records and its structure"
    );
    assert_eq!(
        parsed("truncate users").describe(),
        "Remove every record from users, keeping the table itself"
    );
}

#[test]
fn an_ambiguous_table_name_is_reported_rather_than_picked() {
    let mut ambiguous = schema();
    ambiguous.tables.push(Table {
        namespace: "analytics".to_string(),
        name: "users".to_string(),
        columns: vec![column("id", "integer")],
    });

    let result = parse("delete every user", &ambiguous, Engine::Postgres);
    assert!(
        matches!(result, Err(ParseError::AmbiguousTable { .. })),
        "two tables named users must produce a question, not a coin flip"
    );
}

// --- docs/16 item 3: nothing the user types becomes query text ---

#[test]
fn a_value_containing_query_syntax_stays_a_value() {
    let intent = parsed("delete users where email is x'; DROP TABLE users; --");
    assert_eq!(intent.filter.conditions.len(), 1);
    assert_eq!(
        intent.filter.conditions[0].value,
        Value::Text("x'; DROP TABLE users; --".to_string()),
        "an injection attempt must survive as an ordinary text value, to be \
         bound as a parameter rather than parsed as syntax"
    );
    assert_eq!(intent.table, "users");
}

#[test]
fn a_value_keeps_the_case_the_user_typed() {
    // Regression: the parser lowercased the whole command, which silently
    // case-folded values. An email or a name is data, not a keyword.
    let intent = parsed("delete users where email is Ada@Example.COM");
    assert_eq!(
        intent.filter.conditions[0].value,
        Value::Text("Ada@Example.COM".to_string())
    );
}

#[test]
fn the_word_table_inside_a_value_does_not_change_the_operation() {
    // Regression: operation detection scanned the entire command for the word
    // "table", so a perfectly ordinary value turned a row delete into a
    // rejected DROP TABLE.
    let intent = parsed("delete users where full name is Table Mountain");
    assert!(intent.is_write());
    assert_eq!(intent.table, "users");
    assert_eq!(
        intent.filter.conditions[0].value,
        Value::Text("Table Mountain".to_string())
    );
}

#[test]
fn quoted_values_have_their_quotes_removed() {
    assert_eq!(
        parsed("delete users where email is \"ada@example.com\"")
            .filter
            .conditions[0]
            .value,
        Value::Text("ada@example.com".to_string())
    );
}

// --- inserts (T10) ---

#[test]
fn an_insert_parses_its_values_and_is_a_non_destructive_write() {
    let intent = parsed(
        "add a user with id is 8 and email is ada2@example.com and full name is Ada Lovelace",
    );

    assert_eq!(intent.operation, Operation::Insert);
    assert_eq!(intent.table, "users");
    assert!(intent.is_write(), "an insert changes data");
    assert!(
        !intent.is_destructive(),
        "an insert creates a record and destroys nothing"
    );

    assert_eq!(intent.assignments.len(), 3);
    assert_eq!(intent.assignments[0].column, "id");
    assert_eq!(intent.assignments[0].value, Value::Integer(8));
    assert_eq!(
        intent.assignments[2].value,
        Value::Text("Ada Lovelace".to_string()),
        "the value keeps the case the user typed"
    );
}

#[test]
fn insert_accepts_commas_as_well_as_and() {
    let intent = parsed("insert into users with id = 9, email = z@example.com");
    assert_eq!(intent.assignments.len(), 2);
    assert_eq!(intent.assignments[1].column, "email");
}

#[test]
fn an_insert_with_no_values_is_refused() {
    assert!(matches!(
        error("add a user"),
        ParseError::MissingValues { .. }
    ));
}

#[test]
fn an_insert_cannot_carry_a_condition() {
    // Selecting records makes no sense for a record that does not exist yet.
    assert!(matches!(
        error("add a user with id is 8 where active is true"),
        ParseError::FilterOnInsert
    ));
}

// --- updates (T10) ---

#[test]
fn an_update_parses_its_values_and_its_filter() {
    let intent = parsed("update users set active to false where id is 3");

    assert_eq!(intent.operation, Operation::Update);
    assert_eq!(intent.table, "users");
    assert!(intent.is_write());
    assert!(
        intent.is_destructive(),
        "an update overwrites values that were there before"
    );

    assert_eq!(intent.assignments.len(), 1);
    assert_eq!(intent.assignments[0].column, "active");
    assert_eq!(intent.assignments[0].value, Value::Boolean(false));

    assert_eq!(intent.filter.conditions.len(), 1);
    assert_eq!(intent.filter.conditions[0].column, "id");
}

#[test]
fn an_update_supports_the_set_for_form() {
    let intent = parsed("set active to true for users where id is 3");
    assert_eq!(intent.operation, Operation::Update);
    assert_eq!(intent.table, "users");
    assert_eq!(intent.assignments[0].column, "active");
    assert_eq!(intent.filter.conditions.len(), 1);
}

#[test]
fn an_unfiltered_update_parses_and_says_it_touches_everything() {
    let intent = parsed("update users set active to false");
    assert!(intent.filter.matches_everything());
    assert_eq!(
        intent.describe(),
        "Update every record in users, setting active = false"
    );
}

#[test]
fn an_update_with_no_values_is_refused() {
    assert!(matches!(
        error("update users"),
        ParseError::MissingValues { .. }
    ));
}

#[test]
fn an_assignment_to_an_unknown_column_is_refused() {
    assert!(matches!(
        error("update users set favourite colour to blue"),
        ParseError::UnknownColumn { .. }
    ));
}

#[test]
fn an_assignment_value_that_does_not_fit_its_column_is_refused() {
    assert!(matches!(
        error("update users set id to banana where id is 3"),
        ParseError::UnparseableValue { .. }
    ));
}

#[test]
fn written_values_keep_their_case_and_are_never_treated_as_syntax() {
    let intent = parsed("update users set full name to O'Brien; DROP TABLE users where id is 1");
    assert_eq!(
        intent.assignments[0].value,
        Value::Text("O'Brien; DROP TABLE users".to_string()),
        "a written value is data, to be bound as a parameter"
    );
}
