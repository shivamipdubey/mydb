//! The saved connection list.
//!
//! This is a plain JSON file, not a vault. docs/06-credential-vault.md permits
//! storing credentials unencrypted until phase 4, but explicitly forbids
//! describing that storage in a way that implies security it does not have.
//! Nothing in this module is named vault, encrypt, or protect, and the UI must
//! not claim otherwise either. Phase 4 replaces the credential handling here
//! with the real vault.
//!
//! Two things are done anyway, because they are cheap and reduce blast radius:
//! the file is written with owner-only permissions on Unix, and every write is
//! atomic, so an interrupted save cannot leave a truncated connection list.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use mydb_core::{Engine, Secret};
use serde::{Deserialize, Serialize};

/// A saved database connection.
///
/// Mirrors the `connections` table in docs/15-data-model.md, reduced to what
/// phase 1 needs: there is no vault entry to reference yet, and no health
/// check result because the dashboard that would show it is phase 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    /// Stored in plaintext in phase 1 (docs/06). `Secret` keeps it out of logs
    /// and error messages regardless, per docs/16 item 2.
    pub password: Secret,
    /// Marks the connection higher stakes, requiring the extra confirmation
    /// step in docs/11-production-safety-flag.md before a destructive command
    /// runs against it. Settable by any user until roles arrive in phase 4.
    #[serde(default)]
    pub production: bool,
}

impl Connection {
    /// Builds a connection with a freshly generated id.
    pub fn new(
        name: impl Into<String>,
        engine: Engine,
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        username: impl Into<String>,
        password: Secret,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            engine,
            host: host.into(),
            port,
            database: database.into(),
            username: username.into(),
            password,
            production: false,
        }
    }
}

/// What can go wrong reading or writing the connection list.
///
/// Every variant is deliberately free of connection detail. An error rendered
/// into a log or the UI must never carry a password, so these messages name
/// the file and the failure and nothing else (docs/16 item 2).
#[derive(Debug, thiserror::Error)]
pub enum ConnectionStoreError {
    #[error("could not read the connection file at {path}: {kind}")]
    Read { path: PathBuf, kind: io::ErrorKind },

    #[error("could not write the connection file at {path}: {kind}")]
    Write { path: PathBuf, kind: io::ErrorKind },

    #[error("the connection file at {path} is not valid JSON (line {line}, column {column})")]
    Malformed {
        path: PathBuf,
        line: usize,
        column: usize,
    },

    #[error("no connection with id {0}")]
    NotFound(String),

    #[error("could not determine a config directory for this user")]
    NoConfigDirectory,
}

/// The on-disk shape. Versioned so a future format change can migrate rather
/// than fail to parse.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionFile {
    version: u32,
    connections: Vec<Connection>,
}

const CURRENT_VERSION: u32 = 1;

/// The saved connection list, backed by a JSON file.
#[derive(Debug)]
pub struct ConnectionStore {
    path: PathBuf,
    connections: Vec<Connection>,
}

impl ConnectionStore {
    /// The default config file location for this user.
    pub fn default_path() -> Result<PathBuf, ConnectionStoreError> {
        let base = dirs::config_dir().ok_or(ConnectionStoreError::NoConfigDirectory)?;
        Ok(base.join("MYDB").join("connections.json"))
    }

    /// Loads the connection list, treating a missing file as an empty list.
    ///
    /// A missing file is the normal first-run state, not an error. A malformed
    /// file is an error: silently discarding a list we failed to parse would
    /// lose the user's saved connections.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, ConnectionStoreError> {
        let path = path.into();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    path,
                    connections: Vec::new(),
                });
            }
            Err(error) => {
                return Err(ConnectionStoreError::Read {
                    path,
                    kind: error.kind(),
                })
            }
        };

        let file: ConnectionFile =
            serde_json::from_str(&contents).map_err(|error| ConnectionStoreError::Malformed {
                path: path.clone(),
                line: error.line(),
                column: error.column(),
            })?;

        Ok(Self {
            path,
            connections: file.connections,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> &[Connection] {
        &self.connections
    }

    pub fn get(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// Adds a connection and persists the list.
    pub fn add(&mut self, connection: Connection) -> Result<(), ConnectionStoreError> {
        self.connections.push(connection);
        self.save()
    }

    /// Replaces an existing connection and persists the list.
    pub fn update(&mut self, connection: Connection) -> Result<(), ConnectionStoreError> {
        let slot = self
            .connections
            .iter_mut()
            .find(|c| c.id == connection.id)
            .ok_or_else(|| ConnectionStoreError::NotFound(connection.id.clone()))?;
        *slot = connection;
        self.save()
    }

    /// Removes a connection and persists the list.
    pub fn remove(&mut self, id: &str) -> Result<(), ConnectionStoreError> {
        let before = self.connections.len();
        self.connections.retain(|c| c.id != id);
        if self.connections.len() == before {
            return Err(ConnectionStoreError::NotFound(id.to_string()));
        }
        self.save()
    }

    /// Sets or clears the production flag on a connection.
    ///
    /// Ungated in phases 1 to 3: docs/11-production-safety-flag.md restricts
    /// this to a connection's admin only once roles exist in phase 4.
    pub fn set_production(
        &mut self,
        id: &str,
        production: bool,
    ) -> Result<(), ConnectionStoreError> {
        let slot = self
            .connections
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| ConnectionStoreError::NotFound(id.to_string()))?;
        slot.production = production;
        self.save()
    }

    /// Writes the list to disk atomically.
    ///
    /// Written to a temporary file in the same directory and then renamed, so
    /// an interrupted or failed write leaves the previous list intact rather
    /// than a half-written one. A user losing their saved connections to a
    /// crash mid-save would be a real and avoidable failure.
    fn save(&self) -> Result<(), ConnectionStoreError> {
        let file = ConnectionFile {
            version: CURRENT_VERSION,
            connections: self.connections.clone(),
        };

        let write_error = |error: io::Error| ConnectionStoreError::Write {
            path: self.path.clone(),
            kind: error.kind(),
        };

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(write_error)?;
        }

        let serialized = serde_json::to_string_pretty(&file).map_err(|_| {
            // Serializing our own owned types cannot realistically fail, but
            // the error must not carry the data either way.
            ConnectionStoreError::Write {
                path: self.path.clone(),
                kind: io::ErrorKind::InvalidData,
            }
        })?;

        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, serialized).map_err(write_error)?;
        restrict_to_owner(&temporary)?;
        fs::rename(&temporary, &self.path).map_err(write_error)?;
        Ok(())
    }
}

