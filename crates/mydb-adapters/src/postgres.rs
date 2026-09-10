//! The PostgreSQL adapter (docs/04-database-adapters.md).
//!
//! Phase 1's only engine, and the reference the other adapters follow.
//! Covers connect, describe schema, health, and preview plus execute for
//! delete, insert, update, drop table, and truncate, with before and after
//! state captured inside each write's own transaction
//! (docs/07-audit-log-and-recovery-bin.md).

use mydb_core::{
    Assignment, Column, Intent, Operation, RecordSnapshot, Schema, StateSink, StateSnapshot, Table,
    Value,
};
use tokio_postgres::{Client, NoTls};

use crate::records::{Record, RecordSet, SAMPLE_LIMIT};
use crate::sql::{
    as_driver_params, build_insert, build_set, build_where, can_write_to, quote_identifier,
    quote_table,
};
use crate::{
    Adapter, AdapterError, ApprovedWrite, ExecutionOutcome, Health, Preview, TableOutline,
    STATE_CAPTURE_LIMIT,
};

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
            Operation::Update => self.preview_update(intent).await,
            Operation::Insert => self.preview_insert(intent).await,
            Operation::DropTable | Operation::Truncate => self.preview_table(intent).await,
            // A read needs no preview: docs/05 step 4 runs it directly and
            // shows the result. Reaching here means the workflow routed a
            // read down the write path, which is a logic error worth failing
            // loudly rather than quietly previewing.
            Operation::Read => Err(AdapterError::Unsupported(
                "a read does not need a preview; run it directly".to_string(),
            )),
        }
    }

    async fn execute(
        &self,
        approved: ApprovedWrite,
        capture: &mut dyn StateSink,
    ) -> Result<ExecutionOutcome, AdapterError> {
        // The intent comes out of the approved preview, never from a separate
        // argument. What runs below is necessarily what the user was shown.
        let intent = approved.intent();

        match intent.operation {
            Operation::Delete => self.execute_delete(intent, capture).await,
            Operation::Update => self.execute_update(intent, capture).await,
            // Nothing existed before an insert, so there is nothing to
            // capture and the sink is deliberately left untouched.
            Operation::Insert => self.execute_insert(intent).await,
            // A schema operation destroys everything in the table, so the
            // count it reports is the one the user was shown in the preview.
            // Postgres reports no affected rows for DDL, and saying "0
            // records" after emptying a table would be plainly wrong.
            Operation::DropTable | Operation::Truncate => {
                self.execute_schema_change(intent, approved.preview().affected_count(), capture)
                    .await
            }
            Operation::Read => Err(AdapterError::Unsupported(
                "a read is not an executable write".to_string(),
            )),
        }
    }
}

