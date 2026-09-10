# Database Adapters

Each supported engine gets its own adapter. An adapter must implement four operations: connect, describe schema, build a preview for a given write intent, execute a confirmed write. Never assume one engine's syntax works for another.

## Postgres and MySQL (and other SQL engines)
- Preview for UPDATE or DELETE: run the equivalent SELECT with the same WHERE clause, return the matching rows.
- Preview for INSERT: show the exact row that will be created, no read needed against existing data. Show every column of the table, not only the ones named: an unnamed column is where a default or a null will land, and someone checking whether the new record is right needs to see that. Reading the table's shape is not reading its data.
- Preview for schema changes (DROP TABLE, ALTER TABLE): show the current schema and row count of the affected table. Phase 1 implements DROP TABLE and TRUNCATE only; ALTER TABLE is backlogged to phase 2 (docs/03-phases-roadmap.md), so the rule above is specified but not yet built for it.
- Transactions: wrap the real write in a transaction where the engine supports it, so a mid-statement failure does not leave a partial row change.

### Typing a filter's parameters
Bind every filter parameter according to the target column's real type, read from the schema. Do not bind a value from its own apparent type and hope the engine resolves it.

The reason is concrete. A Rust `i64` bound against an `integer` column fails outright, because `int8` is not `int4` and the driver will not guess. A date bound as text needs an explicit cast, because Postgres infers a parameter's type from its comparison. These look like separate bugs and are one bug: the query builder not using the column types it already has.

The rule follows from that: the builder is given the table's columns, not just the filter, and each parameter is bound natively where an exact type exists, cast from text where it does not, and compared as text for anything exotic. Casting is applied to the parameter, never the column, so an index on the column can still be used.

### Matching text
Compare text columns case-insensitively for equality and inequality. Someone typing a command in plain language should not have to guess the capitalisation the database happens to store.

Fold case in the comparison only. The stored value and the value the user typed are both left exactly as they are; a preview shows the row's real capitalisation even when it was found by typing something different. Ordering comparisons keep the database's own collation, since whether two names are the same name is a different question from how they sort.

### Connection details
Set connection parameters on the driver's own configuration builder; never format them into a connection string. A password containing a space, quote, or equals sign would corrupt a hand-built string, and the resulting parse error can carry fragments of the credential into a message the user sees. This is the same reasoning as docs/16-security-and-cybersafety-checklist.md item 3, applied to connecting rather than querying.

### How the preview requirement is enforced
The preview is not a step an adapter is trusted to call. `execute` accepts only an `ApprovedWrite`, which can be produced solely by consuming a `Preview`, and a `Preview` has private fields and a crate-private constructor, so the only way to obtain one is for an adapter to have actually run a preview against the database. The intent that executes is read back out of the preview rather than passed alongside it, so the statement that runs is necessarily the one the user was shown.

Executing without a preview is therefore not a mistake to catch in review. It does not compile. Any new engine adapter inherits this by implementing the same trait.

The preview's WHERE clause and the write's WHERE clause are produced by the same function from the same filter, so they cannot drift apart and show one set of rows while changing another.

### A preview cannot predict a constraint
A preview reports which records match. It does not know whether the engine will accept the write: a foreign key, a check constraint, or a trigger can still refuse it. When that happens the transaction rolls back, nothing changes, and the user sees a typed error. This is a known and accepted limit, not a gap in the preview.

### Writing values
An insert or update binds its values through the same typing rules as a filter, against the same column types, so a preview and the write it describes cannot disagree about how a value is handled.

One difference matters: case folding belongs to comparisons only. A filter matches text case-insensitively, but an insert or update writes exactly the value the user typed. Folding case on the way in would quietly rewrite the user's data.

A column whose type MYDB cannot type a parameter for can still be read and compared as text, but not written. Writing it is refused by name, before any preview, since a preview the user could confirm and then have fail is worse than an early refusal.

### What is implemented for Postgres
Phase 1: connect, describe schema, a read-only health check, and preview plus execute for DELETE, INSERT, and UPDATE. DROP TABLE and TRUNCATE arrive in T11. The adapter interface deliberately does not expose an execute function until the preview guard that governs it exists, so there is no window in which an adapter can execute without one.

The health check reads the server version. docs/13-dashboard-and-health-monitoring.md requires it to be lightweight and strictly read-only, never something that could be mistaken for a data-changing operation.

## SQLite
- Same pattern as Postgres and MySQL. Note SQLite has weaker concurrent-write guarantees; surface a warning if another process holds a lock.

## MongoDB
- Preview for updateMany, deleteMany, or similar: run the equivalent find() with the same filter, return matching documents.
- Preview for insertOne or insertMany: show the exact document(s) to be created.
- Transactions: only available on replica sets; if the connection is a standalone instance, tell the user multi-document atomicity is not available before they confirm a multi-document write.

## Adding a new engine
1. Add a section to this file describing its read equivalent for each write operation type.
2. State explicitly whether it supports transactions, and under what configuration.
3. Do not enable the engine in the UI until its adapter passes the tests in docs/18-testing-strategy.md.
4. Get user confirmation before adding an engine not already listed here, per CLAUDE.md rule 2.

## Adapters not yet specified
Any engine mentioned in docs/01-prd.md but without a section above is not implemented yet. Do not write code against it from assumption; add the section first.
