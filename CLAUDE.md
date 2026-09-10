# MYDB — Instructions for Claude Code

This file is the entry point. Read it first, every session, before writing any code.

## What MYDB is
A downloadable desktop app that lets a user manage databases through typed or spoken natural language. Every command that changes data shows a preview of exactly what will change before it runs. Nothing executes without explicit confirmation.

## Document map
Read the file for the area you are working on before touching that code. Do not skip this.

- docs/01-prd.md — what MYDB does and why, full feature list
- docs/02-architecture.md — components, how they connect, tech stack
- docs/03-phases-roadmap.md — build order, what ships in each phase
- docs/04-database-adapters.md — per-engine connection and preview rules
- docs/05-confirmation-workflow.md — the core safety loop, mandatory for every write
- docs/06-credential-vault.md — key storage and recovery phrase
- docs/07-audit-log-and-recovery-bin.md — logging and 30-day undo storage
- docs/08-permissions-and-roles.md — per-database admin/viewer roles
- docs/09-sharing-and-lan-hosting.md — host and connect over LAN
- docs/10-nlp-voice-and-local-model.md — language parsing and voice input
- docs/11-production-safety-flag.md — extra friction for flagged databases
- docs/12-ui-ux-guidelines.md — interface rules
- docs/13-dashboard-and-health-monitoring.md — connection health view
- docs/14-local-database-discovery.md — opt-in scan for local databases
- docs/15-data-model.md — internal tables MYDB itself stores
- docs/16-security-and-cybersafety-checklist.md — run after every change, no exceptions
- docs/17-coding-standards.md — style, structure, naming
- docs/18-testing-strategy.md — what to test and how
- docs/19-documentation-rules.md — what to update whenever code changes
- docs/20-loop-execution-protocol.md — how you self-check a phase before moving on
- docs/21-changelog-conventions.md — how to record changes
- docs/22-glossary.md — terms used across these documents
- docs/23-risk-register.md — known risks and how each is handled
- docs/24-task-breakdown-template.md — how to turn a phase into ordered tasks
- docs/25-exit-conditions-definition-of-done.md — what "done" means per phase

## Non-negotiable rules
1. Never run a command that changes data without first showing the equivalent read and getting explicit confirmation. This applies to every database engine, every phase.
2. Never invent a database engine's syntax. Check docs/04-database-adapters.md, and if the engine is not listed there, stop and ask the user before adding one.
3. Run docs/16-security-and-cybersafety-checklist.md after every code change, not just at the end of a phase.
4. Update docs/19-documentation-rules.md's required files after every change, before marking a task done.
5. Follow docs/20-loop-execution-protocol.md at the end of every phase. Do not announce a phase complete until its checklist passes.
6. Work one phase at a time, in the order set in docs/03-phases-roadmap.md. Do not start phase N+1 work before phase N's exit conditions pass.

## Commands
Stack is Tauri v2 with a Rust backend and a TypeScript/React frontend (docs/02-architecture.md).

- Install: `npm install` (Rust dependencies resolve on first build)
- Run: `npm run dev`
- Test: `cargo test --workspace && npm test`
- Lint: `cargo clippy --workspace --all-targets && npm run lint`
- Package (macOS): `npm run build`

Requires the Rust toolchain, Node, and Docker (Docker is used only for the Postgres test instance; the app itself never needs a network).
