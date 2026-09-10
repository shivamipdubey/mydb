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

## Where each suite runs
Continuous integration builds and tests on Windows, macOS, and Linux on every commit, using standard GitHub-hosted runners. Development happens on macOS only, so the matrix exists to catch breakage on the other two platforms as it happens rather than at phase 3.

- Unit tests (Rust and frontend), lint, and the full build: all three platforms.
- Integration tests against containerized Postgres: Linux only. GitHub-hosted macOS runners have no Docker daemon and Windows runners only run Windows containers, so a Postgres service container cannot start on either. Adapter integration correctness is therefore verified on Linux in CI and on the developer's machine locally.
- End-to-end UI tests through `tauri-driver`: Linux only. WKWebView exposes no WebDriver interface, so Tauri's driver cannot run on macOS at all. This is a known cost of the stack chosen in docs/02-architecture.md.
- End-to-end frontend tests with the Tauri command bridge mocked: all three platforms. These cover the screen-level flow anywhere, including macOS, but they do not exercise the real backend.
- The full command-to-confirm-to-execute flow against real Postgres on macOS is verified manually, which docs/25-exit-conditions-definition-of-done.md requires for phase 1 regardless.

A dependency audit job runs `cargo audit` and `npm audit` per docs/16-security-and-cybersafety-checklist.md, item 6.

## The Postgres test instance
`docker-compose.yml` runs Postgres locally, pinned by image digest rather than tag so a retagged upstream image cannot silently change what tests run against. CI uses the same digest as a service container. Control it with `npm run db:up`, `db:reset`, and `db:down`.

`testing/seed.sql` holds a small, exact fixture: seven users and four orders, with values chosen so a filter's expected result can be asserted literally. It is re-runnable, so any test that mutates data restores a known state rather than depending on execution order.

Integration tests are marked `#[ignore]` so `cargo test` remains runnable without Docker, and are run with `npm run test:integration`. They run single-threaded: they share one database instance and reset it to the seed state, so running them concurrently would have them overwrite each other's fixtures. They are never skipped silently: an ignored test reports as ignored, CI runs them explicitly, and the CI job fails if no integration test actually executed, so a suite that quietly stops running cannot be mistaken for a passing one.
