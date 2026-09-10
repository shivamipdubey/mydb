//! Connection health, as shown on the dashboard from phase 5 onward.

use serde::{Deserialize, Serialize};

/// Whether a connection is currently usable.
///
/// docs/13-dashboard-and-health-monitoring.md defines connected, disconnected,
/// and error. Phase 1 distinguishes only reachable from not, since nothing yet
/// tracks a connection across time to know it was previously up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "detail")]
pub enum HealthStatus {
    Connected,
    Error(String),
}

/// The result of one health check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub status: HealthStatus,
    /// The engine's reported version, when the check could read it.
    pub server_version: Option<String>,
}

impl Health {
    pub fn connected(server_version: Option<String>) -> Self {
        Self {
            status: HealthStatus::Connected,
            server_version,
        }
    }

    pub fn error(reason: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Error(reason.into()),
            server_version: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.status, HealthStatus::Connected)
    }
}
