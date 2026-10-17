//! Opt-in Stremio account sync, per profile.
//!
//! Signing in (a link code in the browser, or email and password) stores the session
//! key in the OS keychain (or memory, `secrets.rs`) and the non-secret account details
//! in the profile's `account.json`. While signed in:
//!
//! - **Library** sync (on by default once signed in) follows stremio-core: per item, the
//!   newer `_mtime` wins. It runs at sign-in, at startup, every 15 minutes, a minute
//!   after local library changes, and on "Sync now".
//! - **Addon** sync (off until the user chooses, at sign-in) makes the account's
//!   collection the source of truth: a pull replaces the user's addons with the
//!   account's (Zeiler's built-ins stay under Settings → Content, never imported or
//!   pushed), and every local add, remove or reorder is replayed onto a fresh copy of
//!   the account's collection. A failed pull pauses pushing (`addonsLocked`), so a stale
//!   list never overwrites the account.
//!
//! Nothing is sent while signed out. Sync errors never block local use.

pub mod secrets;

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, serde_as};
use tokio::sync::Notify;
use url::Url;
use zeiler_addon::Descriptor;
use zeiler_sync::sync::{
    AddonOp, Builtins, FirstSync, importable, plan_library_sync, rebase_addon_op,
};
use zeiler_sync::{AuthKey, RawDescriptor, StremioApi, SyncError, UserSummary};

use crate::error::IpcError;
use crate::library::LibraryState;
use crate::persist::JsonStore;
use crate::state::AppState;

use self::secrets::{Sessions, Storage};

pub const ACCOUNT_FILE: &str = "account.json";
const FILE_VERSION: u32 = 1;

/// A profile's account details: never the session key.
#[serde_as]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountFile {
    pub version: u32,
    #[serde_as(deserialize_as = "DefaultOnError")]
    pub user: Option<UserSummary>,
    pub sync_library: bool,
    pub sync_addons: bool,
    /// The sign-in's addon question was answered (sync them, or not).
    pub addons_choice_made: bool,
    /// Unix milliseconds.
    pub last_library_sync: Option<i64>,
    pub last_addons_sync: Option<i64>,
}

/// What the UI shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub signed_in: bool,
    pub user: Option<AccountUser>,
    /// Where the session key is kept.
    pub storage: Option<Storage>,
    pub sync_library: bool,
    pub sync_addons: bool,
    /// Signed in, but the addon question wasn't answered yet.
    pub needs_addon_choice: bool,
    pub last_library_sync: Option<i64>,
    pub last_addons_sync: Option<i64>,
    /// Addon pushes are paused: the last pull failed.
    pub addons_locked: bool,
    /// The last sync error, as a sentence.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountUser {
    pub id: String,
    pub email: String,
    pub premium: bool,
}

/// What a sync did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub pulled: usize,
    pub pushed: usize,
    pub addons: Option<usize>,
}

