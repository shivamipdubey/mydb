# Changelog Conventions

## Format
One entry per change set, newest at the top. Each entry states: date, phase, what changed, why, and which documents were updated alongside it.

## Example entry
2026-09-09, Phase 1: added Postgres adapter's preview function for DELETE operations. Reason: required for the confirmation workflow to show matching rows before execution. Updated docs/04-database-adapters.md.

## Rule
Every entry required by docs/19-documentation-rules.md gets logged here in the same change set. Do not batch several changes into one vague entry; one entry per distinct change.
