//! A string that must never reach a log, an error message, or a panic.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Wraps a credential so it cannot be printed by accident.
///
/// docs/16-security-and-cybersafety-checklist.md item 2 forbids any code path
/// that logs, prints, or writes out a credential, including inside error
/// messages and stack traces. A plain `String` makes that a rule people have to
/// remember; this type makes it the default, because `{:?}` on any struct
/// containing one prints a redaction marker instead of the value.
///
/// Reading the real value requires calling [`Secret::expose`], which is
/// deliberately awkward to name and easy to grep for during review.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the underlying credential.
    ///
    /// Call this only when handing the value to a database driver. Never to
    /// build a log line, an error message, or anything rendered in the UI.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What a redacted credential renders as. Kept as a constant so tests assert
/// against the same string the code produces.
pub const REDACTED: &str = "[redacted]";

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

// Display is intentionally NOT implemented. Anything that wants to print a
// Secret should have to think about it, and `format!("{secret}")` should not
// compile.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_value() {
        let secret = Secret::new("hunter2");
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, REDACTED);
        assert!(!rendered.contains("hunter2"));
    }

    #[test]
    fn debug_of_a_containing_struct_also_redacts() {
        #[derive(Debug)]
        struct Holder {
            #[allow(dead_code)]
            password: Secret,
        }
        let rendered = format!(
            "{:?}",
            Holder {
                password: Secret::new("hunter2")
            }
        );
        assert!(!rendered.contains("hunter2"), "leaked through: {rendered}");
    }

    #[test]
    fn expose_returns_the_real_value_for_drivers() {
        assert_eq!(Secret::new("hunter2").expose(), "hunter2");
    }
}
