//! Shared helpers for adapter integration tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]

use mydb_adapters::postgres::PostgresConnectionDetails;

/// Where the test instance lives.
///
/// Defaults match `docker-compose.yml`. CI overrides the port because a
/// service container publishes on 5432, while local development uses a
/// non-default port to avoid colliding with a real Postgres.
pub fn details_from_env() -> PostgresConnectionDetails<'static> {
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

/// Restores the seed fixture so a mutating test starts from known data.
///
/// Tests that delete rows call this before and after, so they neither inherit
/// another test's damage nor leave any behind.
pub async fn reset_seed() {
    let details = details_from_env();
    let mut config = tokio_postgres::Config::new();
    config
        .host(details.host)
        .port(details.port)
        .dbname(details.database)
        .user(details.username)
        .password(details.password);

    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let seed = include_str!("../../../../testing/seed.sql");
    client.batch_execute(seed).await.unwrap();
}
