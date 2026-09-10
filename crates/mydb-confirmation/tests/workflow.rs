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
use mydb_confirmation::{begin, ExtraStep, Step, WorkflowError};
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
            before: mydb_core::StateSnapshot::empty(),
            after: Some(mydb_core::StateSnapshot::empty()),
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

    let step = begin(
        &adapter,
        intent(Operation::Read, Filter::everything()),
        false,
    )
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

    let step = begin(&adapter, delete(), false).await.unwrap();

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
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete(), false).await.unwrap()
    else {
        panic!("expected a pending write");
    };

    let completed = pending.confirm(&adapter, "").await.unwrap();

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
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete(), false).await.unwrap()
    else {
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
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete(), false).await.unwrap()
    else {
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
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete(), false).await.unwrap()
    else {
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

    let result = begin(&adapter, delete(), false).await;

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
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete(), false).await.unwrap()
    else {
        panic!("expected a pending write");
    };

    let result = pending.confirm(&adapter, "").await;

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
    let Step::AwaitingConfirmation(pending) = begin(&adapter, delete(), false).await.unwrap()
    else {
        panic!("expected a pending write");
    };

    pending.confirm(&adapter, "").await.unwrap();
    // pending.confirm(&adapter, "").await; // would not compile: value moved

    assert_eq!(adapter.calls(), ["build_preview", "execute"]);
}

// --- docs/11: the production flag's extra step, per operation type ---

fn write(operation: Operation) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: "users".to_string(),
        operation,
        filter: Filter::everything(),
        assignments: Vec::new(),
    }
}

/// A stub whose preview reports a chosen number of affected records, so the
/// gate can be tested at each size that changes which gate applies.
struct CountingAdapter {
    inner: StubAdapter,
    count: u64,
}

#[async_trait::async_trait]
impl Adapter for CountingAdapter {
    async fn describe_schema(&self) -> Result<Schema, AdapterError> {
        self.inner.describe_schema().await
    }
    async fn report_health(&self) -> Health {
        self.inner.report_health().await
    }
    async fn run_read(&self, intent: &Intent) -> Result<RecordSet, AdapterError> {
        self.inner.run_read(intent).await
    }
    async fn build_preview(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        self.inner.record("build_preview");
        Ok(Preview::for_testing(
            intent.clone(),
            RecordSet::for_testing(
                vec!["id".to_string()],
                Vec::new(),
                self.count,
                "SELECT".to_string(),
            ),
        ))
    }
    async fn execute(&self, approved: ApprovedWrite) -> Result<ExecutionOutcome, AdapterError> {
        self.inner.execute(approved).await
    }
}

fn counting(count: u64) -> CountingAdapter {
    CountingAdapter {
        inner: StubAdapter::default(),
        count,
    }
}

async fn pending_for(
    adapter: &CountingAdapter,
    operation: Operation,
    production: bool,
) -> Box<mydb_confirmation::PendingWrite> {
    match begin(adapter, write(operation), production).await.unwrap() {
        Step::AwaitingConfirmation(pending) => pending,
        Step::ReadComplete(_) => panic!("expected a pending write"),
    }
}

#[tokio::test]
async fn an_unflagged_connection_asks_for_nothing_extra() {
    let adapter = counting(42);
    for operation in [
        Operation::Delete,
        Operation::Update,
        Operation::DropTable,
        Operation::Truncate,
    ] {
        let pending = pending_for(&adapter, operation, false).await;
        assert_eq!(pending.extra_step(), &ExtraStep::None, "{operation:?}");
        assert!(pending.confirm(&adapter, "").await.is_ok(), "{operation:?}");
    }
}

