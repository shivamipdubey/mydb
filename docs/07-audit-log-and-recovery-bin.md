# Audit Log and Recovery Bin

## Audit log
- Every executed write is logged: timestamp, connection, operation type, the intent that produced it, and a result summary.
- Small operations (below a configurable row or document threshold, default 1000) get full before and after state in the log itself.
- Large operations get a summary (row count, a sample of affected records, and a reference pointing to the recovery bin entry that holds the full detail). Do not duplicate full detail in both the log and the recovery bin.
- The audit log is append-only. Nothing in it is ever edited or deleted by MYDB itself, including during a purge; only the recovery bin purges on its own schedule.

### How state is captured
Inside the write's own transaction, before the write, and persisted only once the write has completed. The reasoning is in docs/05-confirmation-workflow.md step 10.

What each operation captures:

- Delete: the records it removes. The after-state is empty, which is the truth rather than a gap.
- Insert: nothing before, since nothing existed; afterwards, the record as it actually landed, including any defaults the engine filled in, rather than only the values the command named.
- Update: both sides. The after-state is read back by primary key, never by the filter, because the filter may no longer match: "set active to false where active is true" matches nothing once it has run, and re-reading by filter would record that the records had disappeared.
- Schema change: every record in the table, before it goes.

An update on a table with no single-column primary key has no after-state. There is nothing to match the changed records back by, and a composite key or no key at all mean the same thing here. That is recorded as unavailable rather than as empty, because an empty after-state would read as "the records vanished".

Captured values keep their own types. The preview casts everything to text because it exists to be read; a capture must not, because it is the only record of data that may no longer exist.

## Recovery bin
- Holds the full before-state of anything deleted or overwritten, regardless of operation size.
- Retention is 30 days from the time of the operation, then permanent purge. This is fixed for v1; do not make it configurable without checking docs/01-prd.md first for whether that has since changed.
- Size-capped per connection, with a setting the user can raise or lower. If the cap is hit, the oldest entries purge early, and the user is warned.
- Restoring from the recovery bin is a new, explicit write operation. It goes through the same confirmation workflow as any other write (docs/05-confirmation-workflow.md); restoring is not a silent undo.

## What is never logged or stored
Vault passphrases, recovery phrases, or decrypted credentials never appear in the audit log or the recovery bin, under any circumstance.
