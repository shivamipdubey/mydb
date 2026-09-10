# Risk Register

## A destructive command runs without a preview
Mitigation: docs/05-confirmation-workflow.md makes this structurally required, and docs/18-testing-strategy.md requires a test proving call order for every adapter.

## Vault key or recovery phrase lost, credentials become unrecoverable
Mitigation: this is accepted risk by design, since no MYDB server holds a copy. Mitigated by clear warning at vault setup time, not by a hidden recovery path.

## Large delete or update operation grows the audit log or recovery bin beyond reasonable disk usage
Mitigation: size-tiered logging (docs/07) and a configurable recovery bin cap.

## Mid-batch failure across multiple databases leaves an inconsistent state across engines with no shared transaction system
Mitigation: no automatic rollback is attempted. Completed items are left as-is, only the remaining items are discussed with the user, and the recovery bin gives a manual path back for anything already changed.

## LAN hosting exposes a database connection to unauthorized access
Mitigation: password gate, encrypted connection, rate-limited login attempts, hosting off by default.

## Local model misinterprets a command and produces an incorrect but plausible-looking query
Mitigation: the confidence-tiered fallback (docs/10) and the mandatory preview step; even a wrong interpretation is caught by the user seeing the wrong records in the preview before confirming.

## Local database discovery scans the network beyond the user's own machine
Mitigation: discovery is explicitly scoped to local ports and config files only, never a network-wide scan; enforce this in code review against docs/14.

## Cross-platform packaging introduces platform-specific bugs in the vault or local model handling
Mitigation: phase 3 dedicates full testing time to the two platforms not covered in phase 1, rather than assuming parity.
