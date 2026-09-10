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

## audit_log
id, connection id, timestamp, operation type, intent summary, before state (full or reference), after state (full or reference), result (success or failure), size tier (small or large).

## recovery_bin
id, connection id, audit_log id reference, full before state, created at, expires at (created at plus 30 days), purged flag.

## local_model_settings
selected model size, detected hardware summary, override flag, download status.

## lan_sessions (phase 6)
host connection id, port, session start time, connected user identifiers, session end time.

## Rules for this schema
- No table here ever stores a decrypted credential, a vault passphrase, or a recovery phrase.
- Every table that references a connection must cascade-handle connection deletion explicitly (decide per table whether deletion removes history or just detaches it; do not leave this undefined).
