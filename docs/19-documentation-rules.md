# Documentation Rules

Apply after every change, before marking a task done.

## Required updates by change type
- New or changed database engine support: update docs/04-database-adapters.md with the engine's preview and transaction rules.
- New or changed safety behavior (confirmation, production flag, roles): update the matching doc among 05, 08, 11.
- New or changed internal storage: update docs/15-data-model.md.
- New or changed UI screen or panel: update docs/12-ui-ux-guidelines.md.
- Any change touching security-sensitive code (vault, network, credentials): update docs/16-security-and-cybersafety-checklist.md if a new check is needed.
- Any change to build order or dependency between phases: update docs/03-phases-roadmap.md.
- Every change, regardless of type: add an entry to the changelog (docs/21-changelog-conventions.md).

## Rule
A task is not complete until its required documentation update is made in the same change set. Do not defer documentation to a later cleanup pass; a phase's exit condition check (docs/25) verifies this directly.

## Style for these documents
Short, direct sentences. State the rule, then the reason if it is not obvious. Avoid restating the same rule in more than one file; link to the authoritative file instead of duplicating its content.
