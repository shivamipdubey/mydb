//! The database engines MYDB supports.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A supported database engine.
///
/// docs/04-database-adapters.md and CLAUDE.md rule 2 forbid writing code
/// against an engine that does not yet have an adapter section describing its
/// read equivalent for each write operation. Adding a variant here is
/// therefore a documentation change first and a code change second.
///
/// Phase 1 ships Postgres only (docs/03-phases-roadmap.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Engine {
    Postgres,
}

impl Engine {
    /// The engine's name as shown to the user.
    pub fn display_name(self) -> &'static str {
        match self {
            Engine::Postgres => "PostgreSQL",
        }
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_a_stable_lowercase_name() {
        // The config file on disk depends on this spelling; changing it would
        // silently orphan a user's saved connections.
        let json = serde_json::to_string(&Engine::Postgres).unwrap_or_default();
        assert_eq!(json, "\"postgres\"");
    }
}
