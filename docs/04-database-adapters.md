# Database Adapters

Each supported engine gets its own adapter. An adapter must implement four operations: connect, describe schema, build a preview for a given write intent, execute a confirmed write. Never assume one engine's syntax works for another.

## Postgres and MySQL (and other SQL engines)
- Preview for UPDATE or DELETE: run the equivalent SELECT with the same WHERE clause, return the matching rows.
- Preview for INSERT: show the exact row that will be created, no read needed against existing data.
- Preview for schema changes (DROP TABLE, ALTER TABLE): show the current schema and row count of the affected table. Phase 1 implements DROP TABLE and TRUNCATE only; ALTER TABLE is backlogged to phase 2 (docs/03-phases-roadmap.md), so the rule above is specified but not yet built for it.
- Transactions: wrap the real write in a transaction where the engine supports it, so a mid-statement failure does not leave a partial row change.

### Connection details
Set connection parameters on the driver's own configuration builder; never format them into a connection string. A password containing a space, quote, or equals sign would corrupt a hand-built string, and the resulting parse error can carry fragments of the credential into a message the user sees. This is the same reasoning as docs/16-security-and-cybersafety-checklist.md item 3, applied to connecting rather than querying.

### How the preview requirement is enforced
The preview is not a step an adapter is trusted to call. `execute` accepts only an `ApprovedWrite`, which can be produced solely by consuming a `Preview`, and a `Preview` has private fields and a crate-private constructor, so the only way to obtain one is for an adapter to have actually run a preview against the database. The intent that executes is read back out of the preview rather than passed alongside it, so the statement that runs is necessarily the one the user was shown.

Executing without a preview is therefore not a mistake to catch in review. It does not compile. Any new engine adapter inherits this by implementing the same trait.

The preview's WHERE clause and the write's WHERE clause are produced by the same function from the same filter, so they cannot drift apart and show one set of rows while changing another.

### A preview cannot predict a constraint
A preview reports which records match. It does not know whether the engine will accept the write: a foreign key, a check constraint, or a trigger can still refuse it. When that happens the transaction rolls back, nothing changes, and the user sees a typed error. This is a known and accepted limit, not a gap in the preview.

### What is implemented for Postgres
Phase 1: connect, describe schema, and a read-only health check. Preview and execute for DELETE arrive in T7, INSERT and UPDATE in T10, DROP TABLE and TRUNCATE in T11. The adapter interface deliberately does not expose an execute function until the preview guard that governs it exists, so there is no window in which an adapter can execute without one.

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