/// The account sync state, managed (`app.manage`).
pub struct AccountState {
    pub api: StremioApi,
    pub sessions: Sessions,
    /// The active profile's `account.json` (switched with the profile).
    pub file: JsonStore<AccountFile>,
    pub addons_locked: AtomicBool,
    /// A sync is writing pulled items into the library right now: the change events
    /// that causes don't ask for another sync (set only around those writes, so the
    /// user's own changes during a sync still do).
    pub syncing: AtomicBool,
    pub last_error: Mutex<Option<String>>,
    /// One sync at a time.
    pub sync_lock: tokio::sync::Mutex<()>,
    /// Wakes the background sync (a local library change, a sign-in).
    pub kick: Notify,
    /// Zeiler's built-ins, never pushed into an account.
    pub builtins: Builtins,
    /// Local addon changes not yet in the account, oldest first. Appended by the addon
    /// commands under `profile_op` (so in the order they happened), drained by a push
    /// under `sync_lock`. A pull doesn't replace the local list while any are left, so
    /// a change that couldn't be sent is never undone by the next pull.
    pub pending: Mutex<Vec<AddonOp>>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// IPC errors for sync failures: messages name Stremio, never a URL or the key.
pub fn ipc_error(error: SyncError) -> IpcError {
    match error {
        SyncError::Network(_) => IpcError::Network(error.to_string()),
        SyncError::Auth(message) => IpcError::Auth(message),
        SyncError::SessionExpired => IpcError::SessionExpired(error.to_string()),
        other => IpcError::Network(other.to_string()),
    }
}

impl AccountState {
    pub async fn load(
        profile_dir: &Path,
        api: StremioApi,
        sessions: Sessions,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            api,
            sessions,
            file: JsonStore::load(profile_dir.join(ACCOUNT_FILE)).await?,
            addons_locked: AtomicBool::new(false),
            syncing: AtomicBool::new(false),
            last_error: Mutex::new(None),
            sync_lock: tokio::sync::Mutex::new(()),
            kick: Notify::new(),
            builtins: crate::builtin::builtins_for_sync(),
            pending: Mutex::new(Vec::new()),
        })
    }

    /// Follow a profile switch. The caller holds `profile_op` and `sync_lock` for the
    /// whole switch, so no sync runs with one profile's key and another's library.
    pub async fn switch_to(&self, profile_dir: &Path) -> anyhow::Result<()> {
        self.file.switch_to(profile_dir.join(ACCOUNT_FILE)).await?;
        self.addons_locked.store(false, Ordering::SeqCst);
        self.set_error(None);
        Ok(())
    }

    /// Queue local addon changes for the account (called under `profile_op`).
    pub fn queue(&self, ops: Vec<AddonOp>) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(ops);
    }

    pub fn clear_pending(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    fn pending_ops(&self) -> Vec<AddonOp> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_error(&self, error: Option<String>) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = error;
    }

    /// The active profile's session key, if signed in.
    pub fn key(&self, profile_id: &str) -> Option<(AuthKey, Storage)> {
        let (secret, storage) = self.sessions.load(profile_id)?;
        Some((AuthKey::new(secret)?, storage))
    }

    pub async fn status(&self, profile_id: &str) -> AccountStatus {
        let file = self.file.get().await;
        let key = self.key(profile_id);
        let signed_in = key.is_some() && file.user.is_some();
        AccountStatus {
            signed_in,
            user: file
                .user
                .as_ref()
                .filter(|_| signed_in)
                .map(|u| AccountUser {
                    id: u.id.clone(),
                    email: u.email.clone(),
                    premium: u.premium_expire.is_some(),
                }),
            storage: key.map(|(_, s)| s).filter(|_| signed_in),
            sync_library: signed_in && file.sync_library,
            sync_addons: signed_in && file.sync_addons,
            needs_addon_choice: signed_in && !file.addons_choice_made,
            last_library_sync: file.last_library_sync,
            last_addons_sync: file.last_addons_sync,
            addons_locked: self.addons_locked.load(Ordering::SeqCst),
            last_error: self
                .last_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        }
    }

    /// A session ended on Stremio's side: forget it. The library stays, still marked
    /// as that account's (signing in again continues it; another account starts its
    /// own, see `LibraryState::prepare_for_account`).
    pub async fn session_ended(&self, profile_id: &str) {
        self.sessions.forget(profile_id);
        let _ = self.file.update(|f| f.user = None).await;
        self.set_error(Some(SyncError::SessionExpired.to_string()));
    }
}

/// Sign-in finished: keep the key, remember the user, switch library sync on.
pub async fn signed_in(
    account: &AccountState,
    profile_id: &str,
    key: AuthKey,
    user: UserSummary,
    remember: bool,
) -> Result<(), IpcError> {
    account.sessions.store(profile_id, key.expose(), remember);
    let same_user = account
        .file
        .get()
        .await
        .user
        .is_some_and(|u| u.id == user.id);
    account
        .file
        .update(|f| {
            if !same_user {
                // another account: its addon question is asked again
                f.addons_choice_made = false;
                f.sync_addons = false;
                f.last_addons_sync = None;
                f.last_library_sync = None;
            }
            f.version = FILE_VERSION;
            f.user = Some(user);
            f.sync_library = true;
        })
        .await?;
    account.addons_locked.store(false, Ordering::SeqCst);
    account.set_error(None);
    account.kick.notify_one();
    Ok(())
}

// ---- library ---------------------------------------------------------------------------

