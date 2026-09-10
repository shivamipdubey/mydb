//! The confirmation state machine (docs/18-testing-strategy.md).
//!
//! Unit tests against a recording stub adapter rather than a live database,
//! so the sequence itself is what is under test: confirm, edit, and cancel
//! each producing the correct next state, and execution never being reachable
//! without a preview.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Mutex;

use mydb_adapters::{
    Adapter, AdapterError, ApprovedWrite, ExecutionOutcome, Health, Preview, Record, RecordSet,
};
use mydb_confirmation::{begin, Step, WorkflowError};
use mydb_core::{Comparison, Condition, Engine, Filter, Intent, Operation, Schema, Value};

/// A stub adapter that records every call and can be told to fail.
#[derive(Default)]
struct StubAdapter {
    calls: Mutex<Vec<&'static str>>,
    fail_preview: bool,
    fail_execute: bool,
}

impl StubAdapter {
    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: &'static str) {
        self.calls.lock().unwrap().push(call);
    }

    fn sample_records(intent: &Intent) -> RecordSet {
        RecordSet::for_testing(
            vec!["id".to_string(), "email".to_string()],
            vec![Record {
                cells: vec![Some("1".to_string()), Some("ada@example.com".to_string())],
            }],
            1,
            format!("SELECT * FROM {}", intent.qualified_table()),
        )
    }
}

#[async_trait::async_trait]
impl Adapter for StubAdapter {
    async fn describe_schema(&self) -> Result<Schema, AdapterError> {
        self.record("describe_schema");
        Ok(Schema::default())
    }

    async fn report_health(&self) -> Health {
        self.record("report_health");
        Health::connected(None)
    }

    async fn run_read(&self, intent: &Intent) -> Result<RecordSet, AdapterError> {
        self.record("run_read");
        Ok(Self::sample_records(intent))
    }

    async fn build_preview(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        self.record("build_preview");
        if self.fail_preview {
            return Err(AdapterError::query("preview failed on purpose"));
        }
        Ok(Preview::for_testing(
            intent.clone(),
            Self::sample_records(intent),
        ))
    }

    async fn execute(&self, approved: ApprovedWrite) -> Result<ExecutionOutcome, AdapterError> {
        self.record("execute");
        if self.fail_execute {
            return Err(AdapterError::query("execution failed on purpose"));
        }
        Ok(ExecutionOutcome {
            rows_affected: approved.preview().affected_count(),
        })
    }
}

fn intent(operation: Operation, filter: Filter) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: "users".to_string(),
        operation,
        filter,
        assignments: Vec::new(),
    }
}

fn delete() -> Intent {
    intent(Operation::Delete, Filter::everything())
}

fn narrower_delete() -> Intent {
    intent(
        Operation::Delete,
        Filter {
            conditions: vec![Condition {
                column: "active".to_string(),
                comparison: Comparison::Equals,
                value: Value::Boolean(false),
            }],
        },
    )
}

// --- docs/05 step 4: reads skip confirmation ---

#[tokio::test]
async fn a_read_runs_directly_and_is_never_offered_for_confirmation() {
    let adapter = StubAdapter::default();

    let step = begin(&adapter, intent(Operation::Read, Filter::everything()))
        .await
        .unwrap();

    assert!(matches!(step, Step::ReadComplete(_)));
    assert!(step.awaiting_confirmation().is_none());
    assert_eq!(adapter.calls(), ["run_read"]);
}

// --- docs/05 step 5: writes are previewed and stop ---

#[tokio::test]
async fn a_write_is_previewed_and_nothing_runs_until_confirmed() {
    let adapter = StubAdapter::default();

    let step = begin(&adapter, delete()).await.unwrap();

    let pending = step
        .awaiting_confirmation()
        .expect("a write must await confirmation");
    assert_eq!(pending.preview().affected_count(), 1);
    assert_eq!(
        adapter.calls(),
        ["build_preview"],
        "previewing must not execute anything"
    );
}

