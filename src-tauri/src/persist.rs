//! Rust-owned JSON persistence: atomic writes, corrupt-file recovery, and a small
//! generic store for features that keep their own file (never AppState).
//!
//! Every persisted file follows the same rules:
//! - missing file → `T::default()`;
//! - unparseable file → moved aside to `*.json.corrupt.bak` (so the next write cannot
//!   overwrite the user's real data), then `T::default()`;
//! - any other IO error (permissions, AV sharing violation) is returned, so startup
//!   fails rather than silently losing state;
//! - writes go to `*.json.tmp` first, are flushed to disk, then renamed over the target.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::{Mutex, RwLock};
use tracing::warn;

use crate::error::IpcError;

/// Read `path` as JSON with the recovery rules in the module docs.
pub(crate) async fn read_json<T: DeserializeOwned + Default>(path: &Path) -> anyhow::Result<T> {
    match tokio::fs::read(path).await {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Ok(value),
            Err(e) => {
                let backup = path.with_extension("json.corrupt.bak");
                let _ = tokio::fs::rename(path, &backup).await;
                warn!(
                    path = %path.display(),
                    backup = %backup.display(),
                    error = %e,
                    "stored file is corrupt; moved aside, using defaults"
                );
                Ok(T::default())
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(anyhow::Error::new(e).context(format!("failed to read {}", path.display()))),
    }
}

/// Write `value` to `path` as pretty JSON, atomically (temp file + rename) and durably:
/// the temp file reaches the disk before it replaces the target, so a crash or power
/// loss right after a write leaves the old file or the new one, never a zero-filled
/// one (NTFS and APFS can otherwise commit the rename before the data). Callers
/// serialize concurrent writes to the same path themselves (a lock per file).
pub(crate) async fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), IpcError> {
    let json = serde_json::to_vec_pretty(value)
        .map_err(|e| IpcError::Storage(format!("Could not serialize {}: {e}", path.display())))?;
    write_bytes(path, json).await
}