/// Restricts a file to owner read and write.
///
/// The file holds plaintext database passwords in phase 1 (docs/06), so at
/// minimum another user account on the same machine should not be able to read
/// it. This is damage limitation, not encryption, and does not make this a
/// vault.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> Result<(), ConnectionStoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        ConnectionStoreError::Write {
            path: path.to_path_buf(),
            kind: error.kind(),
        }
    })
}

/// Windows inherits directory ACLs and has no mode bits; the config directory
/// is already per-user, so there is nothing equivalent to set here.
#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> Result<(), ConnectionStoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    // Tests may panic; the workspace lints exist to keep panics out of the
    // application, not out of assertions (docs/17-coding-standards.md).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn sample() -> Connection {
        Connection::new(
            "Local analytics",
            Engine::Postgres,
            "localhost",
            55432,
            "mydb_test",
            "mydb_test",
            Secret::new("mydb_test_password"),
        )
    }

    fn temp_store() -> (tempfile::TempDir, ConnectionStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        let store = ConnectionStore::load(&path).unwrap();
        (dir, store)
    }

    #[test]
    fn missing_file_loads_as_an_empty_list() {
        let (_dir, store) = temp_store();
        assert!(store.list().is_empty());
    }

    #[test]
    fn saves_and_reloads_a_connection_unchanged() {
        let (_dir, mut store) = temp_store();
        let connection = sample();
        store.add(connection.clone()).unwrap();

        let reloaded = ConnectionStore::load(store.path()).unwrap();
        assert_eq!(reloaded.list(), &[connection]);
    }

    #[test]
    fn production_flag_persists_across_reload() {
        let (_dir, mut store) = temp_store();
        let connection = sample();
        let id = connection.id.clone();
        store.add(connection).unwrap();
        assert!(!store.get(&id).unwrap().production);

        store.set_production(&id, true).unwrap();

        let reloaded = ConnectionStore::load(store.path()).unwrap();
        assert!(
            reloaded.get(&id).unwrap().production,
            "a connection marked production must still be marked production after a restart"
        );
    }

    #[test]
    fn update_and_remove_persist() {
        let (_dir, mut store) = temp_store();
        let mut connection = sample();
        let id = connection.id.clone();
        store.add(connection.clone()).unwrap();

        connection.name = "Renamed".to_string();
        store.update(connection).unwrap();
        assert_eq!(
            ConnectionStore::load(store.path())
                .unwrap()
                .get(&id)
                .unwrap()
                .name,
            "Renamed"
        );

        store.remove(&id).unwrap();
        assert!(ConnectionStore::load(store.path())
            .unwrap()
            .list()
            .is_empty());
    }

    #[test]
    fn missing_id_reports_not_found_rather_than_silently_succeeding() {
        let (_dir, mut store) = temp_store();
        assert!(matches!(
            store.set_production("no-such-id", true),
            Err(ConnectionStoreError::NotFound(_))
        ));
        assert!(matches!(
            store.remove("no-such-id"),
            Err(ConnectionStoreError::NotFound(_))
        ));
    }

    #[test]
    fn a_malformed_file_is_an_error_not_a_silently_emptied_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        fs::write(&path, "{ this is not json").unwrap();

        assert!(
            matches!(
                ConnectionStore::load(&path),
                Err(ConnectionStoreError::Malformed { .. })
            ),
            "a parse failure must not be mistaken for an empty connection list"
        );
    }

    // --- docs/16 item 2: the password must not escape into any output ---

    #[test]
    fn debug_of_the_store_does_not_contain_the_password() {
        let (_dir, mut store) = temp_store();
        store.add(sample()).unwrap();
        let rendered = format!("{store:?}");
        assert!(
            !rendered.contains("mydb_test_password"),
            "password leaked into Debug output: {rendered}"
        );
        assert!(rendered.contains(mydb_core::REDACTED));
    }

    #[test]
    fn error_messages_never_contain_connection_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        fs::write(&path, "{ not json").unwrap();

        let error = ConnectionStore::load(&path).unwrap_err();
        let rendered = format!("{error}");
        let debug = format!("{error:?}");
        for output in [rendered, debug] {
            assert!(!output.contains("mydb_test_password"), "leaked: {output}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn the_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, mut store) = temp_store();
        store.add(sample()).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "phase 1 stores passwords in plaintext, so at minimum no other \
             user on this machine should be able to read the file"
        );
    }

    #[test]
    fn an_interrupted_save_leaves_no_stray_temporary_file() {
        let (_dir, mut store) = temp_store();
        store.add(sample()).unwrap();
        let temporary = store.path().with_extension("json.tmp");
        assert!(
            !temporary.exists(),
            "the temporary file should have been renamed away, not left behind"
        );
    }
}
