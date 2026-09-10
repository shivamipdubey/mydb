# Product Requirements Document

## Problem
Managing a database usually means writing exact query syntax, and running a destructive command carries the risk of an irreversible mistake. Different databases use different query languages, which raises the barrier further.

## Who this is for
Individual developers and small teams who work with more than one database and want a safer, faster way to inspect and change data. Each person downloads and runs their own copy. There is no MYDB-run backend or shared account system.

## Core promise
Type or speak a request in plain language. MYDB shows you exactly what it understood, and exactly what will change, before anything happens. You confirm or edit. Only then does it run.

## Feature list

### Must have, phase 1 and 2
- Natural language to query, for read and write operations
- Preview step before every write: matching records shown, using the engine's own read equivalent
- Explicit confirm step, with an edit option, before execution
- Support for Postgres first, then the other major engines (MySQL, SQLite, MongoDB, and others from docs/04-database-adapters.md)
- Audit log: every executed command, summary of what changed, timestamp
- Recovery bin: deleted or overwritten data recoverable for 30 days, then purged
- Production flag on a connection, requiring an extra confirmation step (type the affected record count, or type CONFIRM) before a destructive command runs against it

### Must have, phase 3 and 4
- Available on Windows, Mac, and Linux
- Opt-in local scan that finds databases already running on the user's machine
- Encrypted local credential vault, with a manual recovery phrase the user stores themselves
- Per-database roles: a user account can be admin on one connection and viewer on another

### Must have, phase 5 and 6
- Local, on-device language model for parsing commands, sized automatically to the user's hardware, with a manual override
- Voice input, routed through the same preview and confirm step as typed commands
- Three-step fallback for unclear commands: best guess shown for confirm or edit, a short list of alternatives if confidence is low, a clarifying question if neither resolves it
- LAN sharing: one machine hosts a session behind a password, others connect using the host's IP and port
- Dashboard showing every connected database, its connection health, and basic stats (size, table or collection count, last activity)

## Explicit non-goals for v1
- No MYDB-hosted backend, account system, or cloud sync
- No automatic rollback of an already-completed multi-database operation; the recovery bin is the recovery path instead
- No billing or subscription system (v1 is free to download and use)

## Success criteria
A user can connect to a real database, ask in plain language for a change, see exactly what will be affected, confirm it, and later recover anything deleted by mistake within the 30-day window, without writing a line of query syntax.