/// Write `bytes` to `path` the way [`write_json`] does: atomically and durably.
pub(crate) async fn write_bytes(path: &Path, bytes: Vec<u8>) -> Result<(), IpcError> {
    // `<name>.tmp` next to the target (`settings.json.tmp`)
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    // std::fs in one blocking task, so every error of the data write comes back here (a
    // tokio File reports a failed write only on a later flush) and the handle is closed
    // before the rename
    let written = {
        let tmp = tmp.clone();
        tokio::task::spawn_blocking(move || {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
    };
    written.map_err(|e| IpcError::Storage(format!("Could not write {}: {e}", tmp.display())))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| IpcError::Storage(format!("Could not replace {}: {e}", path.display())))?;
    Ok(())
}

/// One JSON file plus its in-memory copy, for a feature that owns its own state
/// (library, profile, …). Register it with `app.manage(...)` in `lib.rs` `.setup()`
/// instead of growing `AppState`. Load per-profile files from
/// [`AppState::profile_dir`](crate::state::AppState::profile_dir) and caches from the
/// cache dir.
///
/// New files should carry a `{"version": N, ...}` envelope so their schema can be
/// migrated at load time.
pub(crate) struct JsonStore<T> {
    /// Changes only under `write_lock` (`switch_to`, when the active profile changes).
    path: std::sync::RwLock<PathBuf>,
    value: RwLock<T>,
    /// Serializes `update`s, so two writers can neither lose each other's change nor
    /// race on the temp file.
    write_lock: Mutex<()>,
}

impl<T> JsonStore<T>
where
    T: Clone + Default + Serialize + DeserializeOwned,
{
    /// Load `path` (creating its parent directory), with the recovery rules above.
    pub(crate) async fn load(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let value = read_json(&path).await?;
        Ok(Self {
            path: std::sync::RwLock::new(path),
            value: RwLock::new(value),
            write_lock: Mutex::new(()),
        })
    }

    fn path(&self) -> PathBuf {
        self.path.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Point the store at another file (another profile's), loading it with the same
    /// recovery rules. Serialized with every update, so no write lands in the old file
    /// after the switch, nor in the new one before it.
    pub(crate) async fn switch_to(&self, path: PathBuf) -> anyhow::Result<()> {
        let _guard = self.write_lock.lock().await;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let value = read_json(&path).await?;
        *self.value.write().await = value;
        *self.path.write().unwrap_or_else(|e| e.into_inner()) = path;
        Ok(())
    }

    /// A snapshot of the current value.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the library store reads through `read`")
    )]
    pub(crate) async fn get(&self) -> T {
        self.value.read().await.clone()
    }

    /// Apply `f` to a copy of the value, persist it, then publish it. If the write
    /// fails, the in-memory value is left unchanged, so memory never runs ahead of disk.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the library store updates through `try_update`")
    )]
    pub(crate) async fn update(&self, f: impl FnOnce(&mut T)) -> Result<T, IpcError> {
        let _guard = self.write_lock.lock().await;
        let mut next = self.value.read().await.clone();
        f(&mut next);
        write_json(&self.path(), &next).await?;
        *self.value.write().await = next.clone();
        Ok(next)
    }

    /// Run `f` on the current value without copying it (for large values such as the
    /// library).
    pub(crate) async fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&*self.value.read().await)
    }

    /// Like [`update`](Self::update), but `f` decides what happens: `Ok(Some(r))`
    /// persists and publishes the changed copy and returns `r`; `Ok(None)` means nothing
    /// changed, so nothing is written; `Err` aborts without a write. Serialized with
    /// every other update, with the same memory-never-ahead-of-disk rule.
    pub(crate) async fn try_update<R>(
        &self,
        f: impl FnOnce(&mut T) -> Result<Option<R>, IpcError>,
    ) -> Result<Option<R>, IpcError> {
        let _guard = self.write_lock.lock().await;
        let mut next = self.value.read().await.clone();
        let Some(result) = f(&mut next)? else {
            return Ok(None);
        };
        write_json(&self.path(), &next).await?;
        *self.value.write().await = next;
        Ok(Some(result))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde::Deserialize;

    /// A fresh, empty `<tmp>/zeiler-<test>-<pid>` directory.
    pub(crate) fn temp_dir(test: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("zeiler-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", default)]
    struct Counter {
        version: u32,
        hits: u32,
    }

    #[tokio::test]
    async fn missing_file_reads_as_defaults() {
        let dir = temp_dir("persist-missing");
        let value: Counter = read_json(&dir.join("counter.json")).await.unwrap();
        assert_eq!(value, Counter::default());
    }

    #[tokio::test]
    async fn corrupt_file_is_moved_aside_and_defaults_are_used() {
        let dir = temp_dir("persist-corrupt");
        let path = dir.join("counter.json");
        std::fs::write(&path, "{ not json").unwrap();

        let value: Counter = read_json(&path).await.unwrap();

        assert_eq!(value, Counter::default());
        assert!(
            !path.exists(),
            "the corrupt file must not stay where a write lands"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("counter.json.corrupt.bak")).unwrap(),
            "{ not json"
        );
    }

    #[tokio::test]
    async fn write_is_atomic_and_round_trips() {
        let dir = temp_dir("persist-write");
        let path = dir.join("counter.json");
        let value = Counter {
            version: 1,
            hits: 3,
        };

        write_json(&path, &value).await.unwrap();

        assert!(
            !dir.join("counter.json.tmp").exists(),
            "temp file renamed away"
        );
        let back: Counter = read_json(&path).await.unwrap();
        assert_eq!(back, value);
    }

    #[tokio::test]
    async fn json_store_updates_persist_and_reload() {
        let dir = temp_dir("persist-store");
        let path = dir.join("nested").join("counter.json");

        let store = JsonStore::<Counter>::load(path.clone()).await.unwrap();
        assert_eq!(store.get().await, Counter::default());

        let after = store.update(|c| c.hits += 2).await.unwrap();
        assert_eq!(after.hits, 2);
        assert_eq!(store.get().await.hits, 2);

        let reloaded = JsonStore::<Counter>::load(path).await.unwrap();
        assert_eq!(reloaded.get().await.hits, 2);
    }

    #[tokio::test]
    async fn json_store_switches_files_and_writes_only_the_new_one() {
        let dir = temp_dir("persist-switch");
        let (a, b) = (dir.join("a/counter.json"), dir.join("b/counter.json"));
        let store = JsonStore::<Counter>::load(a.clone()).await.unwrap();
        store.update(|c| c.hits = 1).await.unwrap();

        store.switch_to(b.clone()).await.unwrap();
        assert_eq!(
            store.get().await,
            Counter::default(),
            "the new file's value"
        );
        store.update(|c| c.hits = 7).await.unwrap();

        let read = |p: &Path| std::fs::read_to_string(p).unwrap();
        assert!(read(&a).contains("\"hits\": 1"));
        assert!(read(&b).contains("\"hits\": 7"));
        store.switch_to(a).await.unwrap();
        assert_eq!(store.get().await.hits, 1);
    }

    #[tokio::test]
    async fn json_store_concurrent_updates_are_not_lost() {
        let dir = temp_dir("persist-store-concurrent");
        let path = dir.join("counter.json");
        let store = std::sync::Arc::new(JsonStore::<Counter>::load(path.clone()).await.unwrap());

        let tasks: Vec<_> = (0..16)
            .map(|_| {
                let store = store.clone();
                tokio::spawn(async move { store.update(|c| c.hits += 1).await.unwrap() })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }

        assert_eq!(store.get().await.hits, 16);
        let on_disk: Counter = read_json(&path).await.unwrap();
        assert_eq!(on_disk.hits, 16);
    }

    #[tokio::test]
    async fn try_update_writes_only_changes() {
        let dir = temp_dir("persist-store-try");
        let path = dir.join("counter.json");
        let store = JsonStore::<Counter>::load(path.clone()).await.unwrap();

        let unchanged = store.try_update(|_| Ok(None::<()>)).await.unwrap();
        assert_eq!(unchanged, None);
        assert!(!path.exists(), "nothing changed, nothing written");

        let refused = store
            .try_update(|c| {
                c.hits = 5;
                Err::<Option<()>, _>(IpcError::InvalidInput("no".into()))
            })
            .await;
        assert!(refused.is_err());
        assert_eq!(
            store.read(|c| c.hits).await,
            0,
            "an error discards the copy"
        );

        let changed = store
            .try_update(|c| {
                c.hits = 7;
                Ok(Some(c.hits * 2))
            })
            .await
            .unwrap();
        assert_eq!(changed, Some(14));
        assert_eq!(store.read(|c| c.hits).await, 7);
        let on_disk: Counter = read_json(&path).await.unwrap();
        assert_eq!(on_disk.hits, 7);
    }

    #[tokio::test]
    async fn json_store_failed_write_leaves_memory_unchanged() {
        let dir = temp_dir("persist-store-fail");
        let path = dir.join("counter.json");
        let store = JsonStore::<Counter>::load(path.clone()).await.unwrap();
        // a directory where the temp file should go makes the write fail
        std::fs::create_dir_all(dir.join("counter.json.tmp")).unwrap();

        let result = store.update(|c| c.hits = 9).await;

        assert!(matches!(result, Err(IpcError::Storage(_))));
        assert_eq!(store.get().await.hits, 0);
    }
}
