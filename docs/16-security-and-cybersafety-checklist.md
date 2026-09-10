# Security and Cybersafety Checklist

Run this checklist after every change, not only at the end of a phase. Treat a failed item as blocking; fix it before moving on.

## After every change
1. Does any new or modified code path execute a write without going through the confirmation workflow in docs/05-confirmation-workflow.md? If yes, stop and fix before continuing.
2. Does any new code log, print, or write to disk a decrypted credential, a vault passphrase, or a recovery phrase, anywhere, including in error messages and stack traces? If yes, stop and fix.
3. Does any new database query build its filter or payload through unvalidated string concatenation instead of parameterized queries or the driver's safe query builder? If yes, stop and fix; this applies to every engine, not just SQL ones.
4. Does any new network-facing code (the LAN host and connect module) accept a connection without the password check and without encryption? If yes, stop and fix.
5. Does any new role check happen only in the UI layer, without a matching check at the adapter or execution layer? If yes, stop and fix.
6. Does any new dependency get added without checking it against known vulnerability databases first? If yes, check it before merging. Run `cargo audit` for Rust dependencies and `npm audit` for frontend ones. A reported vulnerability is blocking. An unmaintained or unsound advisory on a transitive dependency we do not choose directly is not blocking on its own, but record it in the changelog so a later upgrade can clear it.
7. Does the recovery bin or audit log get written to before the real operation completes, in a way that could record something that never actually happened? If yes, fix the ordering.
8. Does any new code assume network access is required, when the feature is meant to work fully offline? If yes, fix the assumption.

## Before ending a phase
Run every item above across the full diff for that phase, not just the last change. Record the result in the phase's exit conditions check (docs/25-exit-conditions-definition-of-done.md).

## Reporting
When a checklist item fails and gets fixed, note it briefly in the changelog (docs/21-changelog-conventions.md) so the pattern is visible later if it recurs.
