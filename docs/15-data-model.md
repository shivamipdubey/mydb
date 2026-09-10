# Data Model (MYDB's own internal storage)

This describes what MYDB stores about itself, not the data inside a user's connected databases.

## connections
id, name, engine type, connection details reference (points to vault entry), production flag, created at, last health check result.

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
