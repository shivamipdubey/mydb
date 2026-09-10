//! What the command history does and does not record, driven end to end.
//!
//! This walks the same path the desktop shell's `confirm_command` walks:
//! parse, begin, preview, then confirm or cancel, then record the attempt
//! through the same `record_intent` the app uses. It exists because there is
//! no history viewer screen in phase 1 by design, so the only way to check
//! the history's behaviour is to read the file.
//!
//! It writes to a temporary file rather than the real one, so running the
//! suite never pollutes a user's own history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::{PostgresAdapter, PostgresConnectionDetails};
use mydb_adapters::Adapter;
use mydb_confirmation::{begin, Step};
use mydb_core::{Engine, MemorySink};
use mydb_storage::{CommandHistory, Outcome};

const CONNECTION: &str = "Local test database";

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

/// Runs a command exactly as the shell does, and records the attempt.
///
/// `authorization` is what the user typed into the production extra step.
/// Returns whether anything was recorded.
async fn run_and_record(
    adapter: &PostgresAdapter,
    history: &CommandHistory,
    text: &str,
    production: bool,
    authorization: &str,
) -> Result<bool, String> {
    let schema = adapter.describe_schema().await.unwrap();
    let intent = mydb_parser::parse(text, &schema, Engine::Postgres).map_err(|e| e.to_string())?;

    match begin(adapter, intent.clone(), production)
        .await
        .map_err(|e| e.to_string())?
    {
        // A read runs directly and is not a write, so nothing is recorded.
        Step::ReadComplete(_) => Ok(false),
        Step::AwaitingConfirmation(pending) => {
            let attempt = pending
                .confirm(adapter, authorization, &mut MemorySink::default())
                .await;
            let outcome = match &attempt {
                Ok(_) => Outcome::Success,
                Err(_) => Outcome::Failure,
            };
            // After the attempt, never before (docs/16 item 7).
            history.record_intent(CONNECTION, &intent, outcome).unwrap();
            Ok(true)
        }
    }
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn the_history_records_writes_and_only_writes() {
    reset_seed().await;
    let adapter = connect().await;
    let dir = tempfile::tempdir().unwrap();
    let history = CommandHistory::at(dir.path().join("command-history.jsonl"));

    // 1. A confirmed delete is recorded as a success.
    run_and_record(
        &adapter,
        &history,
        "delete users where active is false",
        false,
        "",
    )
    .await
    .unwrap();
    assert_eq!(history.len().unwrap(), 1);
    assert_eq!(history.read_all().unwrap()[0].operation, "delete");
    assert_eq!(history.read_all().unwrap()[0].result, Outcome::Success);

    // 2. A confirmed drop is recorded as a drop, not a delete.
    run_and_record(
        &adapter,
        &history,
        "drop table disposable",
        true,
        "disposable",
    )
    .await
    .unwrap();
    let entries = history.read_all().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[1].operation, "drop_table",
        "a drop must not be filed as a delete"
    );

    // 3. A cancelled command adds nothing.
    reset_seed().await;
    let schema = adapter.describe_schema().await.unwrap();
    let intent = mydb_parser::parse(
        "delete users where active is false",
        &schema,
        Engine::Postgres,
    )
    .unwrap();
    let Step::AwaitingConfirmation(pending) = begin(&adapter, intent, false).await.unwrap() else {
        panic!("expected a pending write");
    };
    pending.cancel();
    assert_eq!(
        history.len().unwrap(),
        2,
        "cancelling must leave no trace beyond what was already there"
    );

    // 4. A command that never reaches the database adds nothing.
    let refused = run_and_record(&adapter, &history, "delete every invoice", false, "").await;
    assert!(refused.is_err());
    assert_eq!(history.len().unwrap(), 2);

    // 5. A write the database refuses is recorded, as a failure.
    let attempted = run_and_record(
        &adapter,
        &history,
        "delete users where signup date is before 2024",
        false,
        "",
    )
    .await
    .unwrap();
    assert!(attempted, "the write was attempted, so it is recorded");
    let entries = history.read_all().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[2].result,
        Outcome::Failure,
        "a refused write must be recorded as a failure, never as a success"
    );

    // 6. A read adds nothing.
    let recorded = run_and_record(&adapter, &history, "show me all users", false, "")
        .await
        .unwrap();
    assert!(!recorded);
    assert_eq!(
        history.len().unwrap(),
        3,
        "phase 1 records writes only, and a read changes nothing"
    );

    reset_seed().await;
}