impl PostgresAdapter {
    /// Reads one table's columns, in the table's own order, with their types.
    ///
    /// Three things need this and all three need the types, not just the
    /// names: the preview screen's header row, the explicit projection below,
    /// and, most importantly, typing each filter parameter to the column it
    /// will be compared against.
    async fn describe_table(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Vec<ColumnShape>, AdapterError> {
        const SQL: &str = "
            SELECT column_name, data_type, is_nullable, column_default
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

        Ok(rows
            .iter()
            .map(|row| ColumnShape {
                column: Column {
                    name: row.get("column_name"),
                    data_type: row.get("data_type"),
                    nullable: row.get::<_, String>("is_nullable") == "YES",
                },
                default: row.get("column_default"),
            })
            .collect())
    }

    /// Reads the records a filter matches, with an exact count.
    ///
    /// This one function serves both a read (docs/05 step 4) and the DELETE
    /// preview (docs/04). That is deliberate: the preview for a DELETE is
    /// defined as the equivalent SELECT with the same WHERE clause, so it
    /// should not be a second implementation that could drift from the real
    /// read.
    async fn read_matching(&self, intent: &Intent) -> Result<RecordSet, AdapterError> {
        let schema_columns = columns_of(
            &self
                .describe_table(&intent.namespace, &intent.table)
                .await?,
        );
        let columns: Vec<String> = schema_columns.iter().map(|c| c.name.clone()).collect();
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

        let clause = build_where(&intent.filter, &schema_columns, 1);
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

    /// Runs a confirmed DELETE, capturing what it removes.
    ///
    /// The capture happens inside the write's own transaction, before the
    /// delete. Reading the rows in a separate statement beforehand would
    /// leave a gap in which they could change, and the recovery bin would
    /// then hold something that was never what the delete actually removed.
    async fn execute_delete(
        &self,
        intent: &Intent,
        capture: &mut dyn StateSink,
    ) -> Result<ExecutionOutcome, AdapterError> {
        let shape = self
            .describe_table(&intent.namespace, &intent.table)
            .await?;
        let columns = columns_of(&shape);
        let table = quote_table(&intent.namespace, &intent.table);

        // Built by the same function, from the same filter, against the same
        // column types as the preview's WHERE clause. The two cannot describe
        // different sets of rows.
        let clause = build_where(&intent.filter, &columns, 1);
        let params = as_driver_params(&clause.params);
        let sql = format!("DELETE FROM {table}{}", clause.sql);

        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(describe_query_failure)?;

        let before = match stream_capture(&transaction, &table, &clause.sql, &params, capture).await
        {
            Ok(before) => before,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        };

        let rows_affected = match transaction.execute(&sql, &params).await {
            Ok(count) => count,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(describe_query_failure(error));
            }
        };

        transaction.commit().await.map_err(describe_query_failure)?;
        Ok(ExecutionOutcome::removing(rows_affected, before))
    }

    /// Runs a confirmed INSERT, capturing the record it creates.
    ///
    /// Nothing existed before, so the before-state is empty rather than
    /// unavailable: that is the truth, not a gap in the capture.
    async fn execute_insert(&self, intent: &Intent) -> Result<ExecutionOutcome, AdapterError> {
        let shape = self
            .describe_table(&intent.namespace, &intent.table)
            .await?;
        let columns = columns_of(&shape);
        Self::check_writable(&intent.assignments, &columns)?;

        let table = quote_table(&intent.namespace, &intent.table);
        let statement = build_insert(&table, &intent.assignments, &columns);
        let params = as_driver_params(&statement.params);

        // RETURNING gives the row as it actually landed, including any
        // defaults the table filled in, which is what the audit log should
        // hold rather than only the values the command named.
        let sql = format!(
            "{} RETURNING to_jsonb({}) AS record",
            statement.sql,
            quote_identifier(&intent.table)
        );

        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(describe_query_failure)?;

        let rows = match transaction.query(&sql, &params).await {
            Ok(rows) => rows,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(describe_query_failure(error));
            }
        };

        transaction.commit().await.map_err(describe_query_failure)?;

        let created: Vec<RecordSnapshot> = rows.iter().filter_map(snapshot_from).collect();
        Ok(ExecutionOutcome {
            rows_affected: rows.len() as u64,
            before: StateSnapshot::empty(),
            after: Some(StateSnapshot::complete(created)),
        })
    }

    /// Runs a confirmed UPDATE, capturing the records before and after.
    ///
    /// The after-state is read back by primary key, not by the filter. The
    /// filter may no longer match: "set active to false where active is
    /// true" matches nothing once it has run, and re-reading by filter would
    /// record that the rows had disappeared.
    async fn execute_update(
        &self,
        intent: &Intent,
        capture: &mut dyn StateSink,
    ) -> Result<ExecutionOutcome, AdapterError> {
        let shape = self
            .describe_table(&intent.namespace, &intent.table)
            .await?;
        let columns = columns_of(&shape);
        Self::check_writable(&intent.assignments, &columns)?;
        let key = self.primary_key(&intent.namespace, &intent.table).await?;

        let table = quote_table(&intent.namespace, &intent.table);
        let set = build_set(&intent.assignments, &columns, 1);
        let clause = build_where(&intent.filter, &columns, set.next_placeholder);
        let sql = format!("UPDATE {table}{}{}", set.sql, clause.sql);

        let mut all_params = set.params.clone();
        all_params.extend(clause.params.clone());
        let write_params = as_driver_params(&all_params);

        // The update's WHERE clause is numbered to follow its SET clause, so
        // the capture needs the same filter renumbered from one. Same
        // function, same filter, same column types: the rows captured are
        // necessarily the rows updated.
        let capture_clause = build_where(&intent.filter, &columns, 1);
        let capture_params = as_driver_params(&capture_clause.params);

        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(describe_query_failure)?;

        let before = match stream_capture(
            &transaction,
            &table,
            &capture_clause.sql,
            &capture_params,
            capture,
        )
        .await
        {
            Ok(before) => before,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        };

        let rows_affected = match transaction.execute(&sql, &write_params).await {
            Ok(count) => count,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(describe_query_failure(error));
            }
        };