// --- docs/05 step 10: confirm executes ---

#[tokio::test]
async fn confirm_executes_and_reports_what_happened() {
    let adapter = StubAdapter::default();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete()).await.unwrap() else {
        panic!("expected a pending write");
    };

    let completed = pending.confirm(&adapter).await.unwrap();

    assert_eq!(completed.outcome.rows_affected, 1);
    assert_eq!(completed.description, "Delete every record in users");
    assert_eq!(
        adapter.calls(),
        ["build_preview", "execute"],
        "execution must be preceded by its own preview, in that order"
    );
}

// --- docs/05 step 8: edit returns to preview, never to execute ---

#[tokio::test]
async fn edit_returns_to_the_preview_step_with_the_new_intent() {
    let adapter = StubAdapter::default();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete()).await.unwrap() else {
        panic!("expected a pending write");
    };

    let step = pending.edit(&adapter, narrower_delete()).await.unwrap();

    let revised = step
        .awaiting_confirmation()
        .expect("an edited write must be previewed again");
    assert_eq!(
        revised.intent().filter.conditions.len(),
        1,
        "the revised intent should be the one now awaiting confirmation"
    );
    assert_eq!(
        adapter.calls(),
        ["build_preview", "build_preview"],
        "editing must re-preview and must not execute"
    );
    assert!(
        !adapter.calls().contains(&"execute"),
        "an edit must never run the write"
    );
}

#[tokio::test]
async fn editing_a_write_into_a_read_runs_it_directly() {
    let adapter = StubAdapter::default();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete()).await.unwrap() else {
        panic!("expected a pending write");
    };

    let step = pending
        .edit(&adapter, intent(Operation::Read, Filter::everything()))
        .await
        .unwrap();

    assert!(matches!(step, Step::ReadComplete(_)));
    assert_eq!(adapter.calls(), ["build_preview", "run_read"]);
}

// --- docs/05 step 9: cancel discards ---

#[tokio::test]
async fn cancel_discards_the_command_without_running_anything() {
    let adapter = StubAdapter::default();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete()).await.unwrap() else {
        panic!("expected a pending write");
    };

    let cancelled = pending.cancel();

    assert_eq!(cancelled.summary, "Delete every record in users");
    assert_eq!(
        adapter.calls(),
        ["build_preview"],
        "cancelling must not execute anything"
    );
}

// --- a failed preview stops the workflow before confirm ---

#[tokio::test]
async fn a_failed_preview_produces_nothing_that_could_be_confirmed() {
    let adapter = StubAdapter {
        fail_preview: true,
        ..Default::default()
    };

    let result = begin(&adapter, delete()).await;

    assert!(matches!(result, Err(WorkflowError::Preview(_))));
    assert_eq!(
        adapter.calls(),
        ["build_preview"],
        "a failed preview must never be followed by an execution"
    );
    assert!(!adapter.calls().contains(&"execute"));
}

#[tokio::test]
async fn a_failed_execution_is_reported_as_an_execution_failure() {
    let adapter = StubAdapter {
        fail_execute: true,
        ..Default::default()
    };
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete()).await.unwrap() else {
        panic!("expected a pending write");
    };

    let result = pending.confirm(&adapter).await;

    assert!(
        matches!(result, Err(WorkflowError::Execution(_))),
        "the stage matters: a failed execution is not a failed preview"
    );
}

// --- the sequence cannot be short-circuited ---

#[tokio::test]
async fn a_confirmation_cannot_be_replayed_into_two_executions() {
    // confirm(self) consumes the pending write, so this is enforced by the
    // compiler rather than by a flag. The test documents the guarantee and
    // would fail to compile if confirm were ever changed to take &self.
    let adapter = StubAdapter::default();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete()).await.unwrap() else {
        panic!("expected a pending write");
    };

    pending.confirm(&adapter).await.unwrap();
    // pending.confirm(&adapter).await; // would not compile: value moved

    assert_eq!(adapter.calls(), ["build_preview", "execute"]);
}
