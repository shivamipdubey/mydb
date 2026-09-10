# Glossary

Adapter: the module that connects to one specific database engine and implements connect, describe schema, build preview, and execute.

Intent: the structured result of parsing a typed or spoken command, naming the engine, target, operation, and filter or payload.

Preview: the read-only result shown before a write, built from the target engine's own read equivalent (a SELECT for SQL, a find for MongoDB, and so on).

Confirmation workflow: the mandatory sequence of preview, confirm or edit or cancel, then execute, described in docs/05.

Production flag: a per-connection marker requiring an extra confirmation step before a destructive command runs against that connection.

Vault: the encrypted local store for connection credentials, introduced in phase 4.

Recovery phrase: a locally generated set of words that unlocks the vault if the passphrase is lost. Never stored or transmitted by MYDB.

Recovery bin: local storage holding the full before-state of anything deleted or overwritten, for 30 days.

Audit log: the append-only record of every executed command.

Role: admin or viewer, assigned per user per connection.

LAN hosting: one machine opening a password-protected local port so others can connect their own MYDB app to the same database session.

Discovery: the opt-in scan for databases already running on the local machine.
