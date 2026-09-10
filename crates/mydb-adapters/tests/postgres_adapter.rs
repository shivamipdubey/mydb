//! Integration tests for the Postgres adapter (docs/18-testing-strategy.md).
//!
//! Run with `npm run test:integration` against the instance from
//! `docker-compose.yml`.

// Test code may panic; the workspace lints keep panics out of the application,
// not out of assertions (docs/17-coding-standards.md).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::{PostgresAdapter, PostgresConnectionDetails};
use mydb_adapters::{Adapter, AdapterError};

/// Where the test instance lives.
///
/// Defaults match `docker-compose.yml`. CI overrides the port because a
/// GitHub service container publishes on 5432 rather than the non-default port
/// local development uses to avoid colliding with a real Postgres.
fn details_from_env() -> PostgresConnectionDetails<'static> {
    fn var(name: &str, default: &'static str) -> &'static str {
        match std::env::var(name) {
            // Leaked deliberately: these live for the whole test process, and
            // a test harness is the one place that is unambiguously fine.
            Ok(value) => Box::leak(value.into_boxed_str()),
            Err(_) => default,
        }
    }

    PostgresConnectionDetails {
        host: var("MYDB_TEST_PG_HOST", "localhost"),
        port: var("MYDB_TEST_PG_PORT", "55432")
            .parse()
            .expect("MYDB_TEST_PG_PORT must be a port number"),
        database: var("MYDB_TEST_PG_DATABASE", "mydb_test"),
        username: var("MYDB_TEST_PG_USER", "mydb_test"),
        password: var("MYDB_TEST_PG_PASSWORD", "mydb_test_password"),
    }
}

async fn connect() -> PostgresAdapter {
    PostgresAdapter::connect(details_from_env())
        .await
        .expect("test Postgres should be reachable; start it with ./scripts/db.sh up")
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn connect_succeeds_against_a_real_instance() {
    let adapter = connect().await;
    assert!(adapter.report_health().await.is_connected());
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn describe_schema_returns_the_seeded_tables_and_columns() {
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    let users = schema
        .find_table("users")
        .expect("the seeded users table should be described");
    assert_eq!(users.namespace, "public");

    let column_names: Vec<&str> = users.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        column_names,
        vec!["id", "email", "full_name", "signup_date", "active"],
        "columns should be reported in the table's own order"
    );

    let email = users.column("email").unwrap();
    assert_eq!(email.data_type, "text");
    assert!(!email.nullable);

    assert!(
        schema.find_table("orders").is_some(),
        "every seeded table should be described, not just the first"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn describe_schema_excludes_system_catalogs() {
    let adapter = connect().await;
    let schema = adapter.describe_schema().await.unwrap();

    assert!(
        schema
            .tables
            .iter()
            .all(|t| t.namespace != "pg_catalog" && t.namespace != "information_schema"),
        "the user should see their own tables, not Postgres internals"
    );
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn report_health_reads_the_server_version_without_changing_anything() {
    let adapter = connect().await;
    let health = adapter.report_health().await;

    assert!(health.is_connected());
    assert!(
        health
            .server_version
            .as_deref()
            .is_some_and(|v| v.contains("PostgreSQL")),
        "health check should report the engine version it reached"
    );
}

// --- failure paths: docs/17 requires a typed result, docs/16 item 2 requires
// --- that nothing user-facing carries the credential ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn bad_credentials_return_a_typed_error_instead_of_panicking() {
    let result = PostgresAdapter::connect(PostgresConnectionDetails {
        password: "definitely-the-wrong-password",
        ..details_from_env()
    })
    .await;

    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("connecting with a wrong password must not succeed"),
    };
    assert!(matches!(error, AdapterError::Connection { .. }));
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_connection_error_never_contains_the_password() {
    let result = PostgresAdapter::connect(PostgresConnectionDetails {
        password: "definitely-the-wrong-password",
        ..details_from_env()
    })
    .await;

    let error = result.err().expect("wrong password should fail");
    for rendered in [format!("{error}"), format!("{error:?}")] {
        assert!(
            !rendered.contains("definitely-the-wrong-password"),
            "credential leaked into an error shown to the user: {rendered}"
        );
    }
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_unreachable_host_returns_a_typed_error() {
    let result = PostgresAdapter::connect(PostgresConnectionDetails {
        port: 1,
        ..details_from_env()
    })
    .await;

    assert!(
        matches!(result, Err(AdapterError::Connection { .. })),
        "an unreachable database should surface as a connection error the UI can render"
    );
}
