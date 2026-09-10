# Dashboard and Health Monitoring

## Purpose
One screen showing every connection MYDB knows about, and whether each is currently reachable and healthy.

## Per-connection information shown
- Connection name, engine type, and production flag status.
- Health: connected, disconnected, or error, checked on an interval and on demand.
- Basic stats: approximate size, table or collection count, and time of last activity through MYDB.
- Role: the current user's role on that connection (admin or viewer, once phase 4 ships).

## Health check behavior
- A lightweight, read-only check runs periodically (for example, a simple connectivity ping plus a metadata query), never a query that could be mistaken for a data-changing operation.
- A failed health check surfaces clearly on the dashboard, with the last known good state still visible alongside the current failure.

## Multi-connection actions
- A command can target one connection, several selected by checkbox, or all connections at once.
- If the user's command text already names specific connections, use those. Otherwise, ask which connections to target, defaulting to none selected rather than defaulting to all.
