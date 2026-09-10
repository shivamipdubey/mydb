# Credential Vault

## Phase 1 to 3
No vault yet. Connection credentials sit in a local config file. Do not encrypt this in a way that implies false security; a plain file is acceptable, but never described as a vault, until phase 4.

The file is written with owner-only permissions on Unix, and credentials are wrapped in a type that redacts itself in logs, errors, and panics (docs/16-security-and-cybersafety-checklist.md item 2). Neither is encryption, and neither makes this a vault; they limit who can read the file and keep the plaintext from leaking into output. See docs/15-data-model.md for the stored shape.

## Phase 4 onward
- Credentials for every saved connection are encrypted at rest, using a locally generated key.
- The key itself is protected by a passphrase the user sets.
- On first vault setup, MYDB generates a recovery phrase (a fixed-length list of words, generated locally, never transmitted anywhere). The user is shown this once and told to store it themselves.
- MYDB never stores the recovery phrase itself. Losing both the passphrase and the recovery phrase means the vault cannot be unlocked; state this plainly in the UI at setup time.
- No MYDB server ever holds a copy of the key, the passphrase, or the recovery phrase. There is no recovery path through MYDB.

## What the vault protects
Database usernames, passwords, connection strings, and any API tokens needed to reach a connected database. It does not protect the data inside the connected databases themselves; that stays wherever the database itself stores it.

## Rules for code touching the vault
- Never log a decrypted credential, in any log level, in any environment.
- Never write a decrypted credential to disk outside of memory during an active connection.
- Any new code path that reads from the vault must go through the vault module's own decrypt function, never re-implement decryption inline.
