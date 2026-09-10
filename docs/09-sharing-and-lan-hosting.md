# Sharing and LAN Hosting

Introduced in phase 6. No MYDB server is involved at any point.

## Hosting
- One user's machine can host a session on a chosen local port.
- The host sets a password at the time hosting starts.
- The host's app shows the IP address and port to share with teammates directly (out of band; MYDB does not send it anywhere).

## Connecting
- A teammate's desktop app (or later, browser app) connects using the host's IP, port, and password.
- On successful connection, the role system (docs/08-permissions-and-roles.md) applies: the host, as admin, assigns the connecting user a role on the shared connection.

## Security requirements
- The password is never sent in plain text over the network; the connection itself must be encrypted, even on a LAN.
- A failed password attempt is rate-limited to slow brute-force attempts.
- Hosting is off by default. Starting a host session requires an explicit action, never automatic.
- When hosting stops, all connected sessions end immediately, and no data persists on the host beyond what the host's own local storage already had.
