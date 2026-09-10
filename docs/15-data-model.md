# Data Model (MYDB's own internal storage)

This describes what MYDB stores about itself, not the data inside a user's connected databases.

## connections
id, name, engine type, connection details reference (points to vault entry), production flag, created at, last health check result.

### Phase 1 shape
Phase 1 has no vault and no dashboard, so the stored record is: id, name, engine, host, port, database, username, password, production flag. The password is held in plaintext, which docs/06-credential-vault.md permits until phase 4 and forbids describing as a vault. Created at and last health check result are omitted until something consumes them.

Storage is a versioned JSON file at the user's config directory, `MYDB/connections.json`. Writes are atomic (temporary file, then rename) so an interrupted save cannot truncate the list, and the file is set to owner-only permissions on Unix. Neither measure is encryption; they limit damage while the credentials are plaintext. Phase 4 replaces the credential handling here with the real vault.

## vault_entries
id, connection id, encrypted credential blob, key reference. Never a plaintext field of any kind.

## roles
id, connection id, user identifier, role (admin or viewer). One row per user per connection.

## command_history (phase 1 only)
Phase 1 has no audit log. It has a basic command history (docs/03-phases-roadmap.md) holding five fields per executed write: recorded at, connection, operation type, intent summary, result.

Stored as one JSON object per line in `command-history.jsonl`, in the same directory as the connection list. One object per line rather than a JSON array, because appending to an array means rewriting the file, which is the one thing an append-only log should never do. It also means the file can be read with `cat` while there is no viewer screen.

What it deliberately does not hold: before or after state, affected row data, or any restore action. Storing a driver's error message is also avoided, because a database error can carry row values inside it, such as the key value in a duplicate-key violation, and that would smuggle row data into a store that must not have any. A failure records only that it failed.

Written only after an operation has been attempted, and for both outcomes: a refused write is recorded as a failure, never as a success. A cancelled command, a command refused before reaching the database, and any read leave no entry at all.

The `audit_log` and `recovery_bin` below replace this in phase 2. This is not a smaller version of them, and nothing should treat it as one.

## audit_log
id, connection id, timestamp, operation type, intent summary, before state (full or reference), after state (full or reference), result (success or failure), size tier (small or large).

### As built
A table in a local SQLite database at `MYDB/mydb.sqlite3`, sharing one file with the recovery bin so an entry's reference to a recovery entry is a real foreign key rather than a number that might point at nothing. Schema changes go through a versioned migration, because this file holds the only copy of data a user may later need to recover.

Columns: id, connection id, connection name, recorded at, operation, intent summary, result, size tier, affected count, before state, after state, state sample, recovery entry id, predates state capture.

Three of those need explaining:

- **connection name** is a snapshot taken when the write ran. docs/15's rule below says entries detach on connection deletion rather than disappearing, and the snapshot is how an entry still reads sensibly afterwards. Whether the connection still exists is decided at read time, never written into the entry, because recording it would mean editing an entry that docs/07 says is never edited.
- **state sample** and **recovery entry id** are populated only for a large operation, where docs/07 puts full detail in the recovery bin and a sample plus a reference here. For a small operation, before state and after state hold everything and these are empty. Full detail is never in both.
- **predates state capture** marks the rows imported from the phase 1 command history, which had no way to hold state. Flagged rather than dropped, so the record of what a user did stays continuous and nobody mistakes an old entry's absent state for a capture that failed.

Append-only is enforced by triggers in the database, not by the discipline of the code that writes to it. An UPDATE or DELETE against `audit_log` aborts, so a future code path that tries cannot succeed whatever it intended. This is what makes docs/07's "nothing in it is ever edited or deleted by MYDB itself, including during a purge" true rather than merely intended.

### Captured state
Before and after state are stored as engine-neutral JSON: a set of named values per record, with each value keeping its own shape. A SQL row becomes an object of column name to value; a MongoDB document already is one, nesting included. A rectangular grid of strings was rejected deliberately, because it would flatten a document into a shape it never had, and the recovery bin is the only record of data that no longer exists.

## recovery_bin
id, connection id, audit_log id reference, full before state, created at, expires at (created at plus 30 days), purged flag.

### As built
A table in the same local database as the audit log, so the reference between them is a real foreign key. Columns: id, connection id, connection name, audit_log id, operation, intent summary, created at, expires at, payload, record count, payload bytes, purged, purged at, purged early.

Unlike `audit_log`, this table is mutable, and deliberately so: purging is defined as clearing a payload and marking the row, not as removing it (docs/07-audit-log-and-recovery-bin.md). The reverse reference is filled in after the audit entry exists, because the audit log is append-only and cannot be updated to add it later.

`connection name` is a snapshot, for the same reason as the audit log's. An entry outlives the connection it came from, and its 30-day window runs its normal course either way; nothing purges an entry early because its connection was deleted.

`payload bytes` is kept after a purge so a connection's cap can be reasoned about historically. `purged early` distinguishes a cap-driven purge from an expiry, which docs/07 requires warning the user about.

## recovery_staging
staging id, record. Not part of docs/15's original list, because it holds nothing durable: records stream here while a write's transaction is open and are either finalised into a recovery entry or discarded. Anything left behind by a process that died mid-write is swept at startup, since a write whose outcome is unknown must not be offered as recoverable data.

## local_model_settings
selected model size, detected hardware summary, override flag, download status.

## lan_sessions (phase 6)
host connection id, port, session start time, connected user identifiers, session end time.

## Rules for this schema
- No table here ever stores a decrypted credential, a vault passphrase, or a recovery phrase.
- Every table that references a connection must cascade-handle connection deletion explicitly (decide per table whether deletion removes history or just detaches it; do not leave this undefined).
