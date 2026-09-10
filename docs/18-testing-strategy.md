# Testing Strategy

## What must be tested for every adapter
- Connect succeeds against a real or containerized instance of the engine.
- Preview returns the correct matching records for a given filter.
- Execute only runs after a preview call has happened in the same command flow (test this by asserting the call order, not just the final state).
- A failed preview blocks execute from ever being called.

## What must be tested for the confirmation workflow
- Confirm, edit, and cancel each produce the correct next state.
- Edit returns to the preview step with the updated intent, not straight to execute.
- A production-flagged connection keeps the confirm button disabled until the extra step is completed.
- A viewer-role user cannot reach a working confirm action, tested at the adapter level, not just the UI level.

## What must be tested for the audit log and recovery bin
- Every executed write produces exactly one audit log entry.
- A deleted or overwritten row appears in the recovery bin with correct before-state.
- A recovery bin entry expires and purges after 30 days (test with a manipulated clock, not a real 30-day wait).
- Restoring from the recovery bin goes through the confirmation workflow, not a direct write.

## What must be tested for the language layer
- A high-confidence command produces a single best-guess preview.
- A medium-confidence command produces a list of alternatives.
- A low-confidence command produces a clarifying question, not a guess.
- Voice input, once transcribed, follows the identical path as the same text typed directly.

## What must be tested for security
- Every item in docs/16-security-and-cybersafety-checklist.md that can be expressed as an automated test should be, so a regression fails a test run instead of surfacing only in manual review.

## Test types
Use unit tests for individual adapter functions and the confirmation state machine. Use integration tests against real or containerized database instances for adapter correctness. Use end-to-end tests for the full command-to-confirm-to-execute flow on the UI.
