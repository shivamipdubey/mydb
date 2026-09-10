//! The PostgreSQL adapter (docs/04-database-adapters.md).
//!
//! Phase 1's only engine. Preview and execute arrive in T7; this module
//! currently covers connect, describe schema, and health.

use mydb_core::{Column, Schema, Table};
use tokio_postgres::{Client, NoTls};

use crate::{Adapter, AdapterError, Health};

/// Everything needed to reach one Postgres database.
///
/// Deliberately separate from the stored `Connection` record: the adapter
/// layer should not depend on how connections happen to be persisted, and
/// keeping the credential's lifetime this narrow means it exists in memory
/// only while a connection is being opened (docs/06-credential-vault.md).
pub struct PostgresConnectionDetails<'a> {
    pub host: &'a str,
    pub port: u16,
    pub database: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

/// A live connection to a Postgres database.
pub struct PostgresAdapter {
    client: Client,
}

impl PostgresAdapter {
    /// Opens a connection.
    ///
    /// The connection parameters are set on a builder rather than formatted
    /// into a connection string. docs/16-security-and-cybersafety-checklist.md
    /// item 3 requires filters and payloads to avoid string concatenation, and
    /// the same reasoning applies here: a password containing a space, quote,
    /// or equals sign would corrupt a hand-built connection string, and the
    /// resulting parse error could carry fragments of the credential.
    pub async fn connect(details: PostgresConnectionDetails<'_>) -> Result<Self, AdapterError> {
        let mut config = tokio_postgres::Config::new();
        config
            .host(details.host)
            .port(details.port)
            .dbname(details.database)
            .user(details.username)
            .password(details.password)
            .application_name("MYDB");

        let (client, connection) = config.connect(NoTls).await.map_err(describe_failure)?;

        // The driver splits into a client handle and a connection future that
        // must be polled for the client to work at all.
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                log_connection_drop(&error);
            }
        });

        Ok(Self { client })
    }
}

/// Turns a driver error into a message safe to show the user.
///
/// `tokio_postgres::Error` renders the failure, not the configuration, so it
/// carries no credential. This function exists so that stays true by
/// intention rather than by luck: everything user-facing from this adapter
/// goes through here, and the only thing interpolated is the driver's own
/// description.
fn describe_failure(error: tokio_postgres::Error) -> AdapterError {
    let reason = match error.as_db_error() {
        Some(db_error) => db_error.message().to_string(),
        None => error.to_string(),
    };
    AdapterError::connection(reason)
}

fn describe_query_failure(error: tokio_postgres::Error) -> AdapterError {
    let reason = match error.as_db_error() {
        Some(db_error) => db_error.message().to_string(),
        None => error.to_string(),
    };
    AdapterError::query(reason)
}

/// A dropped connection is worth recording, but only as a category. The
/// connection config, including the password, must never reach a log line
/// (docs/16 item 2).
fn log_connection_drop(error: &tokio_postgres::Error) {
    eprintln!("postgres connection closed: {error}");
}

/// Reads table and column structure from the catalog.
///
/// System namespaces are excluded so the user sees their own tables. The
/// filter is a bound parameter rather than an interpolated list, consistent
/// with every other query this adapter issues.
const DESCRIBE_SCHEMA_SQL: &str = "
    SELECT c.table_schema,
           c.table_name,
           c.column_name,
           c.data_type,
           c.is_nullable
      FROM information_schema.columns AS c
      JOIN information_schema.tables AS t
        ON t.table_schema = c.table_schema
       AND t.table_name = c.table_name
     WHERE c.table_schema <> ALL($1)
       AND t.table_type = 'BASE TABLE'
     ORDER BY c.table_schema, c.table_name, c.ordinal_position
";

const SYSTEM_NAMESPACES: [&str; 2] = ["pg_catalog", "information_schema"];

#[async_trait::async_trait]
impl Adapter for PostgresAdapter {
    async fn describe_schema(&self) -> Result<Schema, AdapterError> {
        let rows = self
            .client
            .query(DESCRIBE_SCHEMA_SQL, &[&SYSTEM_NAMESPACES.as_slice()])
            .await
            .map_err(describe_query_failure)?;

        let mut tables: Vec<Table> = Vec::new();
        for row in rows {
            let namespace: String = row.get("table_schema");
            let name: String = row.get("table_name");
            let column = Column {
                name: row.get("column_name"),
                data_type: row.get("data_type"),
                nullable: row.get::<_, String>("is_nullable") == "YES",
            };

            match tables.last_mut() {
                // Rows arrive grouped and ordered by table, so the run of
                // columns for one table always lands on the entry just added.
                Some(table) if table.namespace == namespace && table.name == name => {
                    table.columns.push(column);
                }
                _ => tables.push(Table {
                    namespace,
                    name,
                    columns: vec![column],
                }),
            }
        }

        Ok(Schema { tables })
    }

    async fn report_health(&self) -> Health {
        // Strictly read-only, and chosen so it could never be mistaken for a
        // data-changing statement (docs/13-dashboard-and-health-monitoring.md).
        match self
            .client
            .query_one("SELECT version() AS version", &[])
            .await
        {
            Ok(row) => Health::connected(row.try_get::<_, String>("version").ok()),
            Err(error) => Health::error(describe_query_failure(error).to_string()),
        }
    }
}
