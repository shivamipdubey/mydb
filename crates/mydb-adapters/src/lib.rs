//! One adapter per database engine, each implementing the same interface:
//! connect, describe schema, build preview, execute, report health
//! (docs/02-architecture.md, docs/04-database-adapters.md).
//!
//! No adapter reaches into another adapter's code, and engine-specific naming
//! stays inside that engine's module (docs/17-coding-standards.md).
//! Postgres arrives in T5; its preview and execute in T7.
