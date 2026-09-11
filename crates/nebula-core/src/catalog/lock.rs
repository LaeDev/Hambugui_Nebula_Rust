//! The cross-process server lock (reliability rule 6).
//!
//! Every mutation of a server — mod install, update, remove, move, server
//! delete and duplicate — runs inside one of these, so two Nebula processes
//! cannot rewrite `modindex.json` or a namespace directory at the same time.
//! The GUI's in-process busy flag is a UX lock; this is the correctness one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::CatalogError;
use crate::layout::ServerDir;

/// The lock file's name inside a server directory. No scanner reads the
/// server's top level, so it never reaches `distribution.json`.
pub const LOCK_FILE: &str = ".nebula-lock";

/// How long a lock is honoured before it is assumed to be an orphan.
///
/// There is **no pid liveness probe**: `std` cannot ask whether a pid is
/// alive, and on Windows a recycled pid would make a naive probe lie. Age plus
/// an unparsable body is the whole break rule, which errs towards refusing a
/// concurrent run for at most half an hour after a crash.
pub const STALE_AFTER_SECS: u64 = 30 * 60;

/// What the lock file holds, so a stale one can name the process that left it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    /// Unix seconds when the lock was taken.
    pub started_at: u64,
}

/// An exclusive hold on one server directory, released on drop.
#[derive(Debug)]
pub struct ServerLock {
    path: PathBuf,
    info: LockInfo,
}

impl ServerLock {
    /// Takes the lock, breaking an orphaned one. A live lock is a clean
    /// refusal, never a wait.
    pub fn acquire(server: &ServerDir) -> Result<Self, CatalogError> {
        Self::acquire_at(&server.path.join(LOCK_FILE))
    }

    /// [`ServerLock::acquire`] against an explicit path, for callers that
    /// already hold the directory and for tests.
    pub fn acquire_at(path: &Path) -> Result<Self, CatalogError> {
        let info = LockInfo {
            pid: std::process::id(),
            started_at: now_secs(),
        };
        match write_new(path, &info) {
            Ok(()) => Ok(Self {
                path: path.to_owned(),
                info,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let held = read_info(path);
                if !breakable(held.as_ref()) {
                    return Err(CatalogError::Locked {
                        pid: held.map_or(0, |info| info.pid),
                    });
                }
                log::warn!(
                    "Breaking a stale Nebula lock at {} (pid {:?})",
                    path.display(),
                    held.map(|info| info.pid)
                );
                std::fs::remove_file(path)?;
                write_new(path, &info)?;
                Ok(Self {
                    path: path.to_owned(),
                    info,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Who holds it, for logging.
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// Where the lock file lives.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ServerLock {
    fn drop(&mut self) {
        // `delete_server` removes the whole directory while holding the lock,
        // so the file being gone already is the expected outcome there.
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("Could not release {}: {error}", self.path.display());
        }
    }
}

/// A lock is breakable when its body is unreadable or unparsable — nothing we
/// wrote — or when it is older than [`STALE_AFTER_SECS`].
fn breakable(info: Option<&LockInfo>) -> bool {
    let Some(info) = info else {
        return true;
    };
    now_secs().saturating_sub(info.started_at) > STALE_AFTER_SECS
}

fn read_info(path: &Path) -> Option<LockInfo> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_new(path: &Path, info: &LockInfo) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec(info).unwrap_or_default())
}

/// Wall clock, not `Instant`: the reader is a different process.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_live_lock_and_breaks_an_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);

        let held = ServerLock::acquire_at(&path).unwrap();
        assert_eq!(held.info().pid, std::process::id());
        let refused = ServerLock::acquire_at(&path).unwrap_err();
        assert!(matches!(refused, CatalogError::Locked { .. }), "{refused}");

        drop(held);
        assert!(!path.is_file());

        // An orphan from half an hour ago is broken, with its pid logged.
        let stale = LockInfo {
            pid: 4242,
            started_at: now_secs() - STALE_AFTER_SECS - 1,
        };
        std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();
        let broken = ServerLock::acquire_at(&path).unwrap();
        assert_eq!(broken.info().pid, std::process::id());
    }

    #[test]
    fn breaks_a_lock_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOCK_FILE);
        std::fs::write(&path, b"not json").unwrap();
        let lock = ServerLock::acquire_at(&path).unwrap();
        assert!(path.is_file());
        drop(lock);
        assert!(!path.is_file());
    }
}