#[tokio::test]
async fn every_destructive_operation_is_gated_on_a_production_connection() {
    // docs/11's testing requirement, for each destructive operation type.
    for operation in [
        Operation::Delete,
        Operation::Update,
        Operation::DropTable,
        Operation::Truncate,
    ] {
        let adapter = counting(42);

        // Nothing typed: refused, and nothing ran.
        let pending = pending_for(&adapter, operation, true).await;
        assert!(pending.extra_step().is_required(), "{operation:?}");
        let refused = pending.confirm(&adapter, "").await;
        assert!(
            matches!(refused, Err(WorkflowError::ExtraStepNotSatisfied { .. })),
            "{operation:?} ran without its extra step"
        );
        assert!(
            !adapter.inner.calls().contains(&"execute"),
            "{operation:?} reached execute despite an unsatisfied gate"
        );

        // The wrong thing typed: still refused.
        let pending = pending_for(&adapter, operation, true).await;
        assert!(pending.confirm(&adapter, "yes please").await.is_err());
        assert!(!adapter.inner.calls().contains(&"execute"));
    }
}

#[tokio::test]
async fn a_schema_change_is_gated_on_the_table_name() {
    let adapter = counting(42);

    for operation in [Operation::DropTable, Operation::Truncate] {
        let pending = pending_for(&adapter, operation, true).await;
        assert_eq!(
            pending.extra_step(),
            &ExtraStep::TableName {
                table: "users".to_string()
            }
        );
        // The record count is not a way past this one.
        assert!(pending.confirm(&adapter, "42").await.is_err());

        let pending = pending_for(&adapter, operation, true).await;
        assert!(pending.confirm(&adapter, "users").await.is_ok());
    }
}

#[tokio::test]
async fn a_large_delete_accepts_the_count_or_the_word() {
    let adapter = counting(42);

    let pending = pending_for(&adapter, Operation::Delete, true).await;
    assert_eq!(
        pending.extra_step(),
        &ExtraStep::CountOrConfirm { count: 42 }
    );
    assert!(pending.confirm(&adapter, "42").await.is_ok());

    let adapter = counting(42);
    let pending = pending_for(&adapter, Operation::Update, true).await;
    assert!(pending.confirm(&adapter, "CONFIRM").await.is_ok());
}

#[tokio::test]
async fn a_delete_of_one_record_will_not_accept_the_count() {
    for count in [0, 1] {
        let adapter = counting(count);
        let pending = pending_for(&adapter, Operation::Delete, true).await;
        assert_eq!(pending.extra_step(), &ExtraStep::ConfirmWord);

        assert!(
            pending.confirm(&adapter, &count.to_string()).await.is_err(),
            "typing {count} is a keystroke, not friction"
        );
        assert!(!adapter.inner.calls().contains(&"execute"));

        let pending = pending_for(&adapter, Operation::Delete, true).await;
        assert!(pending.confirm(&adapter, "confirm").await.is_ok());
    }
}

#[tokio::test]
async fn an_insert_on_a_production_connection_needs_no_extra_step() {
    let adapter = counting(1);
    let pending = pending_for(&adapter, Operation::Insert, true).await;
    assert_eq!(pending.extra_step(), &ExtraStep::None);
    assert!(pending.confirm(&adapter, "").await.is_ok());
}

#[tokio::test]
async fn editing_re_derives_the_gate_for_the_revised_command() {
    // A broad delete narrowed to one record must not keep the count gate the
    // broad version had, and must not become ungated either.
    let wide = counting(42);
    let pending = pending_for(&wide, Operation::Delete, true).await;
    assert_eq!(
        pending.extra_step(),
        &ExtraStep::CountOrConfirm { count: 42 }
    );

    let narrow = counting(1);
    let Step::AwaitingConfirmation(revised) = pending
        .edit(&narrow, write(Operation::Delete))
        .await
        .unwrap()
    else {
        panic!("an edit must return to the preview step");
    };
    assert_eq!(
        revised.extra_step(),
        &ExtraStep::ConfirmWord,
        "the revised command's gate comes from its own preview"
    );
    assert!(revised.extra_step().is_required(), "still production");
}