        // Re-read by identity while still inside the transaction.
        let after = match &key {
            Some(key_column) => {
                match capture_by_key(&transaction, &table, key_column, &before).await {
                    Ok(after) => Some(after),
                    Err(error) => {
                        let _ = transaction.rollback().await;
                        return Err(error);
                    }
                }
            }
            // No single-column primary key, so there is nothing to match the
            // changed records back by. Saying so beats reporting an empty
            // result that would read as "the records vanished".
            None => None,
        };

        transaction.commit().await.map_err(describe_query_failure)?;

        Ok(ExecutionOutcome {
            rows_affected,
            before,
            after,
        })
    }

    /// Runs a confirmed DROP TABLE or TRUNCATE, capturing what it destroys.
    ///
    /// Postgres supports transactional DDL, so these get the same
    /// all-or-nothing guarantee as every other write, and the capture is
    /// taken inside the same transaction.
    async fn execute_schema_change(
        &self,
        intent: &Intent,
        previewed_rows: u64,
        capture: &mut dyn StateSink,
    ) -> Result<ExecutionOutcome, AdapterError> {
        // Confirms the table exists and gives the same clear error as every
        // other operation when it does not.
        self.describe_table(&intent.namespace, &intent.table)
            .await?;
        let table = quote_table(&intent.namespace, &intent.table);

        let sql = match intent.operation {
            Operation::DropTable => format!("DROP TABLE {table}"),
            Operation::Truncate => format!("TRUNCATE TABLE {table}"),
            // Unreachable through the trait, which routes only these two
            // here. Returning rather than panicking keeps docs/17's rule
            // that this layer never throws into the UI.
            other => {
                return Err(AdapterError::Unsupported(format!(
                    "{} is not a schema change",
                    other.verb()
                )))
            }
        };

        let mut client = self.client.lock().await;
        let transaction = client.transaction().await.map_err(describe_query_failure)?;

        let before = match stream_capture(&transaction, &table, "", &[], capture).await {
            Ok(before) => before,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        };

        if let Err(error) = transaction.execute(&sql, &[]).await {
            let _ = transaction.rollback().await;
            return Err(describe_query_failure(error));
        }

        transaction.commit().await.map_err(describe_query_failure)?;

        Ok(ExecutionOutcome {
            // Neither statement reports affected rows, so the count is the
            // one the user was shown. Reporting zero after emptying a table
            // would be plainly wrong.
            rows_affected: previewed_rows,
            before,
            after: Some(StateSnapshot::empty()),
        })
    }

    /// Reads the table's primary key, when it is a single column.
    ///
    /// A composite key or no key at all both return `None`. Both mean the
    /// same thing to the caller: there is no simple identity to re-read a
    /// record by, so it must not pretend there is.
    async fn primary_key(
        &self,
        namespace: &str,
        table: &str,
    ) -> Result<Option<String>, AdapterError> {
        const SQL: &str = "
            SELECT column_name
              FROM information_schema.table_constraints AS c
              JOIN information_schema.key_column_usage AS k
                ON k.constraint_name = c.constraint_name
               AND k.table_schema = c.table_schema
             WHERE c.constraint_type = 'PRIMARY KEY'
               AND c.table_schema = $1
               AND c.table_name = $2
             ORDER BY k.ordinal_position
        ";

        let rows = self
            .client
            .lock()
            .await
            .query(SQL, &[&namespace, &table])
            .await
            .map_err(describe_query_failure)?;

        match rows.len() {
            1 => Ok(rows.first().map(|row| row.get("column_name"))),
            _ => Ok(None),
        }
    }
}