/// Pull what's newer in the account, push what's newer here.
pub async fn sync_library(
    account: &AccountState,
    library: &LibraryState,
    key: &AuthKey,
    uid: &str,
) -> Result<(usize, usize), SyncError> {
    account.syncing.store(true, Ordering::SeqCst);
    let prepared = library.prepare_for_account(uid).await;
    account.syncing.store(false, Ordering::SeqCst);
    prepared.map_err(|e| SyncError::Unexpected(e.to_string()))?;
    let (_, local) = library.sync_snapshot().await;
    let now = crate::library::now();
    let remote = account.api.datastore_meta(key).await?;
    let plan = plan_library_sync(&local, &remote, now);
    let pulled = if plan.pull.is_empty() {
        Vec::new()
    } else if plan.pull.len() == remote.len() {
        // everything (a first sync): one request for the lot
        account.api.datastore_get(key, &[], true).await?
    } else {
        account.api.datastore_get(key, &plan.pull, false).await?
    };
    account.syncing.store(true, Ordering::SeqCst);
    let merged = library.merge_remote(uid, pulled).await;
    account.syncing.store(false, Ordering::SeqCst);
    let changed = merged.map_err(|e| SyncError::Unexpected(e.to_string()))?;
    let push: Vec<_> = plan
        .push
        .iter()
        .filter_map(|id| local.get(id).cloned())
        .collect();
    account.api.datastore_put(key, &push).await?;
    Ok((changed.len(), push.len()))
}

// ---- addons ----------------------------------------------------------------------------

/// A local descriptor as an account collection entry (flags: neither official nor
/// protected).
pub fn to_raw(descriptor: &Descriptor) -> Option<RawDescriptor> {
    Some(RawDescriptor {
        manifest: serde_json::to_value(&descriptor.manifest).ok()?,
        transport_url: descriptor.transport_url.to_string(),
        flags: Some(serde_json::json!({ "official": false, "protected": false })),
        extra: Default::default(),
    })
}

/// An account entry as a local descriptor, when Zeiler can use it (an http(s)
/// manifest URL and a manifest that parses). Remote flags are not kept locally.
pub fn from_raw(raw: &RawDescriptor) -> Option<Descriptor> {
    let transport_url = Url::parse(&raw.transport_url).ok()?;
    zeiler_addon::validate_transport_url(&transport_url).ok()?;
    let manifest = serde_json::from_value(raw.manifest.clone()).ok()?;
    Some(Descriptor {
        manifest,
        transport_url,
        flags: Default::default(),
    })
}

/// Replace the user's addons with the account's (built-ins and entries Zeiler can't
/// use left out). Changes still waiting to be sent go first; while any remain, the
/// local list is left alone. Locks addon pushes when the pull fails. Returns how many
/// addons the profile now has. The caller holds `sync_lock`; the list is written under
/// `profile_op` (lock order: `sync_lock`, then `profile_op`).
pub async fn pull_addons(
    account: &AccountState,
    state: &AppState,
    key: &AuthKey,
) -> Result<usize, SyncError> {
    push_pending(account, key).await?;
    let remote = match account.api.addon_collection_get(key).await {
        Ok(remote) => remote,
        Err(e) => {
            account.addons_locked.store(true, Ordering::SeqCst);
            return Err(e);
        }
    };
    let addons: Vec<Descriptor> = importable(&remote.addons, &account.builtins)
        .iter()
        .filter(|raw| usable(raw))
        .filter_map(from_raw)
        .collect();
    let count = addons.len();
    {
        let _profile = state.profile_op.lock().await;
        // a change made while the collection was being fetched wins: it is sent with
        // the next push, and the next pull sees it
        if !account.pending_ops().is_empty() {
            return Ok(state.addons.read().await.len());
        }
        *state.addons.write().await = addons;
        state
            .persist_addons()
            .await
            .map_err(|e| SyncError::Unexpected(e.to_string()))?;
    }
    account.addons_locked.store(false, Ordering::SeqCst);
    Ok(count)
}

/// Whether Zeiler installs an account entry as one of the user's addons: not one that
/// Stremio protects (its own, like Local Files) and not a loopback address (an addon
/// served by a local Stremio). They stay in the account untouched.
pub fn usable(raw: &RawDescriptor) -> bool {
    if raw.is_protected() {
        return false;
    }
    let Ok(url) = Url::parse(&raw.transport_url) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Domain(d)) => !d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => !ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => !ip.is_loopback(),
        None => false,
    }
}

/// Send the queued local changes, in order, as one rebase of the account's current
/// collection. They leave the queue only once sent.
pub async fn push_pending(account: &AccountState, key: &AuthKey) -> Result<(), SyncError> {
    let ops = account.pending_ops();
    if ops.is_empty() {
        return Ok(());
    }
    push_addon_ops(account, key, &ops).await?;
    let mut pending = account.pending.lock().unwrap_or_else(|e| e.into_inner());
    let sent = ops.len().min(pending.len());
    pending.drain(..sent);
    Ok(())
}

