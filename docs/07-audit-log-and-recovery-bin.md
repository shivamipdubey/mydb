# Audit Log and Recovery Bin

## Audit log
- Every executed write is logged: timestamp, connection, operation type, the intent that produced it, and a result summary.
- Small operations (below a configurable row or document threshold, default 1000) get full before and after state in the log itself.
- Large operations get a summary (row count, a sample of affected records, and a reference pointing to the recovery bin entry that holds the full detail). Do not duplicate full detail in both the log and the recovery bin.
- The audit log is append-only. Nothing in it is ever edited or deleted by MYDB itself, including during a purge; only the recovery bin purges on its own schedule.

## Recovery bin
- Holds the full before-state of anything deleted or overwritten, regardless of operation size.
- Retention is 30 days from the time of the operation, then permanent purge. This is fixed for v1; do not make it configurable without checking docs/01-prd.md first for whether that has since changed.
- Size-capped per connection, with a setting the user can raise or lower. If the cap is hit, the oldest entries purge early, and the user is warned.
- Restoring from the recovery bin is a new, explicit write operation. It goes through the same confirmation workflow as any other write (docs/05-confirmation-workflow.md); restoring is not a silent undo.

## What is never logged or stored
Vault passphrases, recovery phrases, or decrypted credentials never appear in the audit log or the recovery bin, under any circumstance.
