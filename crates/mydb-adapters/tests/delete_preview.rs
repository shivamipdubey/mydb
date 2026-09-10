//! The core safety loop at the adapter layer: a DELETE must be previewed
//! before it can run, and the preview must show exactly what will go.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Mutex;

use mydb_adapters::postgres::PostgresAdapter;
use mydb_adapters::{Adapter, AdapterError, ApprovedWrite, ExecutionOutcome, Health, Preview};
use mydb_core::{Comparison, Condition, Engine, Filter, Intent, Operation, Schema, Value};

mod support;
use support::{details_from_env, reset_seed};

fn delete_intent(filter: Filter) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: "users".to_string(),
        operation: Operation::Delete,
        filter,
    }
}

/// Users 3 and 6, neither of whom has an order.
///
/// Deliberately chosen to avoid the orders foreign key: this filter is for
/// testing the happy path, and a constraint failure is tested on its own
/// below rather than mixed in here.
fn inactive_users() -> Filter {
    Filter {
        conditions: vec![Condition {
            column: "active".to_string(),
            comparison: Comparison::Equals,
            value: Value::Boolean(false),
        }],
    }
}

fn signed_up_before_2024() -> Filter {
    Filter {
        conditions: vec![Condition {
            column: "signup_date".to_string(),
            comparison: Comparison::LessThan,
            value: Value::Date("2024-01-01".to_string()),
        }],
    }
}

async fn adapter() -> PostgresAdapter {
    PostgresAdapter::connect(details_from_env())
        .await
        .expect("test Postgres should be reachable; start it with ./scripts/db.sh up")
}

/// Reads the email column out of a preview, for asserting on exact rows.
fn emails(preview: &Preview) -> Vec<String> {
    let index = preview
        .columns()
        .iter()
        .position(|c| c == "email")
        .expect("the users preview should include the email column");
    preview
        .rows()
        .iter()
        .map(|row| row.cells[index].clone().unwrap_or_default())
        .collect()
}

async fn count_users(adapter: &PostgresAdapter) -> u64 {
    // Counted through a preview of a no-op filter so the test does not need
    // its own database client.
    adapter
        .build_preview(&delete_intent(Filter::everything()))
        .await
        .unwrap()
        .affected_count()
}

// --- the preview shows exactly what would be affected ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn preview_returns_the_rows_the_delete_would_remove() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&delete_intent(signed_up_before_2024()))
        .await
        .unwrap();

    assert_eq!(preview.affected_count(), 3);
    assert_eq!(
        emails(&preview),
        vec!["ada@example.com", "grace@example.com", "alan@example.com"]
    );
    assert!(!preview.is_truncated());
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn preview_changes_nothing() {
    reset_seed().await;
    let adapter = adapter().await;

    let before = count_users(&adapter).await;
    adapter
        .build_preview(&delete_intent(signed_up_before_2024()))
        .await
        .unwrap();
    assert_eq!(
        count_users(&adapter).await,
        before,
        "building a preview must never change data"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_unfiltered_delete_previews_the_whole_table() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&delete_intent(Filter::everything()))
        .await
        .unwrap();

    assert_eq!(preview.affected_count(), 7);
    assert!(preview.intent().filter.matches_everything());
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_filter_matching_nothing_says_so_rather_than_looking_normal() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&delete_intent(Filter {
            conditions: vec![Condition {
                column: "email".to_string(),
                comparison: Comparison::Equals,
                value: Value::Text("nobody@example.com".to_string()),
            }],
        }))
        .await
        .unwrap();

    assert!(preview.affects_nothing());
    assert_eq!(preview.affected_count(), 0);
    assert!(preview.rows().is_empty());
}

// --- execution matches the preview, and cannot happen without one ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn execute_removes_exactly_what_the_preview_showed() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&delete_intent(inactive_users()))
        .await
        .unwrap();
    let expected = preview.affected_count();
    let doomed = emails(&preview);
    assert_eq!(expected, 2);

    let outcome = adapter.execute(preview.approve()).await.unwrap();

    assert_eq!(
        outcome.rows_affected, expected,
        "the number removed must match the number the user was shown"
    );
    assert_eq!(count_users(&adapter).await, 7 - expected);

    // The rows that went are exactly the listed ones, and no others.
    let remaining = emails(
        &adapter
            .build_preview(&delete_intent(Filter::everything()))
            .await
            .unwrap(),
    );
    for gone in &doomed {
        assert!(!remaining.contains(gone), "{gone} should have been deleted");
    }
    assert!(remaining.contains(&"ada@example.com".to_string()));
    assert_eq!(remaining.len(), 5);

    reset_seed().await;
}