/// Replay local addon changes, in order, onto the account's current collection.
pub async fn push_addon_ops(
    account: &AccountState,
    key: &AuthKey,
    ops: &[AddonOp],
) -> Result<(), SyncError> {
    if account.addons_locked.load(Ordering::SeqCst) {
        return Err(SyncError::Unexpected(
            "addon sync is paused until Zeiler can read your account's addons again".into(),
        ));
    }
    let remote = account.api.addon_collection_get(key).await?;
    let next = ops.iter().fold(remote.addons.clone(), |list, op| {
        rebase_addon_op(&list, op, &account.builtins)
    });
    if next != remote.addons {
        account.api.addon_collection_set(key, &next).await?;
    }
    Ok(())
}

/// The first sign-in's addon choice, applied: the account's list (`UseAccount`), or the
/// account's plus the local ones it lacks, uploaded (`Merge`).
pub async fn first_addon_sync(
    account: &AccountState,
    state: &AppState,
    key: &AuthKey,
    choice: FirstSync,
) -> Result<usize, SyncError> {
    let remote = account.api.addon_collection_get(key).await?;
    let local: Vec<RawDescriptor> = state
        .addons
        .read()
        .await
        .iter()
        .filter_map(to_raw)
        .collect();
    let next = zeiler_sync::sync::first_sync(&local, &remote.addons, choice, &account.builtins);
    if next != remote.addons {
        account.api.addon_collection_set(key, &next).await?;
    }
    pull_addons(account, state, key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Seen = Arc<std::sync::Mutex<Vec<(String, Value)>>>;

    /// A mock of api.strem.io: answers each method with a canned `result`, and records
    /// every request body. No real account is involved.
    async fn mock(routes: Vec<(&'static str, Value)>) -> (StremioApi, Seen) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (routes, log) = (routes.clone(), log.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let (end, length) = loop {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                            let length = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (end + 4, length);
                        }
                    };
                    while buf.len() < end + length {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                    let body = serde_json::from_slice(&buf[end..]).unwrap_or(Value::Null);
                    log.lock().unwrap().push((path.clone(), body));
                    let answer = routes
                        .iter()
                        .find(|(p, _)| path == format!("/api/{p}"))
                        .map(|(_, v)| json!({ "result": v }).to_string())
                        .unwrap_or_default();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        let api = StremioApi::with_bases(
            Url::parse(&format!("{base}/api/")).unwrap(),
            Url::parse(&format!("{base}/link/")).unwrap(),
            false,
        )
        .unwrap();
        (api, seen)
    }

    fn item(id: &str, mtime: &str, name: &str) -> Value {
        json!({
            "_id": id, "name": name, "type": "movie", "removed": false, "temp": false,
            "_ctime": mtime, "_mtime": mtime,
            "state": { "timeOffset": 0, "timeWatched": 0, "overallTimeWatched": 0, "timesWatched": 0,
                       "flaggedWatched": 0, "duration": 0, "noNotif": false },
            "behaviorHints": { "hasScheduledVideos": false }
        })
    }

    async fn account_in(dir: &Path, api: StremioApi) -> AccountState {
        AccountState::load(
            dir,
            api,
            Sessions::new(Box::new(secrets::MemoryStore::default())),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn library_sync_pulls_newer_items_and_pushes_local_ones() {
        let dir = crate::persist::tests::temp_dir("account-library-sync");
        std::fs::write(
            dir.join("library.json"),
            json!({ "version": 1, "uid": null, "items": {
                "tt1": item("tt1", "2026-10-01T00:00:00Z", "Old name"),
                "tt2": item("tt2", "2026-10-05T00:00:00Z", "Only here")
            }})
            .to_string(),
        )
        .unwrap();
        let library = LibraryState::load(&dir, |_| {}).await.unwrap();
        let (api, seen) = mock(vec![
            (
                "datastoreMeta",
                json!([
                    ["tt1", 1_759_700_000_000_i64],
                    ["tt3", 1_759_000_000_000_i64]
                ]),
            ),
            (
                "datastoreGet",
                json!([
                    item("tt1", "2025-10-05T21:33:20Z", "New name"),
                    item("tt3", "2025-09-27T19:06:40Z", "Only there")
                ]),
            ),
            ("datastorePut", json!({ "success": true })),
        ])
        .await;
        let account = account_in(&dir, api).await;
        let key = AuthKey::new("k").unwrap();

        let (pulled, pushed) = sync_library(&account, &library, &key, "user-1")
            .await
            .unwrap();

        let (uid, items) = library.sync_snapshot().await;
        assert_eq!(uid.as_deref(), Some("user-1"));
        assert_eq!(items["tt3"].name, "Only there");
        // tt1: the account's copy is older than the local one, so the local one stays
        // and goes up
        assert_eq!(items["tt1"].name, "Old name");
        assert_eq!((pulled, pushed), (1, 2));
        let seen = seen.lock().unwrap().clone();
        let get = seen.iter().find(|(p, _)| p == "/api/datastoreGet").unwrap();
        assert_eq!(get.1["ids"], json!(["tt3"]));
        let put = seen.iter().find(|(p, _)| p == "/api/datastorePut").unwrap();
        let ids: Vec<_> = put.1["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["_id"].clone())
            .collect();
        assert_eq!(ids, [json!("tt1"), json!("tt2")]);
    }

    #[tokio::test]
    async fn addon_pull_imports_all_but_builtins_and_pushes_rebase() {
        let dir = crate::persist::tests::temp_dir("account-addons");
        let state = AppState::load(dir.join("data"), dir.join("cache"))
            .await
            .unwrap();
        let collection = json!({ "addons": [
            { "manifest": { "id": "com.linvo.cinemeta", "name": "Cinemeta" },
              "transportUrl": "https://v3-cinemeta.strem.io/manifest.json",
              "flags": { "official": true, "protected": true } },
            { "manifest": { "id": "org.a", "name": "A" }, "transportUrl": "https://a.example/manifest.json",
              "flags": { "official": false, "protected": false } },
            { "manifest": { "id": "org.local", "name": "Local" }, "transportUrl": "ftp://nope/manifest.json" }
        ], "lastModified": "2026-10-01T00:00:00Z" });
        let (api, seen) = mock(vec![
            ("addonCollectionGet", collection),
            ("addonCollectionSet", json!({ "success": true })),
        ])
        .await;
        let account = account_in(&dir, api).await;
        let key = AuthKey::new("k").unwrap();

        assert_eq!(pull_addons(&account, &state, &key).await.unwrap(), 1);
        let local = state.addons.read().await.clone();
        assert_eq!(local[0].manifest.id, "org.a");

        // adding a built-in never reaches the account; adding another addon does
        let cinemeta: Descriptor = serde_json::from_value(json!({
            "manifest": { "id": "com.linvo.cinemeta", "name": "Cinemeta" },
            "transportUrl": "https://elsewhere.example/manifest.json"
        }))
        .unwrap();
        push_addon_ops(&account, &key, &[AddonOp::Add(to_raw(&cinemeta).unwrap())])
            .await
            .unwrap();
        assert!(
            !seen
                .lock()
                .unwrap()
                .iter()
                .any(|(p, _)| p == "/api/addonCollectionSet")
        );
        let b: Descriptor = serde_json::from_value(json!({
            "manifest": { "id": "org.b", "name": "B" }, "transportUrl": "https://b.example/manifest.json"
        }))
        .unwrap();
        push_addon_ops(&account, &key, &[AddonOp::Add(to_raw(&b).unwrap())])
            .await
            .unwrap();
        let seen = seen.lock().unwrap().clone();
        let set = seen
            .iter()
            .find(|(p, _)| p == "/api/addonCollectionSet")
            .unwrap();
        let urls: Vec<_> = set.1["addons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["transportUrl"].clone())
            .collect();
        assert_eq!(
            urls.len(),
            4,
            "the account's own entries are kept, even unusable ones"
        );
        assert_eq!(urls[3], json!("https://b.example/manifest.json"));
        assert_eq!(set.1["addons"][0]["flags"]["protected"], true);
    }

    #[tokio::test]
    async fn queued_changes_go_first_and_a_pull_never_undoes_them() {
        let dir = crate::persist::tests::temp_dir("account-pending");
        let state = AppState::load(dir.join("data"), dir.join("cache"))
            .await
            .unwrap();
        let mine: Descriptor = serde_json::from_value(json!({
            "manifest": { "id": "org.mine", "name": "Mine" }, "transportUrl": "https://mine.example/manifest.json"
        }))
        .unwrap();
        *state.addons.write().await = vec![mine.clone()];
        // the account doesn't answer addonCollectionSet: the push fails
        let (api, _) = mock(vec![(
            "addonCollectionGet",
            json!({ "addons": [], "lastModified": null }),
        )])
        .await;
        let account = account_in(&dir, api).await;
        let key = AuthKey::new("k").unwrap();
        account.queue(vec![AddonOp::Add(to_raw(&mine).unwrap())]);

        assert!(pull_addons(&account, &state, &key).await.is_err());
        assert_eq!(
            state.addons.read().await.len(),
            1,
            "the local install survives"
        );
        assert_eq!(account.pending.lock().unwrap().len(), 1, "still queued");
    }

    #[test]
    fn protected_and_loopback_entries_stay_in_the_account() {
        let entry = |url: &str, protected: bool| -> RawDescriptor {
            serde_json::from_value(json!({
                "manifest": { "id": "x", "name": "X" }, "transportUrl": url,
                "flags": { "official": false, "protected": protected }
            }))
            .unwrap()
        };
        assert!(usable(&entry("https://a.example/manifest.json", false)));
        assert!(!usable(&entry("https://a.example/manifest.json", true)));
        assert!(!usable(&entry(
            "http://127.0.0.1:11470/local-addon/manifest.json",
            false
        )));
        assert!(!usable(&entry(
            "http://localhost:7000/manifest.json",
            false
        )));
        assert!(!usable(&entry("http://[::1]:7000/manifest.json", false)));
    }

    #[tokio::test]
    async fn another_accounts_library_is_kept_aside_not_uploaded() {
        let dir = crate::persist::tests::temp_dir("account-other-library");
        std::fs::write(
            dir.join("library.json"),
            json!({ "version": 1, "uid": "user-a", "items": {
                "tt1": item("tt1", "2026-10-01T00:00:00Z", "A's title")
            }})
            .to_string(),
        )
        .unwrap();
        let library = LibraryState::load(&dir, |_| {}).await.unwrap();
        let (api, seen) = mock(vec![
            ("datastoreMeta", json!([])),
            ("datastorePut", json!({ "success": true })),
        ])
        .await;
        let account = account_in(&dir, api).await;
        let key = AuthKey::new("k").unwrap();

        sync_library(&account, &library, &key, "user-b")
            .await
            .unwrap();

        let (uid, items) = library.sync_snapshot().await;
        assert_eq!(uid.as_deref(), Some("user-b"));
        assert!(items.is_empty());
        let kept = std::fs::read_to_string(dir.join("library-user_a.json")).unwrap();
        assert!(kept.contains("A's title"));
        let pushed = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, b)| {
                p == "/api/datastorePut" && !b["changes"].as_array().unwrap().is_empty()
            })
            .count();
        assert_eq!(pushed, 0, "nothing of account A reaches account B");
    }

    #[tokio::test]
    async fn a_failed_addon_pull_pauses_pushes() {
        let dir = crate::persist::tests::temp_dir("account-locked");
        let state = AppState::load(dir.join("data"), dir.join("cache"))
            .await
            .unwrap();
        let (api, _) = mock(vec![]).await; // every method answers with nothing usable
        let account = account_in(&dir, api).await;
        let key = AuthKey::new("k").unwrap();
        assert!(pull_addons(&account, &state, &key).await.is_err());
        assert!(account.addons_locked.load(Ordering::SeqCst));
        assert!(
            push_addon_ops(&account, &key, &[AddonOp::Remove("x".into())])
                .await
                .is_err()
        );
    }

    #[test]
    fn descriptors_cross_to_the_account_and_back() {
        let local: Descriptor = serde_json::from_value(serde_json::json!({
            "manifest": { "id": "org.a", "name": "A", "types": ["movie"], "resources": ["stream"] },
            "transportUrl": "https://a.example/key/manifest.json",
            "flags": { "official": true, "protected": true }
        }))
        .unwrap();
        let raw = to_raw(&local).unwrap();
        assert_eq!(raw.transport_url, "https://a.example/key/manifest.json");
        assert!(
            !raw.is_protected(),
            "a local addon is never pushed as protected"
        );
        let back = from_raw(&raw).unwrap();
        assert_eq!(back.manifest.id, "org.a");
        assert!(!back.flags.protected, "remote flags are not kept locally");

        let unusable: RawDescriptor = serde_json::from_value(serde_json::json!({
            "manifest": { "id": "x", "name": "X" }, "transportUrl": "ftp://x/manifest.json"
        }))
        .unwrap();
        assert!(from_raw(&unusable).is_none());
    }
}
