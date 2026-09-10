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

## What phase 1 built
The shell renders panels in a column: connections, command entry, and, when there is one, a result or a preview. Each is an independent component, so a later phase adds a panel without touching the others.

Decisions worth keeping:

- A SQL NULL renders as a distinct marker, not an empty cell. A user judging whether a preview matches what they meant needs to tell an absent value from a blank one.
- A filter that matches the whole table gets its own warning line, separate from the record count. An unfiltered delete is the most damaging thing a user can confirm by accident and should never look like an ordinary filtered one.
- A preview matching zero records says so in plain words rather than showing an empty table, because that usually means the filter is wrong.
- When more records are affected than are listed, the interface says the count above is exact. The sample is capped; the count never is.
- Editing a command produces a fresh preview screen rather than reusing the previous one, so a revised command is never shown alongside the earlier command's state.
- The confirm button carries the destructive styling; edit and cancel are secondary. Cancel states plainly that nothing was changed.
- A drop is shown with its table's structure and a warning that the table itself goes, not only its records. Someone reading the record count alone would miss half of what is lost. A truncate shows the same structure but says the table remains.
- On a production-flagged connection, the extra step's field is labelled for what it wants ("Table name", "Record count, or CONFIRM"), and the confirm button stays disabled until it is satisfied. A wrong entry leaves the preview on screen rather than discarding it.
- Buttons are named for what they act on, "Edit command" against "Edit connection", so no two controls on screen share a label.

## Accessibility
- Every action reachable by typing must also be reachable without voice, and vice versa where feasible.
- Color alone must never be the only signal for a production-flagged connection or a pending destructive action; use a label or icon alongside color.
