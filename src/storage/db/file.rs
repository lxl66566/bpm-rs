//! use `config-file2` as the backend of the storage.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Result, anyhow};
use config_file2::{LoadConfigFile, StoreConfigFile};

use super::DbOperation;
use crate::storage::{Repo, RepoList};

/// Process-wide registry of held db locks (ref-counted per canonical path).
///
/// The OS lock must be exclusive across processes, but re-entrant within one
/// process: code may legitimately hold several `Db` handles for the same path
/// (e.g. a test keeping one while dispatch creates another). The first holder
/// takes the OS lock; later ones just bump the count. The OS lock is released
/// when the last handle drops.
static LOCK_REGISTRY: OnceLock<Mutex<HashMap<PathBuf, usize>>> = OnceLock::new();

fn lock_registry() -> &'static Mutex<HashMap<PathBuf, usize>> {
    LOCK_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// RAII guard: attach to (or create) the process-wide hold on a db lock.
#[derive(Debug)]
struct ProcessLock {
    path: PathBuf,
    /// Only `Some` for the handle that took the OS lock.
    file: Option<File>,
}

impl ProcessLock {
    fn acquire(db_path: &Path) -> Result<Self> {
        let mut registry = lock_registry().lock().unwrap();
        if let Some(count) = registry.get_mut(db_path) {
            // Re-entrant within this process; the OS lock is already held.
            *count += 1;
            return Ok(Self {
                path: db_path.to_path_buf(),
                file: None,
            });
        }
        let lock_path = Db::lock_path(db_path);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        // Call the fs4 trait method fully-qualified: std's inherent
        // File::try_lock (stable since 1.89) would shadow it on newer
        // toolchains and mark the import unused; this keeps MSRV 1.85
        // compatible either way.
        fs4::FileExt::try_lock(&file).map_err(|e| {
            anyhow!(
                "could not lock `{}` (another bpm instance may be running): {e}",
                lock_path.display()
            )
        })?;
        registry.insert(db_path.to_path_buf(), 1);
        Ok(Self {
            path: db_path.to_path_buf(),
            file: Some(file),
        })
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        let mut registry = lock_registry().lock().unwrap();
        if let Some(count) = registry.get_mut(&self.path)
            && *count > 0
        {
            *count -= 1;
            if *count == 0 {
                registry.remove(&self.path);
                // Dropping the file handle releases the OS lock.
                self.file = None;
            }
        }
    }
}

#[derive(Debug)]
pub struct Db {
    pub db_path: PathBuf,
    pub repo_list: Mutex<RepoList>,
    /// Cross-process exclusive lock (re-entrant in-process), held for the
    /// lifetime of this handle.
    _lock: ProcessLock,
}

impl Db {
    /// Lock file path: `<db_path>.lock` (appended, so `db.json.lock`).
    fn lock_path(db_path: &Path) -> PathBuf {
        let mut name = db_path.as_os_str().to_os_string();
        name.push(".lock");
        PathBuf::from(name)
    }

    /// Atomically persist the repo list to disk.
    ///
    /// Writes to a temporary file first, syncs it, then renames it to the
    /// final path. The sync + directory fsync (unix) ensure a crash never
    /// leaves a truncated or missing db behind.
    fn store_atomic(&self) -> Result<()> {
        // Preserve the original extension so config-file2 can detect the format.
        // e.g. db.json -> db.tmp.json -> rename to db.json
        let tmp_path = {
            let stem = self.db_path.file_stem().unwrap_or_default();
            let tmp_name = match self.db_path.extension() {
                Some(ext) => format!("{}.tmp.{}", stem.to_string_lossy(), ext.to_string_lossy()),
                None => format!("{}.tmp", stem.to_string_lossy()),
            };
            self.db_path.with_file_name(tmp_name)
        };
        self.repo_list.lock().unwrap().store(&tmp_path)?;
        // fsync contents before rename, otherwise a crash right after the
        // rename could leave an empty/truncated db on disk.
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tmp_path)?
            .sync_all()?;
        fs::rename(&tmp_path, &self.db_path)?;
        #[cfg(unix)]
        if let Some(parent) = self.db_path.parent() {
            // fsync the directory so the rename itself is durable
            use std::os::fd::AsRawFd;
            let dir = fs::File::open(parent)?;
            // SAFETY: dir is an owned, valid fd; fsync does not touch memory.
            unsafe { libc::fsync(dir.as_raw_fd()) };
        }
        Ok(())
    }
}

