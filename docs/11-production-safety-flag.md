# Production Safety Flag

## Purpose
Give the user a way to mark a connection as higher stakes, so destructive commands against it require an extra deliberate step beyond the normal confirm button.

## Behavior
- Any connection can be flagged production from the connection settings screen. Restricting this to the connection's admin applies only once the role system exists in phase 4 (docs/08-permissions-and-roles.md); in phases 1 to 3 there is no role distinction, so any user of the app can set the flag. The extra confirmation step below is never gated on roles and applies from phase 1 onward.
- An update counts as destructive for this purpose. It overwrites values that were there before, and docs/07-audit-log-and-recovery-bin.md treats overwritten data as something the recovery bin must hold, which is to say as data loss. An insert does not count: it creates a record and destroys nothing.
- When a destructive command (delete, drop, truncate, update, or similar) targets a production-flagged connection, the confirm step in docs/05-confirmation-workflow.md requires one additional action: the user types the exact affected record count shown in the preview, or types the word CONFIRM, before the confirm button becomes active.
- This extra step applies in addition to, not instead of, the normal preview and confirm sequence.
- The production flag is per connection, set once, and visible at all times in the UI while that connection is active (a persistent visual indicator, not just a settings toggle buried in a menu).

## Testing requirement
A test proving the confirm button stays disabled until the extra step is completed, for every destructive operation type, on a production-flagged connection.
