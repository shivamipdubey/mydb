# Confirmation Workflow

This is the core safety loop. Every write operation, on every engine, in every phase, goes through this exact sequence. No exceptions, no shortcuts, no engine-specific skip.

## Sequence
1. User enters a command, typed or spoken.
2. The parser returns a structured intent: engine, target, operation, filter or payload.
3. If the intent is ambiguous, follow docs/10-nlp-voice-and-local-model.md's fallback ladder before continuing.
4. If the operation is a read, run it directly and show the result. Stop here; reads do not need confirmation.
5. If the operation is a write, build and run the adapter's preview (docs/04-database-adapters.md). Show the user exactly what will be affected: matching rows or documents, or the row or document to be created.
6. If the target connection is flagged production, apply the extra step from docs/11-production-safety-flag.md before the confirm button is enabled.
7. Present three options: confirm, edit, cancel.
8. On edit, let the user change the filter, the payload, or the target, and return to step 5 with the new intent.
9. On cancel, discard the intent, log nothing beyond an optional cancelled-command note.
10. On confirm, run the adapter's execute operation. Before running, capture full before-state for the audit log and recovery bin (docs/07-audit-log-and-recovery-bin.md, stored per docs/15-data-model.md). After running, log the after-state and store the recovery entry. Capture the before-state inside the write's own transaction, not in a separate statement beforehand: a gap between reading the records and writing over them means the recovery store could hold something that was never what the write actually changed. Persist it after the write completes, never before, since a log entry for something that did not happen is worse than no entry at all (docs/16-security-and-cybersafety-checklist.md item 7). A write the engine refuses is recorded as a failure with nothing captured, because the transaction takes the capture back with it.
11. If executing a batch across multiple connections, run them one at a time, in the order the user specified. If one fails partway, stop, leave the already-completed ones untouched, and ask the user only about the remaining, unrun ones. Do not attempt automatic rollback.

## What breaks this workflow
- Any code path that executes a write without first showing a preview from the correct adapter.
- Any code path that treats a "confirm all" batch shortcut as skipping the per-item preview; each item in a batch still needs its own preview shown, even if the user can confirm the batch in one action after reviewing all of them.
- Any silent retry of a failed write without re-showing the preview.

## How the sequence is enforced
The sequence is expressed as types, not as a status field, because a status field can be ignored.

Submitting a command returns either a read result or a pending write. A pending write is only ever produced by a successful preview, and is consumed by whichever of confirm, edit, or cancel the user chooses. Confirm is the only one that can execute, and it consumes the pending write, so one confirmation cannot be replayed into two executions. Edit returns a fresh pending write built from a fresh preview of the revised intent, so an edited command is looked at again before anything happens.

A failed preview returns an error and no pending write at all. Step 7's rule that a failed preview must stop the workflow before confirm is therefore not a check anyone has to perform; there is simply nothing to confirm.

## Testing requirement
docs/18-testing-strategy.md requires a test for every adapter that proves a write cannot execute without a prior preview call in the test's call log.