impl DbOperation for Db {
    fn create_or_open(path: impl AsRef<Path>) -> Result<Self>
    where
        Self: Sized,
    {
        let db_path = path.as_ref().to_path_buf();
        // Lock BEFORE reading so a concurrent writer cannot make us load
        // stale state.
        let lock = ProcessLock::acquire(&db_path)?;
        let repo_list = RepoList::load_or_default(&db_path)?.into();
        Ok(Self {
            db_path,
            repo_list,
            _lock: lock,
        })
    }

    fn get_repo_list(&self) -> RepoList {
        self.repo_list.lock().unwrap().clone()
    }

    fn get_repo(&self, name: &str) -> Option<Repo> {
        self.repo_list
            .lock()
            .unwrap()
            .iter()
            .find(|x| x.name == name)
            .cloned()
    }

    fn insert_repo(&self, repo: Repo) -> Result<()> {
        // Upsert by name so that updating an already-tracked repo overwrites
        // its previous record instead of appending a duplicate entry.
        self.repo_list.lock().unwrap().upsert(repo);
        self.store_atomic()?;
        Ok(())
    }

    fn remove_repo(&self, name: &str) -> Result<()> {
        self.repo_list.lock().unwrap().retain(|x| x.name != name);
        self.store_atomic()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Repo;

    fn temp_db_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_db.ron");
        (dir, path)
    }

    #[test]
    fn test_db_basic_operation() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = Db::create_or_open(temp_dir.path().join("test.ron"))?;
        db.insert_repo(
            Repo::new("bpm")
                .by_url("https://github.com/lxl66566/bpm-rs/")
                .unwrap(),
        )?;
        db.insert_repo(
            Repo::new("abd")
                .by_url("https://github.com/lxl6656645/b132/")
                .unwrap(),
        )?;

        let all = db.get_repo_list();
        assert_eq!(all.len(), 2);

        let found = db.get_repo("bpm").unwrap();
        assert_eq!(found.name, "bpm");
        assert_eq!(found.repo_owner.unwrap(), "lxl66566");

        assert!(db.get_repo("nonexistent").is_none());

        db.remove_repo("bpm").unwrap();
        assert!(db.get_repo("bpm").is_none());
        assert!(db.get_repo("abd").is_some());
        assert_eq!(db.get_repo_list().len(), 1);
        Ok(())
    }

    #[test]
    fn db_persistence() {
        let (_dir, path) = temp_db_path();

        {
            let db = Db::create_or_open(&path).unwrap();
            db.insert_repo(
                Repo::new("persist-test")
                    .by_url("https://github.com/test/repo")
                    .unwrap(),
            )
            .unwrap();
        }

        let db2 = Db::create_or_open(&path).unwrap();
        let found = db2.get_repo("persist-test").unwrap();
        assert_eq!(found.repo_owner.unwrap(), "test");
        assert_eq!(found.repo_name.unwrap(), "repo");
    }

    /// The db lock is re-entrant within a process (multiple handles may
    /// coexist, e.g. a test holding one while dispatch opens another), but
    /// exclusive across processes. After all handles drop, the lock is
    /// available again.
    #[test]
    fn db_lock_is_reentrant_in_process() {
        let (_dir, path) = temp_db_path();
        let _db1 = Db::create_or_open(&path).unwrap();
        let _db2 = Db::create_or_open(&path).unwrap();
        drop(_db1);
        drop(_db2);
        assert!(Db::create_or_open(&path).is_ok());
    }

    /// Regression test: `insert_repo` must upsert (overwrite by name) instead
    /// of always appending. This mirrors what `cli_update` does — it
    /// re-inserts the same repo after an update — and previously produced
    /// duplicate entries in the db.
    #[test]
    fn db_insert_repo_upsert_no_duplicates() -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, path) = temp_db_path();
        let db = Db::create_or_open(&path)?;

        let mut repo = Repo::new("abc")
            .by_url("https://github.com/owner/abc")
            .unwrap();
        repo.version = Some("v1.0.0".into());
        db.insert_repo(repo.clone())?;

        // Simulate an update: bump version then re-insert.
        let mut updated = repo.clone();
        updated.version = Some("v2.0.0".into());
        db.insert_repo(updated)?;

        let all = db.get_repo_list();
        assert_eq!(all.len(), 1, "update must not create a duplicate repo");
        let got = db.get_repo("abc").unwrap();
        assert_eq!(got.version.as_deref(), Some("v2.0.0"));

        // A second, different repo must not be affected.
        db.insert_repo(
            Repo::new("xyz")
                .by_url("https://github.com/owner/xyz")
                .unwrap(),
        )?;
        assert_eq!(db.get_repo_list().len(), 2);
        Ok(())
    }
}
