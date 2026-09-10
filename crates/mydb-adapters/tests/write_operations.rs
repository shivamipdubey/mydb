//! INSERT and UPDATE, through the same preview-then-execute loop as DELETE.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::PostgresAdapter;
use mydb_adapters::{Adapter, AdapterError};
use mydb_core::{
    Assignment, Comparison, Condition, Engine, Filter, Intent, MemorySink, Operation, Value,
};

mod support;
use support::{details_from_env, reset_seed};

async fn adapter() -> PostgresAdapter {
    PostgresAdapter::connect(details_from_env())
        .await
        .expect("start the test database with ./scripts/db.sh up")
}

fn assignment(column: &str, value: Value) -> Assignment {
    Assignment {
        column: column.to_string(),
        value,
    }
}

fn intent(operation: Operation, filter: Filter, assignments: Vec<Assignment>) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: "users".to_string(),
        operation,
        filter,
        assignments,
    }
}

fn by_id(id: i64) -> Filter {
    Filter {
        conditions: vec![Condition {
            column: "id".to_string(),
            comparison: Comparison::Equals,
            value: Value::Integer(id),
        }],
    }
}

fn new_user() -> Vec<Assignment> {
    vec![
        assignment("id", Value::Integer(8)),
        assignment("email", Value::Text("Grace2@Example.com".to_string())),
        assignment("full_name", Value::Text("Grace Hopper II".to_string())),
        assignment("signup_date", Value::Date("2026-03-01".to_string())),
        assignment("active", Value::Boolean(true)),
    ]
}

/// Reads one cell from the row matching a filter.
async fn cell(adapter: &PostgresAdapter, filter: Filter, column: &str) -> Option<String> {
    let records = adapter
        .run_read(&intent(Operation::Read, filter, Vec::new()))
        .await
        .unwrap();
    let index = records.columns().iter().position(|c| c == column)?;
    records.records().first()?.cells[index].clone()
}

async fn total_users(adapter: &PostgresAdapter) -> u64 {
    adapter
        .run_read(&intent(Operation::Read, Filter::everything(), Vec::new()))
        .await
        .unwrap()
        .total_count()
}