/// Reads a captured row's JSON into a snapshot.
///
/// Postgres builds the JSON with `to_jsonb`, so each value keeps its own
/// type: a number stays a number, a null stays a null, a nested value stays
/// nested. Casting everything to text for display, as the preview does,
/// would be wrong here; this is the only record of data that may no longer
/// exist.
fn snapshot_from(row: &tokio_postgres::Row) -> Option<RecordSnapshot> {
    match row.try_get::<_, serde_json::Value>("record") {
        Ok(serde_json::Value::Object(fields)) => Some(RecordSnapshot::new(fields)),
        _ => None,
    }
}

/// Streams the records a write is about to change, with an exact count.
///
/// Every record goes to the sink, one at a time, so a before-state larger
/// than memory still reaches the recovery bin in full, as docs/07 requires.
/// Only a bounded sample is kept in memory, for the audit log: docs/07 puts
/// full detail in the log for a small operation and a sample plus a
/// reference for a large one, so the log never needs the whole thing.
///
/// `where_sql` is either a rendered WHERE clause or empty, in which case the
/// whole table is captured, which is what a drop or truncate destroys.
async fn stream_capture(
    transaction: &tokio_postgres::Transaction<'_>,
    table: &str,
    where_sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    sink: &mut dyn StateSink,
) -> Result<StateSnapshot, AdapterError> {
    use futures_util::{pin_mut, TryStreamExt};

    let total: i64 = transaction
        .query_one(
            &format!("SELECT count(*) AS total FROM {table} AS t{where_sql}"),
            params,
        )
        .await
        .map_err(describe_query_failure)?
        .get("total");

    // query_raw rather than query: it yields rows as they arrive instead of
    // collecting them all first, which is the whole point when the set is
    // larger than memory.
    let stream = transaction
        .query_raw(
            &format!("SELECT to_jsonb(t) AS record FROM {table} AS t{where_sql}"),
            params.iter().copied(),
        )
        .await
        .map_err(describe_query_failure)?;
    pin_mut!(stream);

    let mut kept: Vec<RecordSnapshot> = Vec::new();
    while let Some(row) = stream.try_next().await.map_err(describe_query_failure)? {
        if let Some(snapshot) = snapshot_from(&row) {
            sink.accept(&snapshot)
                .map_err(|error| AdapterError::query(error.to_string()))?;
            if kept.len() < STATE_CAPTURE_LIMIT {
                kept.push(snapshot);
            }
        }
    }

    Ok(StateSnapshot::sample(kept, total.max(0) as u64))
}

/// Re-reads records by their primary key values.
///
/// Both sides are compared as text so one query serves any key type, integer,
/// uuid or otherwise, without this function needing to know which.
async fn capture_by_key(
    transaction: &tokio_postgres::Transaction<'_>,
    table: &str,
    key_column: &str,
    before: &StateSnapshot,
) -> Result<StateSnapshot, AdapterError> {
    let keys: Vec<String> = before
        .records()
        .iter()
        .filter_map(|record| record.get(key_column))
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .collect();

    if keys.is_empty() {
        return Ok(StateSnapshot::empty());
    }

    let quoted = quote_identifier(key_column);
    let rows = transaction
        .query(
            &format!(
                "SELECT to_jsonb(t) AS record FROM {table} AS t \
                 WHERE t.{quoted}::text = ANY($1) LIMIT {STATE_CAPTURE_LIMIT}"
            ),
            &[&keys],
        )
        .await
        .map_err(describe_query_failure)?;

    let records: Vec<RecordSnapshot> = rows.iter().filter_map(snapshot_from).collect();
    Ok(StateSnapshot::complete(records))
}

/// A column, plus the default the table gives it when a value is not supplied.
struct ColumnShape {
    column: Column,
    default: Option<String>,
}

fn columns_of(shape: &[ColumnShape]) -> Vec<Column> {
    shape.iter().map(|entry| entry.column.clone()).collect()
}

