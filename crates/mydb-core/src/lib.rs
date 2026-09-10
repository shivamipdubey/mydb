//! Domain types shared by every other MYDB module.
//!
//! This crate exists so the parser, adapters, confirmation engine, and storage
//! can all speak about the same `Intent` without depending on each other.
//! docs/17-coding-standards.md forbids one module reaching into another's code;
//! a shared vocabulary crate is how that stays true as modules are added.
//!
//! Phase 1 scope: types are introduced by the task that needs them (T6 adds the
//! real `Intent`). This crate currently carries only the phase marker below.

/// The phase of docs/03-phases-roadmap.md this build implements.
///
/// Used by the smoke test to prove the workspace compiles and links. It is not
/// a feature flag; nothing branches on it.
pub const IMPLEMENTED_PHASE: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_links_and_reports_phase_one() {
        assert_eq!(IMPLEMENTED_PHASE, 1);
    }
}
