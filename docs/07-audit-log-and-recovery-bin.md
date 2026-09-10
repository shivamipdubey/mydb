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
- Retention is 30 days from the time of the operation, then permanent purge. This is fixed for v1; do not make it configurable without checking docs/01-prd.md first for whether that has since changed. The deadline is stored on the entry when it is created rather than computed on each read, so an entry's window cannot move if the retention rule ever changes. The clock is injected rather than read directly, because docs/18-testing-strategy.md requires proving expiry with a manipulated clock rather than a 30-day wait, and a retention rule that can only be tested by waiting a month is one nobody tests.
- Size-capped per connection, with a setting the user can raise or lower. If the cap is hit, the oldest entries purge early, and the user is warned. Oldest first, because the newest entry is the one most likely to still be wanted. The cap is measured in bytes of stored payload and is per connection, so one busy connection cannot purge another's history.

### What purging means
Purging clears an entry's payload and marks it with the date, keeping the row. It is not a deletion.

Two reasons. For a large operation the audit log holds a reference to the recovery entry, and that reference must resolve forever, answering "this existed and was purged on this date" rather than pointing at nothing. And recording the loss any other way would mean editing an audit entry, which this document forbids and the audit log's triggers physically prevent.

What survives a purge is the description: which connection, which operation, the intent, how many records, when it was created, when it was purged, and whether it went early because of a cap rather than because it expired. What goes is the data itself. A purged entry reports that it cannot be restored from, rather than appearing restorable and failing later.

Purged entries are excluded from the bin's listing unless explicitly asked for, since the bin is a place to recover from and these cannot be recovered from.

### Capturing something too large to hold in memory
A before-state larger than available memory streams into a staging area while the write's transaction is open, and becomes a real expiring entry only once the write has committed. If the write fails or rolls back, the staged records are discarded: they describe something that never happened, and an entry for that would be worse than no entry.

There is no row limit on a capture. The per-connection byte cap is what bounds the bin, which is the setting the user controls.

Staged records are not recovery data and are never offered as such. Anything left staged by a process that died mid-write is swept at startup, because a write whose outcome is unknown must not be presented as recoverable.
- Restoring from the recovery bin is a new, explicit write operation. It goes through the same confirmation workflow as any other write (docs/05-confirmation-workflow.md); restoring is not a silent undo.

## What is never logged or stored
Vault passphrases, recovery phrases, or decrypted credentials never appear in the audit log or the recovery bin, under any circumstance.
