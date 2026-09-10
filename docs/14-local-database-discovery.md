# Local Database Discovery

## Behavior
- Off by default. The user must explicitly opt in from the connection manager screen, using a clearly labeled action.
- When triggered, MYDB scans common local ports and known config file locations for database instances (for example, default Postgres, MySQL, and MongoDB ports, and common local config paths).
- Found instances are presented as suggestions, not auto-added connections. The user reviews and confirms each one before it becomes a saved connection, including entering any needed credentials.

## What this does not do
- It does not scan beyond the local machine.
- It does not attempt to connect using guessed or default credentials; discovery only identifies that something is listening on a known port, it never attempts login on the user's behalf.
- It never runs automatically on startup; every scan is a separate, explicit user action.
