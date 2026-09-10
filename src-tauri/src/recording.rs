//! Where a write's captured before-state goes.
//!
//! The adapter streams records as it reads them and knows nothing about the
//! recovery bin (docs/17-coding-standards.md keeps them separate modules).
//! This is the piece that joins the two: a sink backed by the recovery bin's
//! staging area, so a before-state larger than memory still reaches the bin
//! in full, as docs/07-audit-log-and-recovery-bin.md requires.
//!
//! Staged records only become a recovery entry once the write has committed.
//! If it fails, they are discarded: they describe something that never
//! happened, and offering that to a user as recoverable data would be worse
//! than offering nothing.

use std::sync::{Arc, Mutex};

use mydb_core::{RecordSnapshot, SinkError, StateSink};
use mydb_storage::{RecoveryBin, StagedCapture, SystemClock};
use rusqlite::Connection;

/// Shared handle to the local record store.
///
/// Shared rather than borrowed because the sink is handed to the adapter and
/// has to outlive any one call, and because `rusqlite::Connection` is not
/// `Sync`, so a reference to one cannot cross the async boundary the adapter
/// sits behind. Each record is written under a brief lock; the guard is never
/// held across an await.
pub type Records = Arc<Mutex<Option<Connection>>>;

/// A sink that streams into the recovery bin's staging area.
pub struct StagingSink {
    records: Records,
    capture: StagedCapture,
    /// The first error, kept so the adapter's failure is reported once rather
    /// than once per remaining record.
    failed: Option<SinkError>,
}

impl StagingSink {
    /// Begins a capture. Fails only if the record store is unavailable, in
    /// which case the caller should not proceed with a write it cannot make
    /// recoverable.
    pub fn begin(records: Records) -> Result<Self, String> {
        let capture = {
            let guard = records.lock().map_err(|_| POISONED.to_string())?;
            let connection = guard.as_ref().ok_or_else(|| UNAVAILABLE.to_string())?;
            RecoveryBin::new(connection, &SystemClock).stage()
        };
        Ok(Self {
            records,
            capture,
            failed: None,
        })
    }

    /// Turns the staged records into a real, expiring recovery entry.
    ///
    /// Called only after the write has committed. Returns the entry's id, or
    /// `None` when there was nothing to capture.
    pub fn finalize(
        self,
        connection_id: &str,
        connection_name: &str,
        operation: &str,
        intent_summary: &str,
    ) -> Result<Option<i64>, String> {
        let guard = self.records.lock().map_err(|_| POISONED.to_string())?;
        let connection = guard.as_ref().ok_or_else(|| UNAVAILABLE.to_string())?;
        let bin = RecoveryBin::new(connection, &SystemClock);

        if let Some(error) = self.failed {
            // The capture was incomplete, so an entry built from it would
            // claim to hold a before-state it does not have. No entry and a
            // clear complaint beats a partial one that looks whole.
            let _ = bin.discard(self.capture);
            return Err(error.to_string());
        }

        if self.capture.records_staged() == 0 {
            // Nothing was captured, so there is nothing to recover. An empty
            // entry would only clutter the bin.
            let _ = bin.discard(self.capture);
            return Ok(None);
        }

        bin.finalize(
            self.capture,
            connection_id,
            connection_name,
            operation,
            intent_summary,
        )
        .map(|entry| Some(entry.id))
        .map_err(|error| error.to_string())
    }

    /// Throws the capture away, because the write did not happen.
    pub fn discard(self) -> Result<(), String> {
        let guard = self.records.lock().map_err(|_| POISONED.to_string())?;
        let connection = guard.as_ref().ok_or_else(|| UNAVAILABLE.to_string())?;
        RecoveryBin::new(connection, &SystemClock)
            .discard(self.capture)
            .map_err(|error| error.to_string())
    }
}

const POISONED: &str = "the local record store is in an inconsistent state";
const UNAVAILABLE: &str = "the local record store is unavailable";

impl StateSink for StagingSink {
    fn accept(&mut self, record: &RecordSnapshot) -> Result<(), SinkError> {
        // After a failure the rest of the capture is ignored rather than
        // retried per record. The entry will be refused at finalize time,
        // because a partial before-state must not be presented as whole.
        if self.failed.is_some() {
            return Ok(());
        }

        let outcome = (|| -> Result<(), String> {
            let guard = self.records.lock().map_err(|_| POISONED.to_string())?;
            let connection = guard.as_ref().ok_or_else(|| UNAVAILABLE.to_string())?;
            RecoveryBin::new(connection, &SystemClock)
                .stage_record(&mut self.capture, record)
                .map_err(|error| error.to_string())
        })();

        match outcome {
            Ok(()) => Ok(()),
            Err(detail) => {
                let failure = SinkError::new(detail);
                self.failed = Some(failure.clone());
                Err(failure)
            }
        }
    }
}
