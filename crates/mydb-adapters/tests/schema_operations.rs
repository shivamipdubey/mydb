//! DROP TABLE and TRUNCATE: previewed as a table, not as a list of records.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mydb_adapters::postgres::PostgresAdapter;
use mydb_adapters::{Adapter, AdapterError, PreviewBody};
use mydb_core::{Comparison, Condition, Engine, Filter, Intent, Operation, Value};

mod support;
use support::{details_from_env, reset_seed};

async fn adapter() -> PostgresAdapter {
    PostgresAdapter::connect(details_from_env())
        .await
        .expect("start the test database with ./scripts/db.sh up")
}

fn on(table: &str, operation: Operation) -> Intent {
    Intent {
        engine: Engine::Postgres,
        namespace: "public".to_string(),
        table: table.to_string(),
        operation,
        filter: Filter::everything(),
        assignments: Vec::new(),
    }
}

/// Whether a table still exists, asked without assuming it does.
async fn table_exists(adapter: &PostgresAdapter, name: &str) -> bool {
    adapter
        .describe_schema()
        .await
        .unwrap()
        .find_table(name)
        .is_some()
}

async fn row_count(adapter: &PostgresAdapter, table: &str) -> u64 {
    adapter
        .run_read(&on(table, Operation::Read))
        .await
        .unwrap()
        .total_count()
}

// --- the preview shows the table, per docs/04 ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_drop_previews_the_current_schema_and_row_count() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&on("disposable", Operation::DropTable))
        .await
        .unwrap();

    let outline = match preview.body() {
        PreviewBody::Table(outline) => outline,
        PreviewBody::Records(_) => {
            panic!("docs/04 requires a schema change to preview the table, not matching records")
        }
    };

    assert_eq!(outline.row_count(), 3);
    let names: Vec<&str> = outline
        .columns()
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(names, ["id", "label", "note"]);

    // The structure a drop also removes, described accurately.
    assert_eq!(outline.columns()[0].data_type, "integer");
    assert!(!outline.columns()[0].nullable);
    assert!(outline.columns()[2].nullable);

    // The count the confirmation screen shows is the table's rows.
    assert_eq!(preview.affected_count(), 3);

    // And nothing has happened.
    assert!(table_exists(&adapter, "disposable").await);
    assert_eq!(row_count(&adapter, "disposable").await, 3);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_truncate_previews_the_table_the_same_way() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&on("disposable", Operation::Truncate))
        .await
        .unwrap();

    assert!(matches!(preview.body(), PreviewBody::Table(_)));
    assert_eq!(preview.affected_count(), 3);
    assert_eq!(row_count(&adapter, "disposable").await, 3);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn an_empty_table_previews_as_affecting_nothing() {
    reset_seed().await;
    let adapter = adapter().await;

    // Empty it first, then preview a drop of the empty table.
    let preview = adapter
        .build_preview(&on("disposable", Operation::Truncate))
        .await
        .unwrap();
    adapter.execute(preview.approve()).await.unwrap();

    let preview = adapter
        .build_preview(&on("disposable", Operation::DropTable))
        .await
        .unwrap();
    assert!(preview.affects_nothing());
    assert_eq!(preview.affected_count(), 0);

    reset_seed().await;
}

// --- execution ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_confirmed_truncate_empties_the_table_but_keeps_it() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&on("disposable", Operation::Truncate))
        .await
        .unwrap();
    let outcome = adapter.execute(preview.approve()).await.unwrap();

    assert_eq!(
        outcome.rows_affected, 3,
        "the count reported must be the one the user was shown, not the zero \
         Postgres reports for a DDL statement"
    );
    assert!(
        table_exists(&adapter, "disposable").await,
        "a truncate keeps the table"
    );
    assert_eq!(row_count(&adapter, "disposable").await, 0);

    reset_seed().await;
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_confirmed_drop_removes_the_table_entirely() {
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&on("disposable", Operation::DropTable))
        .await
        .unwrap();
    let outcome = adapter.execute(preview.approve()).await.unwrap();

    assert_eq!(outcome.rows_affected, 3);
    assert!(!table_exists(&adapter, "disposable").await);
    // Other tables are untouched.
    assert!(table_exists(&adapter, "users").await);

    reset_seed().await;
}

// --- the guard holds for these operations too ---

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_drop_of_a_table_that_does_not_exist_fails_at_preview() {
    reset_seed().await;
    let adapter = adapter().await;

    assert!(matches!(
        adapter
            .build_preview(&on("no_such_table", Operation::DropTable))
            .await,
        Err(AdapterError::Query { .. })
    ));
    assert!(table_exists(&adapter, "users").await);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_drop_the_database_refuses_leaves_everything_standing() {
    // orders references users, so dropping users is refused. The transaction
    // rolls back and both tables survive intact.
    reset_seed().await;
    let adapter = adapter().await;

    let preview = adapter
        .build_preview(&on("users", Operation::DropTable))
        .await
        .unwrap();
    let result = adapter.execute(preview.approve()).await;

    assert!(matches!(result, Err(AdapterError::Query { .. })));
    assert!(table_exists(&adapter, "users").await);
    assert_eq!(row_count(&adapter, "users").await, 7);
}

#[tokio::test]
#[ignore = "requires the Docker Postgres instance; run with --ignored"]
async fn a_filter_is_ignored_rather_than_narrowing_a_truncate() {
    // The parser refuses a condition on a schema operation. Should one reach
    // the adapter anyway, a truncate must still be understood as emptying
    // the whole table, never as a partial delete.
    reset_seed().await;
    let adapter = adapter().await;

    let with_filter = Intent {
        filter: Filter {
            conditions: vec![Condition {
                column: "id".to_string(),
                comparison: Comparison::Equals,
                value: Value::Integer(1),
            }],
        },
        ..on("disposable", Operation::Truncate)
    };

    let preview = adapter.build_preview(&with_filter).await.unwrap();
    assert_eq!(
        preview.affected_count(),
        3,
        "the preview must report every record in the table, since that is \
         what a truncate removes"
    );

    adapter.execute(preview.approve()).await.unwrap();
    assert_eq!(row_count(&adapter, "disposable").await, 0);

    reset_seed().await;
}
