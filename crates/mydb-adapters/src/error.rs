//! Adapter failures, in a form safe to show a user.

/// Something an adapter could not do.
///
/// Variants carry a human-readable reason, never the connection's credentials.
/// docs/16-security-and-cybersafety-checklist.md item 2 forbids a credential
/// reaching an error message, and errors from this layer are rendered directly
/// in the UI, so the constructors below are the only way in and they take a
/// reason string the caller has already sanitised.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    #[error("could not connect to the database: {reason}")]
    Connection { reason: String },

    #[error("the database rejected the request: {reason}")]
    Query { reason: String },

    #[error("{0}")]
    Unsupported(String),
}

impl AdapterError {
    pub fn connection(reason: impl Into<String>) -> Self {
        Self::Connection {
            reason: reason.into(),
        }
    }

    pub fn query(reason: impl Into<String>) -> Self {
        Self::Query {
            reason: reason.into(),
        }
    }
}
