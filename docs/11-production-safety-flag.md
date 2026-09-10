# Production Safety Flag

## Purpose
Give the user a way to mark a connection as higher stakes, so destructive commands against it require an extra deliberate step beyond the normal confirm button.

## Behavior
- Any connection can be flagged production from the connection settings screen. Restricting this to the connection's admin applies only once the role system exists in phase 4 (docs/08-permissions-and-roles.md); in phases 1 to 3 there is no role distinction, so any user of the app can set the flag. The extra confirmation step below is never gated on roles and applies from phase 1 onward.
- An update counts as destructive. It overwrites values that were there before, and docs/07-audit-log-and-recovery-bin.md treats overwritten data as something the recovery bin must hold, which is to say as data loss. An insert does not count: it creates a record and destroys nothing, so it needs no extra step.
- The extra step applies in addition to, not instead of, the normal preview and confirm sequence.
- The production flag is per connection, set once, and visible at all times in the UI while that connection is active (a persistent visual indicator, not just a settings toggle buried in a menu).

## What the extra step asks for
The step is matched to what is actually at risk, not made uniform. A record count is a reasonable thing to make someone retype when records are what will be lost, and a poor one otherwise.

### DELETE and UPDATE
Type the exact affected record count shown in the preview, or the word CONFIRM.

When the preview affects no records or one, the count is not offered and CONFIRM is required instead. Typing "0" or "1" is a keystroke, not a moment of deliberation, and a gate that adds no friction is not a gate.

### DROP TABLE and TRUNCATE
Type the table's name, qualified as the preview shows it. The record count is not accepted.

Two reasons. A count says nothing about the structure a drop also destroys, so it understates what is being agreed to. And the count is meaningless when the table happens to be empty, while the table's name is meaningful whatever the row count.

### INSERT
No extra step, on any connection. An insert destroys nothing.

## How it is enforced
The step is worked out when the preview is built, from the count the user is actually looking at, so the number they are asked to type is the number they were shown. Editing a command re-derives it from the revised command's own preview: narrowing a large delete to a single record changes the gate from a count to CONFIRM, and does not remove it.

What the user types is checked in the confirmation engine, not only wherever the interface disables a button. A gate enforced solely in the interface is not a gate. The interface applies the same rule as well, so the user is not invited to press something that will be refused, and a wrong entry leaves the preview on screen to try again rather than discarding it.

Matching ignores surrounding space and letter case. The friction is in having to type the right thing at all; requiring someone to match a table name's capitalisation on top of that would be arbitrary, and MYDB matches text case-insensitively elsewhere.

## Testing requirement
A test proving the confirm button stays disabled until the extra step is completed, for every destructive operation type, on a production-flagged connection. That is four operations: delete, update, drop table, and truncate. Insert is excluded, since it destroys nothing.

Each type also needs a test that the wrong entry is refused, not merely that the right one is accepted: that a record count does not open a schema change's gate, and that typing "1" does not open a single-record delete's.
