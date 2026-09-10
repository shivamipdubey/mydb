//! Before and after state, captured inside each write's own transaction
//! (docs/07-audit-log-and-recovery-bin.md).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::PostgresAdapter;
use mydb_adapters::{Adapter, AdapterError};
use mydb_core::{
    Assignment, Comparison, Condition, Engine, Filter, Intent, Operation, StateSnapshot, Value,
};

mod support;
use support::{details_from_env, reset_seed};

async fn adapter() -> PostgresAdapter {
    PostgresAdapter::connect(details_from_env())
        .await
        .expect("start the test database with ./scripts/db.sh up")
}

fn on(table: &str, operation: Operation, filter: Filter, assignments: Vec<Assignment>) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: table.to_string(),
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

async fn run(adapter: &PostgresAdapter, intent: Intent) -> mydb_adapters::ExecutionOutcome {
    let preview = adapter.build_preview(&intent).await.unwrap();
    adapter.execute(preview.approve()).await.unwrap()
}

// --- a delete: the records existed, and afterwards they do not ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_delete_captures_the_records_it_removed() {
    reset_seed().await;
    let adapter = adapter().await;

    let outcome = run(
        &adapter,
        on("users", Operation::Delete, by_id(3), Vec::new()),
    )
    .await;

    assert_eq!(outcome.rows_affected, 1);
    assert_eq!(outcome.before.total(), 1);

    let record = &outcome.before.records()[0];
    assert_eq!(record.get("id").unwrap(), &serde_json::json!(3));
    assert_eq!(
        record.get("email").unwrap(),
        &serde_json::json!("alan@example.com")
    );
    // The after-state is empty because the record is gone, which is the
    // truth rather than a gap in the capture.
    assert_eq!(outcome.after, Some(StateSnapshot::empty()));

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn captured_values_keep_their_types_rather_than_becoming_text() {
    // The preview casts everything to text for display. A capture must not:
    // it is the only record of data that no longer exists.
    reset_seed().await;
    let adapter = adapter().await;

    let outcome = run(
        &adapter,
        on("users", Operation::Delete, by_id(3), Vec::new()),
    )
    .await;

    let record = &outcome.before.records()[0];
    assert!(
        record.get("id").unwrap().is_number(),
        "an integer stays a number"
    );
    assert!(
        record.get("active").unwrap().is_boolean(),
        "a boolean stays a boolean"
    );
    assert!(
        record.get("note").is_none() || record.get("note").unwrap().is_null(),
        "a null stays a null"
    );

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_null_column_is_captured_as_null_not_as_an_empty_string() {
    reset_seed().await;
    let adapter = adapter().await;

    // disposable.note is null for row 1.
    let outcome = run(
        &adapter,
        on("disposable", Operation::Delete, by_id(1), Vec::new()),
    )
    .await;

    let record = &outcome.before.records()[0];
    assert!(record.get("note").unwrap().is_null());

    reset_seed().await;
}

// --- an insert: nothing existed, and afterwards the record does ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_insert_captures_the_record_as_it_actually_landed() {
    reset_seed().await;
    let adapter = adapter().await;

    let outcome = run(
        &adapter,
        on(
            "disposable",
            Operation::Insert,
            Filter::everything(),
            vec![
                Assignment {
                    column: "id".to_string(),
                    value: Value::Integer(9),
                },
                Assignment {
                    column: "label".to_string(),
                    value: Value::Text("Ninth".to_string()),
                },
            ],
        ),
    )
    .await;

    assert_eq!(outcome.rows_affected, 1);
    assert!(
        outcome.before.is_empty(),
        "nothing existed before, which is empty rather than unavailable"
    );

    let after = outcome.after.unwrap();
    assert_eq!(after.total(), 1);
    let record = &after.records()[0];
    assert_eq!(record.get("label").unwrap(), &serde_json::json!("Ninth"));
    assert!(
        record.get("note").unwrap().is_null(),
        "the column the command never named is captured as it landed"
    );

    reset_seed().await;
}

// --- an update: both sides, matched by identity rather than by filter ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_update_captures_both_sides_even_when_the_filter_no_longer_matches() {
    // "set active to false where active is true" matches nothing once it has
    // run. Re-reading by filter would record that the rows had disappeared.
    reset_seed().await;
    let adapter = adapter().await;

    let outcome = run(
        &adapter,
        on(
            "users",
            Operation::Update,
            Filter {
                conditions: vec![Condition {
                    column: "active".to_string(),
                    comparison: Comparison::Equals,
                    value: Value::Boolean(true),
                }],
            },
            vec![Assignment {
                column: "active".to_string(),
                value: Value::Boolean(false),
            }],
        ),
    )
    .await;

    assert_eq!(outcome.rows_affected, 5);
    assert_eq!(outcome.before.total(), 5);
    assert!(outcome
        .before
        .records()
        .iter()
        .all(|record| record.get("active").unwrap() == &serde_json::json!(true)));

    let after = outcome
        .after
        .expect("users has a single-column primary key");
    assert_eq!(
        after.total(),
        5,
        "the same records must be found again by identity, not by filter"
    );
    assert!(after
        .records()
        .iter()
        .all(|record| record.get("active").unwrap() == &serde_json::json!(false)));

    reset_seed().await;
}

// --- a schema change: everything in the table ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_truncate_captures_every_record_in_the_table() {
    reset_seed().await;
    let adapter = adapter().await;

    let outcome = run(
        &adapter,
        on(
            "disposable",
            Operation::Truncate,
            Filter::everything(),
            Vec::new(),
        ),
    )
    .await;

    assert_eq!(outcome.before.total(), 3);
    assert_eq!(outcome.after, Some(StateSnapshot::empty()));

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_drop_captures_every_record_before_the_table_goes() {
    reset_seed().await;
    let adapter = adapter().await;

    let outcome = run(
        &adapter,
        on(
            "disposable",
            Operation::DropTable,
            Filter::everything(),
            Vec::new(),
        ),
    )
    .await;

    assert_eq!(outcome.before.total(), 3);
    assert!(adapter
        .describe_schema()
        .await
        .unwrap()
        .find_table("disposable")
        .is_none());

    reset_seed().await;
}

// --- a refused write captures nothing, because nothing happened ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_write_the_database_refuses_captures_nothing() {
    reset_seed().await;
    let adapter = adapter().await;

    // Deleting user 1 orphans rows in orders, so the transaction rolls back.
    let intent = on("users", Operation::Delete, by_id(1), Vec::new());
    let preview = adapter.build_preview(&intent).await.unwrap();
    let result = adapter.execute(preview.approve()).await;

    assert!(matches!(result, Err(AdapterError::Query { .. })));

    // And the row is still there: the capture was inside the transaction and
    // went back with it.
    let still_there = adapter
        .run_read(&on("users", Operation::Read, by_id(1), Vec::new()))
        .await
        .unwrap();
    assert_eq!(still_there.total_count(), 1);

    reset_seed().await;
}
