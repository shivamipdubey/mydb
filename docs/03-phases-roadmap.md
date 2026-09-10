# Phases and Roadmap

Work one phase at a time. Do not begin a phase until the previous one passes its exit conditions in docs/25-exit-conditions-definition-of-done.md.

## Phase 1: Core safety loop, single engine, single platform
- Pick Postgres as the first engine.
- Pick one operating system to build and test against first.
- Build the command parser at a basic level: rule-based or simple model, accuracy matters more than sophistication here.
- Build the confirmation workflow end to end: type a command, see the matching read result, confirm or edit, then execute.
- Build the production flag and its extra confirmation step.
- Credentials stored in a plain local config file. The vault comes later.
- No audit log detail requirements yet beyond a basic command history.
- Package the macOS build. macOS packaging is a phase 1 deliverable, stated here explicitly so it is not left implied by phase 3's wording. The bundle is unsigned; see the code signing gap below.
- Set up cross-platform continuous integration: build and test on Windows, macOS, and Linux on every commit. This lands in phase 1, not phase 3, because development happens on macOS only and breakage on the other two platforms should surface as it is introduced rather than accumulate. Phase 3 still owns packaging and hands-on testing for those platforms (docs/18-testing-strategy.md records what CI can and cannot verify on each).

## Phase 2: Database breadth
- Add MongoDB.
- Add the remaining major engines from docs/04-database-adapters.md, one at a time, each with its own preview adapter.
- Build the full audit log: before and after state, full detail for small operations, summary plus reference for large ones.
- Build the 30-day recovery bin.
- Backlog, schema editing: add ALTER TABLE support, with the preview rules docs/04-database-adapters.md already defines for it. Phase 1 ships DROP TABLE and TRUNCATE only; ALTER TABLE was deliberately left out of phase 1 scope.

## Code signing gap, carried into phase 3
The phase 1 macOS bundle is unsigned and not notarized. It carries only an ad-hoc signature with no Team Identifier, which is what Rust's linker produces by default and is not something Gatekeeper accepts.

What that costs, stated plainly so it is not discovered later:

- On the machine that built it, the app runs. Opening it the first time needs right-click then Open, or clearing the quarantine attribute. Running from source with `npm run dev` never hits this.
- Given to anyone else, it effectively does not work. An un-notarized app downloaded from the internet is blocked on Apple Silicon, and the way around it is for the recipient to run a terminal command against a binary from someone else. That is not a distribution path.
- docs/01-prd.md calls MYDB a downloadable desktop app, so real distribution needs a Developer ID certificate and notarization. That is a purchase (a paid Apple Developer account), not a code change, and it is deliberately not treated as a coding task.

The phase 1 bundle is also arm64 only. An Intel Mac needs either a separate x86_64 build or a universal binary.

Both belong to phase 3, which owns packaging and distribution across platforms. Windows has its own equivalent problem: an unsigned installer triggers SmartScreen warnings, and Authenticode signing is also a purchase. Neither is a reason to delay phase 1; both are reasons phase 1's bundle should not be described as distributable.

## Phase 3: Platform breadth
- Package and test on the remaining two operating systems, and resolve the code signing gap above for all three. macOS is already packaged in phase 1, and cross-platform CI already builds and tests all three from phase 1 onward; phase 3 is where Windows and Linux get real packaging and hands-on testing.
- Build the opt-in local scan for already-running databases.

## Phase 4: Vault and roles
- Build the encrypted credential vault with the manual recovery phrase.
- Build per-database roles: admin and viewer, assignable per connection.

## Phase 5: Language layer
- Add hardware detection and automatic local model sizing, with manual override.
- Add the three-step ambiguity fallback: best guess, alternatives, clarifying question.
- Add voice input, routed through the same parser and confirmation workflow as typed input.

## Phase 6: Sharing
- Build the LAN host mode: password-gated, IP and port based.
- Build the connect-to-host flow from another machine's desktop app.

## Ordering rule
A later phase can be reordered only if it has no dependency on an earlier one. Phase 5's language layer depends on phase 1's parser existing. Phase 6's sharing depends on phase 4's roles existing, since a shared connection needs a role system to mean anything. Do not skip ahead on a whim; check dependencies against docs/02-architecture.md first.
