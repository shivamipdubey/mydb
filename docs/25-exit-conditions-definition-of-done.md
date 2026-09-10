# Exit Conditions and Definition of Done

Run this check at the end of every phase, per docs/20-loop-execution-protocol.md. A phase is not done until every applicable item below passes.

## Universal checks, every phase
1. Every feature listed for this phase in docs/01-prd.md and docs/03-phases-roadmap.md exists and works, including every screen, button, and confirmation step it depends on.
2. Every write path added or touched in this phase goes through the full confirmation workflow (docs/05), with no exceptions found in code review.
3. docs/16-security-and-cybersafety-checklist.md passes against the full diff for the phase, not just the last task.
4. All tests required by docs/18-testing-strategy.md for the areas touched in this phase pass.
5. All documentation updates required by docs/19-documentation-rules.md are present for every change made in this phase.
6. The changelog (docs/21-changelog-conventions.md) has an entry for every change made in this phase.

## Phase-specific checks
- Phase 1: a real command against a real Postgres connection, from typed input to confirmed execution, works end to end, including the production flag path.
- Phase 2: every engine listed in docs/04-database-adapters.md for phase 2 has a working preview and execute path, and the audit log and recovery bin both behave correctly for at least one small and one large operation.
- Phase 3: the app runs and passes phase 1 and 2's checks again on all three operating systems, and the discovery scan finds a locally running database on each.
- Phase 4: the vault encrypts and decrypts correctly, the recovery phrase is generated and verified to work for recovery, and role checks are enforced at the adapter level for both admin and viewer.
- Phase 5: the local model is selected correctly for at least two different hardware profiles, all three ambiguity fallback tiers are demonstrated, and voice input produces the same result as the equivalent typed command.
- Phase 6: a second machine can connect to a hosted session using the correct password, gets rejected with an incorrect one, and receives the role the host assigned.

## If a check fails
Fix the specific failing item, then re-run every check in this file again from the top, not only the one that failed. A fix can introduce a new failure elsewhere; catching that is the point of re-running the whole list.
