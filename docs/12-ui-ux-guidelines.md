# UI and UX Guidelines

## Core screens
Each screen names the phase it ships in, per docs/03-phases-roadmap.md. A screen is not built before its phase.

- Command bar (phase 1): where typed input is entered, always visible. Voice input is added to this same bar in phase 5.
- Preview screen (phase 1): shows the parsed intent in plain language, the matching records or documents, and confirm, edit, and cancel actions.
- Connection manager (phase 1): add, edit, remove connections, and set the production flag. Managing vault entries is added here in phase 4; the opt-in local discovery scan is added here in phase 3.
- Recovery bin viewer (phase 2): browse and restore anything within its 30-day window. Depends on the recovery bin itself, built in phase 2.
- Audit log viewer (phase 2): browse past commands, filterable by connection, date, and operation type. Phase 1 has only a basic command history, not this screen.
- Dashboard (phase 5): lists every connected database, its health, and basic stats (docs/13-dashboard-and-health-monitoring.md). docs/01-prd.md lists it under phase 5 and 6; docs/03-phases-roadmap.md has no entry for it, so treat phase 5 as its home until the roadmap says otherwise.

## Layout principle
The interface should be structured so panels and sections can be rearranged or added without a full rebuild. Treat the dashboard and preview screen as a set of independent panels (connection list, health panel, recent activity, command bar) rather than one fixed layout, so new panels can be added in later phases without redesigning the whole screen.

## Confirmation screen requirements
- The parsed intent must be shown in plain language, not just raw query syntax, though raw syntax can be shown as a secondary, expandable detail.
- Affected records must be visibly listed or clearly counted, never just implied.
- The confirm button is disabled until any required extra steps (production flag, role check) are satisfied.

## Accessibility
- Every action reachable by typing must also be reachable without voice, and vice versa where feasible.
- Color alone must never be the only signal for a production-flagged connection or a pending destructive action; use a label or icon alongside color.