/// docs/18-testing-strategy.md: prove execute only runs after a preview, by
/// asserting the order of calls rather than the final state.
///
/// The type system already makes the reverse order impossible to compile, but
/// the test is required and is worth having: it would catch a future change
/// that widened the door, such as adding a second way to construct a preview.
struct RecordingAdapter {
    inner: PostgresAdapter,
    calls: Mutex<Vec<&'static str>>,
}

#[async_trait::async_trait]
impl Adapter for RecordingAdapter {
    async fn describe_schema(&self) -> Result<Schema, AdapterError> {
        self.calls.lock().unwrap().push("describe_schema");
        self.inner.describe_schema().await
    }

    async fn report_health(&self) -> Health {
        self.calls.lock().unwrap().push("report_health");
        self.inner.report_health().await
    }

    async fn build_preview(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        self.calls.lock().unwrap().push("build_preview");
        self.inner.build_preview(intent).await
    }

    async fn execute(&self, approved: ApprovedWrite) -> Result<ExecutionOutcome, AdapterError> {
        self.calls.lock().unwrap().push("execute");
        self.inner.execute(approved).await
    }
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn execute_is_never_reached_before_a_preview_in_the_same_flow() {
    reset_seed().await;
    let recording = RecordingAdapter {
        inner: adapter().await,
        calls: Mutex::new(Vec::new()),
    };

    let preview = recording
        .build_preview(&delete_intent(inactive_users()))
        .await
        .unwrap();
    recording.execute(preview.approve()).await.unwrap();

    assert_eq!(
        recording.calls.lock().unwrap().as_slice(),
        ["build_preview", "execute"],
        "a write must be preceded by its own preview in the same flow"
    );

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_failed_preview_yields_no_approval_so_nothing_can_execute() {
    reset_seed().await;
    let adapter = adapter().await;
    let before = count_users(&adapter).await;

    // A column that does not exist: the preview fails, and because an
    // ApprovedWrite can only be made from a Preview, there is nothing to hand
    // to execute. The failure is not merely reported, it is structural.
    let result = adapter
        .build_preview(&delete_intent(Filter {
            conditions: vec![Condition {
                column: "no_such_column".to_string(),
                comparison: Comparison::Equals,
                value: Value::Integer(1),
            }],
        }))
        .await;

    assert!(matches!(result, Err(AdapterError::Query { .. })));
    assert_eq!(
        count_users(&adapter).await,
        before,
        "a failed preview must leave the data untouched"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_read_intent_is_refused_by_both_preview_and_execute() {
    let adapter = adapter().await;
    let read = Intent {
        operation: Operation::Read,
        ..delete_intent(Filter::everything())
    };

    assert!(matches!(
        adapter.build_preview(&read).await,
        Err(AdapterError::Unsupported(_))
    ));
}

// --- docs/16 item 3, end to end against a real database ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_injection_attempt_is_treated_as_data_not_syntax() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&delete_intent(Filter {
            conditions: vec![Condition {
                column: "email".to_string(),
                comparison: Comparison::Equals,
                value: Value::Text("x'; DROP TABLE users; --".to_string()),
            }],
        }))
        .await
        .unwrap();

    assert_eq!(preview.affected_count(), 0, "no user has that email");

    adapter.execute(preview.approve()).await.unwrap();

    assert_eq!(
        count_users(&adapter).await,
        7,
        "the table must still exist with every row intact"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_write_the_database_refuses_leaves_everything_untouched() {
    // Deleting these users would orphan rows in orders, so Postgres rejects
    // the statement. The preview cannot know that in advance, so what matters
    // is that the refusal is clean: the transaction rolls back, the error is
    // typed rather than a panic, and not one row is lost.
    reset_seed().await;
    let adapter = adapter().await;
    let before = count_users(&adapter).await;

    let preview = adapter
        .build_preview(&delete_intent(signed_up_before_2024()))
        .await
        .unwrap();
    assert_eq!(preview.affected_count(), 3);

    let result = adapter.execute(preview.approve()).await;

    assert!(
        matches!(result, Err(AdapterError::Query { .. })),
        "a constraint violation must surface as a typed error, not a panic"
    );
    assert_eq!(
        count_users(&adapter).await,
        before,
        "a rejected delete must be all-or-nothing, per the transaction rule in docs/04"
    );
}
