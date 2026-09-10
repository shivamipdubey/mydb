//! Typed text to confirmed execution, against real Postgres.
//!
//! Everything between the command bar and the database, with nothing stubbed:
//! the parser, the adapter, the preview, and the confirmation sequence. This
//! is the backend half of the phase 1 exit condition in
//! docs/25-exit-conditions-definition-of-done.md; the other half is running
//! the same command through the interface by hand on macOS, which
//! `tauri-driver` cannot automate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::{PostgresAdapter, PostgresConnectionDetails};
use mydb_adapters::Adapter;
use mydb_confirmation::{begin, Step, WorkflowError};
use mydb_core::Engine;
use mydb_core::MemorySink;

fn details() -> PostgresConnectionDetails<'static> {
    fn var(name: &str, default: &'static str) -> &'static str {
        match std::env::var(name) {
            Ok(value) => Box::leak(value.into_boxed_str()),
            Err(_) => default,
        }
    }
    PostgresConnectionDetails {
        host: var("MYDB_TEST_PG_HOST", "localhost"),
        port: var("MYDB_TEST_PG_PORT", "55432").parse().unwrap(),
        database: var("MYDB_TEST_PG_DATABASE", "mydb_test"),
        username: var("MYDB_TEST_PG_USER", "mydb_test"),
        password: var("MYDB_TEST_PG_PASSWORD", "mydb_test_password"),
    }
}

async fn reset_seed() {
    let d = details();
    let mut config = tokio_postgres::Config::new();
    config
        .host(d.host)
        .port(d.port)
        .dbname(d.database)
        .user(d.username)
        .password(d.password);
    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .batch_execute(include_str!("../../../testing/seed.sql"))
        .await
        .unwrap();
}

async fn connect() -> PostgresAdapter {
    PostgresAdapter::connect(details())
        .await
        .expect("start the test database with ./scripts/db.sh up")
}

/// Runs a read command and reports how many users exist.
async fn user_count(adapter: &PostgresAdapter, schema: &mydb_core::Schema) -> u64 {
    let intent = mydb_parser::parse("show me all users", schema, Engine::Postgres).unwrap();
    match begin(adapter, intent, false).await.unwrap() {
        Step::ReadComplete(records) => records.total_count(),
        Step::AwaitingConfirmation(_) => panic!("a read must not await confirmation"),
    }
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn typed_command_to_confirmed_delete_works_end_to_end() {
    reset_seed().await;
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    assert_eq!(user_count(&adapter, &schema).await, 7);

    // 1. The user types a command in plain language.
    let intent = mydb_parser::parse(
        "delete users where active is false",
        &schema,
        Engine::Postgres,
    )
    .unwrap();
    assert!(intent.is_write());

    // 2. It is previewed, not run.
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, false).await.unwrap() else {
        panic!("a delete must await confirmation");
    };
    assert_eq!(pending.preview().affected_count(), 2);
    assert_eq!(
        pending.description(),
        "Delete records in users where active is false"
    );
    assert_eq!(
        user_count(&adapter, &schema).await,
        7,
        "previewing must not change anything"
    );

    // 3. The user cancels. Still nothing has happened.
    let cancelled = pending.cancel();
    assert_eq!(
        cancelled.summary,
        "Delete records in users where active is false"
    );
    assert_eq!(user_count(&adapter, &schema).await, 7);

    // 4. The user types it again and confirms.
    let intent = mydb_parser::parse(
        "delete users where active is false",
        &schema,
        Engine::Postgres,
    )
    .unwrap();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, false).await.unwrap() else {
        panic!("a delete must await confirmation");
    };
    let completed = pending
        .confirm(&adapter, "", &mut MemorySink::default())
        .await
        .unwrap();

    assert_eq!(completed.outcome.rows_affected, 2);
    assert_eq!(user_count(&adapter, &schema).await, 5);

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn editing_a_command_re_previews_against_the_real_database() {
    reset_seed().await;
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    // A command that would take the whole table.
    let broad = mydb_parser::parse("delete all users", &schema, Engine::Postgres).unwrap();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, broad, false).await.unwrap() else {
        panic!("expected a pending write");
    };
    assert_eq!(pending.preview().affected_count(), 7);
    assert!(pending.intent().filter.matches_everything());

    // The user notices and narrows it. The revised command is previewed again.
    let narrowed = mydb_parser::parse(
        "delete users where active is false",
        &schema,
        Engine::Postgres,
    )
    .unwrap();
    let Step::AwaitingConfirmation(revised) = pending.edit(&adapter, narrowed).await.unwrap()
    else {
        panic!("an edit must return to the preview step");
    };

    assert_eq!(revised.preview().affected_count(), 2);
    assert_eq!(
        user_count(&adapter, &schema).await,
        7,
        "editing must not have run either command"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_command_naming_an_unknown_table_never_reaches_the_database() {
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    let result = mydb_parser::parse("delete every invoice", &schema, Engine::Postgres);

    assert!(
        result.is_err(),
        "an unknown table must be refused before anything is previewed or run"
    );
}

// --- docs/11: the gate holds against a real database, not just a stub ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_production_delete_will_not_run_until_its_gate_is_satisfied() {
    reset_seed().await;
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    let intent = mydb_parser::parse(
        "delete users where active is false",
        &schema,
        Engine::Postgres,
    )
    .unwrap();

    // Flagged production: the extra step applies.
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, true).await.unwrap() else {
        panic!("expected a pending write");
    };
    assert_eq!(pending.preview().affected_count(), 2);

    let refused = pending
        .confirm(&adapter, "", &mut MemorySink::default())
        .await;
    assert!(
        matches!(refused, Err(WorkflowError::ExtraStepNotSatisfied { .. })),
        "an unsatisfied gate must stop the write"
    );
    assert_eq!(
        user_count(&adapter, &schema).await,
        7,
        "nothing may be deleted while the gate is unsatisfied"
    );

    // Typing the previewed count lets it through.
    let intent = mydb_parser::parse(
        "delete users where active is false",
        &schema,
        Engine::Postgres,
    )
    .unwrap();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, true).await.unwrap() else {
        panic!("expected a pending write");
    };
    let completed = pending
        .confirm(&adapter, "2", &mut MemorySink::default())
        .await
        .unwrap();
    assert_eq!(completed.outcome.rows_affected, 2);
    assert_eq!(user_count(&adapter, &schema).await, 5);

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_production_drop_is_gated_on_the_table_name() {
    reset_seed().await;
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    let intent = mydb_parser::parse("drop table disposable", &schema, Engine::Postgres).unwrap();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, true).await.unwrap() else {
        panic!("expected a pending write");
    };

    // The row count is not a way past a schema change's gate.
    assert!(pending
        .confirm(&adapter, "3", &mut MemorySink::default())
        .await
        .is_err());
    assert!(
        adapter
            .describe_schema()
            .await
            .unwrap()
            .find_table("disposable")
            .is_some(),
        "the table must still be there"
    );

    let intent = mydb_parser::parse("drop table disposable", &schema, Engine::Postgres).unwrap();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, true).await.unwrap() else {
        panic!("expected a pending write");
    };
    pending
        .confirm(&adapter, "disposable", &mut MemorySink::default())
        .await
        .unwrap();
    assert!(adapter
        .describe_schema()
        .await
        .unwrap()
        .find_table("disposable")
        .is_none());

    reset_seed().await;
}
