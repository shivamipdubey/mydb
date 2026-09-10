//! Proves the containerized Postgres test instance is reachable and seeded.
//!
//! docs/18-testing-strategy.md requires adapter correctness to be verified
//! against a real or containerized engine. This file is the harness that later
//! adapter tests build on; it deliberately tests the fixture itself, so a
//! broken fixture fails here with an obvious message instead of surfacing as a
//! confusing failure inside an adapter test.
//!
//! These tests are `#[ignore]`d so `cargo test` stays runnable without Docker.
//! Run them with `cargo test --workspace -- --ignored`, which is what the
//! Linux CI job does. They are never skipped silently: an ignored test is
//! reported as ignored, and CI runs them explicitly.

// The workspace denies unwrap, expect, and panic to keep them out of the
// application (docs/17-coding-standards.md). Test code is the one place a
// panic is the correct response to an unexpected value.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// Connection string for the local test instance from docker-compose.yml.
/// Overridable so CI can point at its own service container.
fn test_database_url() -> String {
    std::env::var("MYDB_TEST_DATABASE_URL").unwrap_or_else(|_| {
        "host=localhost port=55432 user=mydb_test password=mydb_test_password dbname=mydb_test"
            .to_string()
    })
}

async fn connect() -> tokio_postgres::Client {
    let (client, connection) = tokio_postgres::connect(&test_database_url(), tokio_postgres::NoTls)
        .await
        .unwrap_or_else(|error| {
            panic!("could not reach the test Postgres instance ({error}). Start it with ./scripts/db.sh up")
        });
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("test connection error: {error}");
        }
    });
    client
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn connects_to_the_test_instance() {
    let client = connect().await;
    let row = client.query_one("SELECT 1 AS one", &[]).await.unwrap();
    let one: i32 = row.get("one");
    assert_eq!(one, 1);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn seed_data_is_present_and_exact() {
    let client = connect().await;

    let row = client
        .query_one("SELECT count(*) AS n FROM users", &[])
        .await
        .unwrap();
    let users: i64 = row.get("n");
    assert_eq!(users, 7, "seed should contain exactly 7 users");

    let row = client
        .query_one("SELECT count(*) AS n FROM orders", &[])
        .await
        .unwrap();
    let orders: i64 = row.get("n");
    assert_eq!(orders, 4, "seed should contain exactly 4 orders");

    // A filter later tasks build previews from, asserted here so the fixture
    // itself is known-good before any adapter depends on it.
    let rows = client
        .query(
            // ($1::text)::date, not $1::date: Postgres infers the parameter's
            // type from the comparison and would demand a date, rejecting the
            // string bind. Typed parameter binding proper is the adapter's job
            // in T7; this harness only needs a known-good filter.
            "SELECT email FROM users WHERE signup_date < ($1::text)::date ORDER BY id",
            &[&"2024-01-01"],
        )
        .await
        .unwrap();
    let emails: Vec<String> = rows.iter().map(|r| r.get::<_, String>("email")).collect();
    assert_eq!(
        emails,
        vec!["ada@example.com", "grace@example.com", "alan@example.com"],
        "exactly three users signed up before 2024-01-01"
    );
}
