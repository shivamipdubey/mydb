# Permissions and Roles

Introduced in phase 4. Applies per database connection, not per user account globally.

## Roles
- Admin on a connection: can preview and execute writes, manage that connection's production flag, and manage who else has access if the connection is shared.
- Viewer on a connection: can run reads and see previews, cannot confirm or execute a write.

## Rules
- A single user account can hold different roles on different connections: admin on one, viewer on another. Role checks always happen per connection, never globally.
- The confirmation workflow (docs/05-confirmation-workflow.md) must check the caller's role on the target connection before enabling the confirm step. A viewer never sees an enabled confirm button, on any screen.
- Role assignment on a shared connection is controlled by that connection's admin, exercised through the LAN host module (docs/09-sharing-and-lan-hosting.md).

## Testing requirement
Every write path needs a test proving a viewer-role call is rejected before it reaches the adapter's execute function, not just hidden in the UI.
