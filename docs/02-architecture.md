# Architecture

## Shape of the system
MYDB is a single downloadable desktop application. No component belongs to MYDB's own servers. Everything below runs on the user's machine.

## Tech stack
Chosen in phase 1 and used for every phase after it.

- Shell and packaging: Tauri v2. Small bundle, and the frontend can only reach the backend through explicitly registered commands, which makes docs/17-coding-standards.md's "UI never calls a database driver directly" rule structural rather than conventional.
- Backend: Rust, as a Cargo workspace. One crate per component below, so no module reaches into another's internals. `mydb-core` holds the shared domain types (the Intent) that the others speak in, which is what keeps the parser, adapters, and confirmation engine from depending on each other.
- Frontend: TypeScript and React, built with Vite. Panels are components, per docs/12-ui-ux-guidelines.md's requirement that screens be rearrangeable without a rebuild.
- Tests: `cargo test` for Rust, Vitest for the frontend, and `tauri-driver` for end-to-end runs. Note `tauri-driver` cannot run on macOS, because WKWebView exposes no WebDriver interface; docs/18-testing-strategy.md records how end-to-end coverage is arranged around that.
- Lint: Clippy for Rust, ESLint plus `tsc` for TypeScript. The workspace denies `unwrap`, `expect`, and `panic` in non-test code, enforcing docs/17's typed-result rule at build time.

### Known cost of this choice
Each operating system uses a different system WebView (WKWebView, WebView2, WebKitGTK), so rendering differences are a real risk. Cross-platform CI from phase 1 onward exists to surface those early rather than at phase 3.

## Components
1. Desktop shell — the window, menus, and native OS integration (file dialogs, auto-update check, system tray).
2. UI layer — the screens: command bar, preview screen, dashboard, connection manager, settings.
3. Command parser — turns typed or spoken text into a structured query intent. Calls the local model (docs/10-nlp-voice-and-local-model.md).
4. Adapter layer — one adapter per database engine (docs/04-database-adapters.md). Each adapter implements: connect, describe schema, build a preview read for a given intent, execute a confirmed write, report health.
5. Confirmation engine — takes a structured intent and a preview result, renders it, and blocks execution until the user confirms (docs/05-confirmation-workflow.md).
6. Vault — encrypted local storage for connection credentials (docs/06-credential-vault.md).
7. Audit and recovery store — local storage for the audit log and the 30-day recovery bin (docs/07-audit-log-and-recovery-bin.md).
8. Permissions module — per-database role checks (docs/08-permissions-and-roles.md).
9. LAN host/connect module — opens a local port for sharing, or connects to another machine's open port (docs/09-sharing-and-lan-hosting.md).
10. Discovery module — opt-in local scan for running databases (docs/14-local-database-discovery.md).

## Data flow for a single command
User input reaches the command parser. The parser returns a structured intent (engine, target table or collection, operation, filter). The adapter layer builds a read equivalent of that intent and runs it against the target database. The confirmation engine shows the result to the user. On confirm, the adapter layer runs the real operation, the audit module logs before and after state, and the recovery module stores anything deleted or overwritten.

## Storage on the user's machine
- Vault: encrypted file, unlocked by a local key, recoverable only through the user's own exported recovery phrase.
- Audit log: local database, summary entries only for large operations, full detail for small ones (docs/07-audit-log-and-recovery-bin.md).
- Recovery bin: local database, full row detail, 30-day retention, size-capped and configurable.
- Connection list: local config, references the vault for credentials, never stores plaintext passwords itself.

## What does not exist in v1
No MYDB server, no account system, no telemetry endpoint MYDB controls. If future phases add optional cloud recovery or a hosted browser version, that is a new component added on top of this, not a replacement for it.
