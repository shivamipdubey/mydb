# Database Adapters

Each supported engine gets its own adapter. An adapter must implement four operations: connect, describe schema, build a preview for a given write intent, execute a confirmed write. Never assume one engine's syntax works for another.

## Postgres and MySQL (and other SQL engines)
- Preview for UPDATE or DELETE: run the equivalent SELECT with the same WHERE clause, return the matching rows.
- Preview for INSERT: show the exact row that will be created, no read needed against existing data.
- Preview for schema changes (DROP TABLE, ALTER TABLE): show the current schema and row count of the affected table. Phase 1 implements DROP TABLE and TRUNCATE only; ALTER TABLE is backlogged to phase 2 (docs/03-phases-roadmap.md), so the rule above is specified but not yet built for it.
- Transactions: wrap the real write in a transaction where the engine supports it, so a mid-statement failure does not leave a partial row change.

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
