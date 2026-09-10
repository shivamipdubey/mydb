//! Filters must work on every column type the schema can report, and text
//! matching must not depend on the user guessing stored capitalisation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::PostgresAdapter;
use mydb_adapters::Adapter;
use mydb_core::{Comparison, Condition, Engine, Filter, Intent, Operation, Value};

mod support;
use support::{details_from_env, reset_seed};

async fn adapter() -> PostgresAdapter {
    PostgresAdapter::connect(details_from_env())
        .await
        .expect("start the test database with ./scripts/db.sh up")
}

fn read(table: &str, column: &str, comparison: Comparison, value: Value) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: table.to_string(),
        operation: Operation::Read,
        filter: Filter {
            conditions: vec![Condition {
                column: column.to_string(),
                comparison,
                value,
            }],
        },
    }
}

async fn count(intent: Intent) -> u64 {
    let adapter = adapter().await;
    adapter
        .run_read(&intent)
        .await
        .unwrap_or_else(|error| panic!("filter failed: {error}"))
        .total_count()
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn filter_on_an_integer_column() {
    reset_seed().await;
    assert_eq!(
        count(read("users", "id", Comparison::Equals, Value::Integer(5))).await,
        1
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn filter_on_a_boolean_column() {
    reset_seed().await;
    assert_eq!(
        count(read(
            "users",
            "active",
            Comparison::Equals,
            Value::Boolean(true)
        ))
        .await,
        5
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn filter_on_a_date_column() {
    reset_seed().await;
    assert_eq!(
        count(read(
            "users",
            "signup_date",
            Comparison::LessThan,
            Value::Date("2024-01-01".to_string())
        ))
        .await,
        3
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn filter_on_a_text_column() {
    reset_seed().await;
    assert_eq!(
        count(read(
            "users",
            "email",
            Comparison::Equals,
            Value::Text("ada@example.com".to_string())
        ))
        .await,
        1
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn filter_on_a_numeric_column() {
    reset_seed().await;
    assert_eq!(
        count(read(
            "orders",
            "total",
            Comparison::GreaterThan,
            Value::Float(50.0)
        ))
        .await,
        2
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn text_filters_match_regardless_of_typed_capitalisation() {
    reset_seed().await;
    for typed in ["Alan Turing", "alan turing", "ALAN TURING", "aLaN tUrInG"] {
        assert_eq!(
            count(read(
                "users",
                "full_name",
                Comparison::Equals,
                Value::Text(typed.to_string())
            ))
            .await,
            1,
            "{typed:?} should match the stored \"Alan Turing\""
        );
    }
}

// --- the stored value must be untouched by any of this ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn matching_case_insensitively_does_not_alter_the_stored_case() {
    reset_seed().await;
    let adapter = adapter().await;

    let found = adapter
        .run_read(&read(
            "users",
            "full_name",
            Comparison::Equals,
            Value::Text("alan turing".to_string()),
        ))
        .await
        .unwrap();

    let name_index = found
        .columns()
        .iter()
        .position(|c| c == "full_name")
        .unwrap();
    assert_eq!(
        found.records()[0].cells[name_index].as_deref(),
        Some("Alan Turing"),
        "the row comes back with the capitalisation the database stores, \
         not the capitalisation that was typed to find it"
    );

    // And the stored row is still exactly as it was afterwards.
    let all = adapter
        .run_read(&read("users", "id", Comparison::Equals, Value::Integer(3)))
        .await
        .unwrap();
    assert_eq!(
        all.records()[0].cells[name_index].as_deref(),
        Some("Alan Turing")
    );
}

// --- preview and execute must agree, on typing and on case ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_delete_matched_case_insensitively_removes_exactly_the_previewed_row() {
    reset_seed().await;
    let adapter = adapter().await;

    let intent = Intent {
        operation: Operation::Delete,
        ..read(
            "users",
            "full_name",
            Comparison::Equals,
            Value::Text("aLaN tUrInG".to_string()),
        )
    };

    let preview = adapter.build_preview(&intent).await.unwrap();
    assert_eq!(preview.affected_count(), 1);

    let outcome = adapter.execute(preview.approve()).await.unwrap();
    assert_eq!(
        outcome.rows_affected, 1,
        "execute must match the same row the preview matched, or the two \
         would disagree about what the user confirmed"
    );

    // Alan is gone; nobody else went with him.
    assert_eq!(
        count(read(
            "users",
            "full_name",
            Comparison::Equals,
            Value::Text("Alan Turing".to_string())
        ))
        .await,
        0
    );
    assert_eq!(
        count(Intent {
            filter: Filter::everything(),
            ..read("users", "id", Comparison::Equals, Value::Integer(1))
        })
        .await,
        6
    );

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_delete_on_an_integer_column_previews_and_executes() {
    // The integer binding has to work on the write path too, not only the
    // read path that first surfaced the bug.
    reset_seed().await;
    let adapter = adapter().await;

    let intent = Intent {
        operation: Operation::Delete,
        ..read("users", "id", Comparison::Equals, Value::Integer(3))
    };

    let preview = adapter.build_preview(&intent).await.unwrap();
    assert_eq!(preview.affected_count(), 1);

    let outcome = adapter.execute(preview.approve()).await.unwrap();
    assert_eq!(outcome.rows_affected, 1);

    reset_seed().await;
}
