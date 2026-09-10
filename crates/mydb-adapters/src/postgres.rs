//! The PostgreSQL adapter (docs/04-database-adapters.md).
//!
//! Phase 1's only engine. Preview and execute arrive in T7; this module
//! currently covers connect, describe schema, and health.

use mydb_core::{Column, Intent, Operation, Schema, Table};
use tokio_postgres::{Client, NoTls};

use crate::records::{Record, RecordSet, SAMPLE_LIMIT};
use crate::sql::{as_driver_params, build_where, quote_identifier, quote_table};
use crate::{Adapter, AdapterError, ApprovedWrite, ExecutionOutcome, Health, Preview};

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
///
/// The client sits behind a mutex because opening a transaction requires
/// mutable access to it, while the adapter is shared across the app as `&self`.
/// One connection per adapter is right for phase 1: a desktop app runs one
/// user's commands one at a time, and pooling would add concurrency the
/// confirmation workflow does not want anyway.
pub struct PostgresAdapter {
    client: tokio::sync::Mutex<Client>,
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

        Ok(Self {
            client: tokio::sync::Mutex::new(client),
        })
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
            .lock()
            .await
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
            .lock()
            .await
            .query_one("SELECT version() AS version", &[])
            .await
        {
            Ok(row) => Health::connected(row.try_get::<_, String>("version").ok()),
            Err(error) => Health::error(describe_query_failure(error).to_string()),
        }
    }

    async fn run_read(&self, intent: &Intent) -> Result<RecordSet, AdapterError> {
        self.read_matching(intent).await
    }

    async fn build_preview(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        match intent.operation {
            Operation::Delete => self.preview_delete(intent).await,
            // A read needs no preview: docs/05 step 4 runs it directly and
            // shows the result. Reaching here means the workflow routed a
            // read down the write path, which is a logic error worth failing
            // loudly rather than quietly previewing.
            Operation::Read => Err(AdapterError::Unsupported(
                "a read does not need a preview; run it directly".to_string(),
            )),
        }
    }

    async fn execute(&self, approved: ApprovedWrite) -> Result<ExecutionOutcome, AdapterError> {
        // The intent comes out of the approved preview, never from a separate
        // argument. What runs below is necessarily what the user was shown.
        let intent = approved.intent();

        match intent.operation {
            Operation::Delete => self.execute_delete(intent).await,
            Operation::Read => Err(AdapterError::Unsupported(
                "a read is not an executable write".to_string(),
            )),
        }
    }
}

impl PostgresAdapter {
    /// Reads the column names of one table, in the table's own order.
    ///
    /// Needed twice over: the preview screen needs a header row, and the
    /// SELECT below names each column explicitly rather than using `*`.
    async fn column_names(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Vec<String>, AdapterError> {
        const SQL: &str = "
            SELECT column_name
              FROM information_schema.columns
             WHERE table_schema = $1
               AND table_name = $2
             ORDER BY ordinal_position
        ";

        let rows = self
            .client
            .lock()
            .await
            .query(SQL, &[&namespace, &table])
            .await
            .map_err(describe_query_failure)?;

        if rows.is_empty() {
            return Err(AdapterError::query(format!(
                "no table called {table} in {namespace}"
            )));
        }

        Ok(rows.iter().map(|row| row.get("column_name")).collect())
    }

    /// Reads the records a filter matches, with an exact count.
    ///
    /// This one function serves both a read (docs/05 step 4) and the DELETE
    /// preview (docs/04). That is deliberate: the preview for a DELETE is
    /// defined as the equivalent SELECT with the same WHERE clause, so it
    /// should not be a second implementation that could drift from the real
    /// read.
    async fn read_matching(&self, intent: &Intent) -> Result<RecordSet, AdapterError> {
        let columns = self.column_names(&intent.namespace, &intent.table).await?;
        let table = quote_table(&intent.namespace, &intent.table);

        // Each column is cast to text so any column type the user happens to
        // have can be rendered, rather than this adapter needing to decode
        // every type Postgres supports. These records exist to be read.
        let projection = columns
            .iter()
            .map(|name| {
                let quoted = quote_identifier(name);
                format!("{quoted}::text AS {quoted}")
            })
            .collect::<Vec<_>>()
            .join(", ");

        let clause = build_where(&intent.filter, 1);
        let params = as_driver_params(&clause.params);

        let client = self.client.lock().await;

        let total_count: i64 = client
            .query_one(
                &format!("SELECT count(*) AS total FROM {table}{}", clause.sql),
                &params,
            )
            .await
            .map_err(describe_query_failure)?
            .get("total");

        let select_sql = format!(
            "SELECT {projection} FROM {table}{} LIMIT {SAMPLE_LIMIT}",
            clause.sql
        );
        let rows = client
            .query(&select_sql, &params)
            .await
            .map_err(describe_query_failure)?;

        let records = rows
            .iter()
            .map(|row| Record {
                cells: (0..row.len())
                    .map(|index| row.get::<_, Option<String>>(index))
                    .collect(),
            })
            .collect();

        Ok(RecordSet::new(
            columns,
            records,
            total_count.max(0) as u64,
            select_sql,
        ))
    }

    /// The DELETE preview: the equivalent SELECT with the same WHERE clause
    /// (docs/04-database-adapters.md).
    async fn preview_delete(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        let affected = self.read_matching(intent).await?;
        Ok(Preview::new(intent.clone(), affected))
    }

    /// Runs a confirmed DELETE inside a transaction.
    ///
    /// docs/04-database-adapters.md requires the real write to be wrapped in a
    /// transaction where the engine supports it, so a mid-statement failure
    /// cannot leave a partial change behind.
    async fn execute_delete(&self, intent: &Intent) -> Result<ExecutionOutcome, AdapterError> {
        let table = quote_table(&intent.namespace, &intent.table);

        // Built by the same function, from the same filter, as the preview's
        // WHERE clause. The two cannot describe different sets of rows.
        let clause = build_where(&intent.filter, 1);
        let params = as_driver_params(&clause.params);
        let sql = format!("DELETE FROM {table}{}", clause.sql);

        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(describe_query_failure)?;

        let rows_affected = match transaction.execute(&sql, &params).await {
            Ok(count) => count,
            Err(error) => {
                // Rolled back explicitly rather than by drop, so the failure
                // path reads as deliberate.
                let _ = transaction.rollback().await;
                return Err(describe_query_failure(error));
            }
        };

        transaction.commit().await.map_err(describe_query_failure)?;

        Ok(ExecutionOutcome { rows_affected })
    }
}
