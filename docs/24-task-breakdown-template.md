# Task Breakdown Template

Use this to turn a phase from docs/03-phases-roadmap.md into ordered, workable tasks.

## Per-task fields
- Task name: short, specific.
- Phase: which phase this belongs to.
- Depends on: which earlier tasks must be done first.
- What to build: the concrete change, referencing the specific doc section it implements.
- Preview requirement: if the task touches a write path, name the exact preview behavior it must produce, referencing docs/05.
- Documentation to update: list the specific files from docs/19-documentation-rules.md this task will touch.
- Tests required: list the specific test cases from docs/18-testing-strategy.md this task must satisfy.
- Security check: name which items in docs/16-security-and-cybersafety-checklist.md apply.
- Done means: a short, checkable statement, not a vague description.

## Example
Task name: Postgres adapter, DELETE preview.
Phase: 1.
Depends on: Postgres adapter connect and describe schema.
What to build: buildPreview function that converts a DELETE intent's filter into an equivalent SELECT, returns matching rows.
Preview requirement: matching rows shown before any DELETE can execute.
Documentation to update: docs/04-database-adapters.md (confirm the section already covers this, update if the implementation reveals a gap).
Tests required: preview returns correct rows for a given filter; execute cannot run without a prior preview call in the same flow.
Security check: item 3 (parameterized queries), item 1 (no write without confirmation).
Done means: a DELETE command against Postgres shows the correct matching rows and cannot execute until confirmed.