// --- INSERT ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn insert_preview_shows_the_exact_record_that_will_be_created() {
    reset_seed().await;
    let adapter = adapter().await;

    let create = intent(Operation::Insert, Filter::everything(), new_user());
    let preview = adapter.build_preview(&create).await.unwrap();

    assert_eq!(preview.affected_count(), 1, "one record will be created");
    assert_eq!(preview.rows().len(), 1);

    // Every column of the table, so the user can see what lands where.
    assert_eq!(
        preview.columns(),
        ["id", "email", "full_name", "signup_date", "active"]
    );
    let cells = &preview.rows()[0].cells;
    assert_eq!(cells[0].as_deref(), Some("8"));
    assert_eq!(
        cells[1].as_deref(),
        Some("Grace2@Example.com"),
        "the value is shown exactly as it will be stored"
    );
    assert_eq!(cells[4].as_deref(), Some("true"));

    // And nothing has been created yet.
    assert_eq!(total_users(&adapter).await, 7);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn insert_preview_reads_nothing_from_the_table_itself() {
    // docs/04: an insert preview needs no read against existing data. An
    // empty table is the clearest way to show it does not depend on one.
    reset_seed().await;
    let adapter = adapter().await;

    let empty = Intent {
        table: "orders".to_string(),
        ..intent(
            Operation::Insert,
            Filter::everything(),
            vec![
                assignment("id", Value::Integer(99)),
                assignment("user_id", Value::Integer(1)),
                assignment("total", Value::Float(12.5)),
                assignment("placed_at", Value::Date("2026-01-01".to_string())),
            ],
        )
    };

    let preview = adapter.build_preview(&empty).await.unwrap();
    assert_eq!(preview.affected_count(), 1);
    assert!(
        preview.statement().starts_with("INSERT INTO"),
        "the statement shown should be the insert itself, not a select: {}",
        preview.statement()
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn insert_executes_only_after_its_preview_and_stores_the_value_verbatim() {
    reset_seed().await;
    let adapter = adapter().await;

    let create = intent(Operation::Insert, Filter::everything(), new_user());
    let preview = adapter.build_preview(&create).await.unwrap();
    let outcome = adapter
        .execute(preview.approve(), &mut MemorySink::default())
        .await
        .unwrap();

    assert_eq!(outcome.rows_affected, 1);
    assert_eq!(total_users(&adapter).await, 8);
    assert_eq!(
        cell(&adapter, by_id(8), "email").await.as_deref(),
        Some("Grace2@Example.com"),
        "an insert must store the case the user typed, never a folded version"
    );
    assert_eq!(
        cell(&adapter, by_id(8), "signup_date").await.as_deref(),
        Some("2026-03-01"),
        "a date value must reach a date column correctly"
    );

    reset_seed().await;
}

// --- UPDATE ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn update_preview_shows_the_records_that_will_change() {
    reset_seed().await;
    let adapter = adapter().await;

    let change = intent(
        Operation::Update,
        by_id(1),
        vec![assignment("active", Value::Boolean(false))],
    );
    let preview = adapter.build_preview(&change).await.unwrap();

    assert_eq!(preview.affected_count(), 1);
    let email_index = preview.columns().iter().position(|c| c == "email").unwrap();
    assert_eq!(
        preview.rows()[0].cells[email_index].as_deref(),
        Some("ada@example.com")
    );

    // Still true: previewing changed nothing.
    assert_eq!(
        cell(&adapter, by_id(1), "active").await.as_deref(),
        Some("true")
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn update_executes_only_the_records_it_previewed() {
    reset_seed().await;
    let adapter = adapter().await;

    let change = intent(
        Operation::Update,
        by_id(1),
        vec![assignment("active", Value::Boolean(false))],
    );
    let preview = adapter.build_preview(&change).await.unwrap();
    let outcome = adapter
        .execute(preview.approve(), &mut MemorySink::default())
        .await
        .unwrap();

    assert_eq!(outcome.rows_affected, 1);
    assert_eq!(
        cell(&adapter, by_id(1), "active").await.as_deref(),
        Some("false")
    );
    assert_eq!(
        cell(&adapter, by_id(2), "active").await.as_deref(),
        Some("true"),
        "no record outside the filter may be touched"
    );

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn update_writes_values_of_every_type_correctly() {
    reset_seed().await;
    let adapter = adapter().await;

    let change = intent(
        Operation::Update,
        by_id(4),
        vec![
            assignment("full_name", Value::Text("KATHERINE Johnson".to_string())),
            assignment("signup_date", Value::Date("2020-12-25".to_string())),
            assignment("active", Value::Boolean(false)),
        ],
    );
    let preview = adapter.build_preview(&change).await.unwrap();
    adapter
        .execute(preview.approve(), &mut MemorySink::default())
        .await
        .unwrap();

    assert_eq!(
        cell(&adapter, by_id(4), "full_name").await.as_deref(),
        Some("KATHERINE Johnson"),
        "the written value keeps its case; only comparisons fold case"
    );
    assert_eq!(
        cell(&adapter, by_id(4), "signup_date").await.as_deref(),
        Some("2020-12-25")
    );
    assert_eq!(
        cell(&adapter, by_id(4), "active").await.as_deref(),
        Some("false")
    );

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_update_found_case_insensitively_updates_the_right_record() {
    reset_seed().await;
    let adapter = adapter().await;

    let change = intent(
        Operation::Update,
        Filter {
            conditions: vec![Condition {
                column: "full_name".to_string(),
                comparison: Comparison::Equals,
                value: Value::Text("ada lovelace".to_string()),
            }],
        },
        vec![assignment("active", Value::Boolean(false))],
    );

    let preview = adapter.build_preview(&change).await.unwrap();
    assert_eq!(preview.affected_count(), 1);
    let outcome = adapter
        .execute(preview.approve(), &mut MemorySink::default())
        .await
        .unwrap();
    assert_eq!(
        outcome.rows_affected, 1,
        "the update must match the same record the preview matched"
    );
    assert_eq!(
        cell(&adapter, by_id(1), "full_name").await.as_deref(),
        Some("Ada Lovelace"),
        "matching case-insensitively must not rewrite the stored case"
    );

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_unfiltered_update_previews_the_whole_table() {
    reset_seed().await;
    let adapter = adapter().await;

    let change = intent(
        Operation::Update,
        Filter::everything(),
        vec![assignment("active", Value::Boolean(false))],
    );
    let preview = adapter.build_preview(&change).await.unwrap();

    assert_eq!(preview.affected_count(), 7);
    assert!(preview.intent().filter.matches_everything());
}

// --- failures stop before anything runs ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_failed_write_preview_yields_no_approval() {
    reset_seed().await;
    let adapter = adapter().await;
    let before = total_users(&adapter).await;

    let broken = intent(
        Operation::Update,
        by_id(1),
        vec![assignment("no_such_column", Value::Integer(1))],
    );

    assert!(matches!(
        adapter.build_preview(&broken).await,
        Err(AdapterError::Query { .. })
    ));
    assert_eq!(total_users(&adapter).await, before);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_rejected_insert_leaves_the_table_untouched() {
    // id 1 already exists, so the primary key refuses this.
    reset_seed().await;
    let adapter = adapter().await;

    let clash = intent(
        Operation::Insert,
        Filter::everything(),
        vec![
            assignment("id", Value::Integer(1)),
            assignment("email", Value::Text("clash@example.com".to_string())),
            assignment("full_name", Value::Text("Clash".to_string())),
            assignment("signup_date", Value::Date("2026-01-01".to_string())),
        ],
    );

    let preview = adapter.build_preview(&clash).await.unwrap();
    let result = adapter
        .execute(preview.approve(), &mut MemorySink::default())
        .await;

    assert!(matches!(result, Err(AdapterError::Query { .. })));
    assert_eq!(
        total_users(&adapter).await,
        7,
        "a refused insert must be all or nothing"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_injection_attempt_in_a_written_value_is_stored_as_text() {
    reset_seed().await;
    let adapter = adapter().await;

    let nasty = "x'); DROP TABLE users; --";
    let change = intent(
        Operation::Update,
        by_id(7),
        vec![assignment("full_name", Value::Text(nasty.to_string()))],
    );

    let preview = adapter.build_preview(&change).await.unwrap();
    adapter
        .execute(preview.approve(), &mut MemorySink::default())
        .await
        .unwrap();

    assert_eq!(
        cell(&adapter, by_id(7), "full_name").await.as_deref(),
        Some(nasty),
        "it should be stored as an ordinary name, not executed"
    );
    assert_eq!(total_users(&adapter).await, 7, "the table still exists");

    reset_seed().await;
}
