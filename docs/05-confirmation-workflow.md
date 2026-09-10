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
10. On confirm, run the adapter's execute operation. Before running, capture full before-state for the audit log and recovery bin (docs/07-audit-log-and-recovery-bin.md, stored per docs/15-data-model.md). After running, log the after-state and store the recovery entry. In phase 1 there is no audit log or recovery bin yet, only a basic command history (docs/03-phases-roadmap.md); before-state capture and recovery entries begin in phase 2.
11. If executing a batch across multiple connections, run them one at a time, in the order the user specified. If one fails partway, stop, leave the already-completed ones untouched, and ask the user only about the remaining, unrun ones. Do not attempt automatic rollback.

## What breaks this workflow
- Any code path that executes a write without first showing a preview from the correct adapter.
- Any code path that treats a "confirm all" batch shortcut as skipping the per-item preview; each item in a batch still needs its own preview shown, even if the user can confirm the batch in one action after reviewing all of them.
- Any silent retry of a failed write without re-showing the preview.

## Testing requirement
docs/18-testing-strategy.md requires a test for every adapter that proves a write cannot execute without a prior preview call in the test's call log.
