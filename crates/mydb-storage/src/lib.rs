//! MYDB's own local storage (docs/15-data-model.md).
//!
//! Phase 1 holds the connection config file (T4) and the basic command history
//! (T13). It is deliberately NOT a vault: docs/06-credential-vault.md allows a
//! plain file until phase 4 but forbids calling it a vault or implying
//! encryption that is not there. The audit log and recovery bin are phase 2.

mod connections;

pub use connections::{Connection, ConnectionStore, ConnectionStoreError};