/// A value as the preview should display it. `None` renders as null.
fn display_value(value: &Value) -> Option<String> {
    match value {
        Value::Text(text) => Some(text.clone()),
        Value::Integer(number) => Some(number.to_string()),
        Value::Float(number) => Some(number.to_string()),
        Value::Boolean(flag) => Some(flag.to_string()),
        Value::Date(date) => Some(date.clone()),
        Value::Null => None,
    }
}

impl PostgresAdapter {
    /// Refuses a write MYDB cannot type correctly.
    ///
    /// An exotic column type can be read and compared as text, but writing
    /// one needs a cast that cannot be built without knowing the underlying
    /// type. Saying so plainly beats writing something subtly wrong.
    fn check_writable(assignments: &[Assignment], columns: &[Column]) -> Result<(), AdapterError> {
        for assignment in assignments {
            match columns.iter().find(|c| c.name == assignment.column) {
                None => {
                    return Err(AdapterError::query(format!(
                        "no column called {}",
                        assignment.column
                    )))
                }
                Some(column) if !can_write_to(&column.data_type) => {
                    return Err(AdapterError::Unsupported(format!(
                        "MYDB cannot write to {}, which is of type {}, yet",
                        column.name, column.data_type
                    )))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// The INSERT preview: the exact record that will be created.
    ///
    /// docs/04-database-adapters.md is explicit that this needs no read
    /// against existing data, and it does none. It reads the table's shape
    /// so it can show every column, including the ones the command did not
    /// name: those are where a default or a null will land, and a person
    /// checking whether the new record is right needs to see them.
    async fn preview_insert(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        let shape = self
            .describe_table(&intent.namespace, &intent.table)
            .await?;
        let schema_columns = columns_of(&shape);
        Self::check_writable(&intent.assignments, &schema_columns)?;

        let cells = shape
            .iter()
            .map(|entry| {
                match intent
                    .assignments
                    .iter()
                    .find(|assignment| assignment.column == entry.column.name)
                {
                    Some(assignment) => display_value(&assignment.value),
                    // Showing the default expression rather than a blank is
                    // the honest answer: the row will not be null here.
                    None => entry.default.clone(),
                }
            })
            .collect();

        let statement = build_insert(
            &quote_table(&intent.namespace, &intent.table),
            &intent.assignments,
            &schema_columns,
        );

        Ok(Preview::new(
            intent.clone(),
            RecordSet::new(
                schema_columns.iter().map(|c| c.name.clone()).collect(),
                vec![Record { cells }],
                1,
                statement.sql,
            ),
        ))
    }

    /// The UPDATE preview: the equivalent SELECT with the same WHERE clause,
    /// showing the records that will be changed
    /// (docs/04-database-adapters.md).
    async fn preview_update(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        let schema_columns = columns_of(
            &self
                .describe_table(&intent.namespace, &intent.table)
                .await?,
        );
        // Checked before the preview, not after: a preview the user could
        // confirm and then have fail would be worse than an early refusal.
        Self::check_writable(&intent.assignments, &schema_columns)?;

        let affected = self.read_matching(intent).await?;
        Ok(Preview::new(intent.clone(), affected))
    }

    /// The DROP TABLE and TRUNCATE preview: the table's current schema and
    /// row count (docs/04-database-adapters.md).
    ///
    /// Not a list of matching records, for two reasons. Neither operation has
    /// a filter to match against, and a DROP TABLE removes the structure as
    /// well as the data, so the structure is part of what the user is being
    /// asked to agree to lose.
    async fn preview_table(&self, intent: &Intent) -> Result<Preview, AdapterError> {
        let schema_columns = columns_of(
            &self
                .describe_table(&intent.namespace, &intent.table)
                .await?,
        );
        let table = quote_table(&intent.namespace, &intent.table);

        let count_sql = format!("SELECT count(*) AS total FROM {table}");
        let row_count: i64 = self
            .client
            .lock()
            .await
            .query_one(&count_sql, &[])
            .await
            .map_err(describe_query_failure)?
            .get("total");

        Ok(Preview::of_table(
            intent.clone(),
            TableOutline::new(schema_columns, row_count.max(0) as u64, count_sql),
        ))
    }
}
