//! Stremio account commands (see `account/mod.rs`). The session key never crosses
//! IPC; the password crosses it once, in the password sign-in, and is dropped after
//! the call. The link-code sign-in is the recommended one.

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use zeiler_sync::sync::{AddonOp, FirstSync, importable, same_url};
use zeiler_sync::{LinkCode, SyncError};

use crate::account::{self, AccountState, AccountStatus, SyncReport, ipc_error};
use crate::error::IpcError;
use crate::library::LibraryState;
use crate::state::AppState;

pub const ACCOUNT_CHANGED: &str = "account://changed";
/// Emitted when a sync replaced the active profile's addons (the UI reloads them).
pub const ADDONS_CHANGED: &str = "addons://changed";
/// How often the background sync runs while signed in.
const SYNC_EVERY: Duration = Duration::from_secs(15 * 60);
/// A local library change is pushed this long after it (more changes join it).
const AFTER_CHANGE: Duration = Duration::from_secs(60);

async fn active_profile(state: &AppState) -> String {
    state
        .profiles
        .read()
        .await
        .active_id
        .clone()
        .unwrap_or_default()
}

async fn announce(app: &AppHandle) -> Option<AccountStatus> {
    let state = app.try_state::<AppState>()?;
    let account = app.try_state::<AccountState>()?;
    let status = account.status(&active_profile(&state).await).await;
    let _ = app.emit(ACCOUNT_CHANGED, &status);
    Some(status)
}

/// Sync the active profile now: library, then addons, as its settings say. Does
/// nothing while signed out.
pub async fn sync_all(app: &AppHandle) -> Result<SyncReport, IpcError> {
    let (Some(state), Some(account), Some(library)) = (
        app.try_state::<AppState>(),
        app.try_state::<AccountState>(),
        app.try_state::<LibraryState>(),
    ) else {
        return Ok(SyncReport::default());
    };
    let report = {
        let _one = account.sync_lock.lock().await;
        let profile = active_profile(&state).await;
        let Some((key, _)) = account.key(&profile) else {
            return Ok(SyncReport::default());
        };
        let file = account.file.get().await;
        let Some(user) = file.user.clone() else {
            return Ok(SyncReport::default());
        };
        let result: Result<SyncReport, SyncError> = async {
            let mut report = SyncReport::default();
            if file.sync_library {
                let (pulled, pushed) =
                    account::sync_library(&account, &library, &key, &user.id).await?;
                report.pulled = pulled;
                report.pushed = pushed;
                let _ = account
                    .file
                    .update(|f| f.last_library_sync = Some(account::now_ms()))
                    .await;
            }
            if file.sync_addons {
                report.addons = Some(account::pull_addons(&account, &state, &key).await?);
                let _ = app.emit(ADDONS_CHANGED, ());
                let _ = account
                    .file
                    .update(|f| f.last_addons_sync = Some(account::now_ms()))
                    .await;
            }
            Ok(report)
        }
        .await;
        match result {
            Ok(report) => {
                account.set_error(None);
                Ok(report)
            }
            Err(SyncError::SessionExpired) => {
                account.session_ended(&profile).await;
                Err(ipc_error(SyncError::SessionExpired))
            }
            Err(e) => {
                account.set_error(Some(e.to_string()));
                Err(ipc_error(e))
            }
        }
    };
    announce(app).await;
    report
}

/// The background sync: shortly after launch, every 15 minutes, and a minute after a
/// local library change.
pub fn start_background_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            let _ = sync_all(&app).await;
            let Some(account) = app.try_state::<AccountState>() else {
                return;
            };
            tokio::select! {
                _ = account.kick.notified() => tokio::time::sleep(AFTER_CHANGE).await,
                _ = tokio::time::sleep(SYNC_EVERY) => {}
            }
        }
    });
}

/// Local addon changes for the account: queued now (the caller holds `profile_op`,
/// so in the order they happened), sent in the background when addon sync is on. A
/// failure is reported in the account status, never to the change itself; the change
/// stays queued and goes with the next sync.
pub async fn addons_changed(app: &AppHandle, ops: Vec<AddonOp>) {
    let Some(account) = app.try_state::<AccountState>() else {
        return;
    };
    if !account.file.get().await.sync_addons || ops.is_empty() {
        return;
    }
    account.queue(ops);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (Some(state), Some(account)) =
            (app.try_state::<AppState>(), app.try_state::<AccountState>())
        else {
            return;
        };
        let pushed = {
            let _one = account.sync_lock.lock().await;
            let profile = active_profile(&state).await;
            let Some((key, _)) = account.key(&profile) else {
                return;
            };
            account::push_pending(&account, &key).await
        };
        match pushed {
            Ok(()) => account.set_error(None),
            Err(e) => account.set_error(Some(format!(
                "Your addon change wasn't sent to Stremio yet ({e}); it goes with the next sync."
            ))),
        }
        announce(&app).await;
    });
}

#[tauri::command]
pub async fn account_status(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
) -> Result<AccountStatus, IpcError> {
    Ok(account.status(&active_profile(&state).await).await)
}

/// A code for the browser sign-in; the UI opens `link` and polls.
#[tauri::command]
pub async fn account_link_start(account: State<'_, AccountState>) -> Result<LinkCode, IpcError> {
    let mut code = account.api.link_create().await.map_err(ipc_error)?;
    if !code.link.starts_with("https://") {
        return Err(IpcError::Network(
            "Stremio sent a sign-in link Zeiler won't open.".into(),
        ));
    }
    // only an image Zeiler may show (a data: PNG, or https)
    if code
        .qrcode
        .as_deref()
        .is_some_and(|q| !(q.starts_with("data:image/") || q.starts_with("https://")))
    {
        code.qrcode = None;
    }
    Ok(code)
}

/// Whether the browser sign-in for `code` was confirmed: `None` while waiting; then
/// the session is kept (in the keychain when `remember`) and the first sync starts.
#[tauri::command]
pub async fn account_link_poll(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    code: String,
    remember: bool,
) -> Result<Option<AccountStatus>, IpcError> {
    if code.is_empty() || code.len() > 64 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(IpcError::InvalidInput("That is not a sign-in code.".into()));
    }
    let token = match account.api.link_read(&code).await {
        Ok(token) => token,
        Err(SyncError::LinkPending) => return Ok(None),
        Err(e) => return Err(ipc_error(e)),
    };
    let (key, user) = account
        .api
        .login_with_token(&token)
        .await
        .map_err(ipc_error)?;
    let profile = active_profile(&state).await;
    account::signed_in(&account, &profile, key, user, remember).await?;
    first_sync(&app);
    Ok(announce(&app).await)
}

/// Sign in with email and password (sent only to api.strem.io).
#[tauri::command]
pub async fn account_login_password(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    email: String,
    mut password: String,
    remember: bool,
) -> Result<AccountStatus, IpcError> {
    use zeroize::Zeroize;
    let email = email.trim().to_string();
    if email.is_empty() || password.is_empty() {
        password.zeroize();
        return Err(IpcError::InvalidInput(
            "Enter your email and password.".into(),
        ));
    }
    let result = account.api.login(&email, &password).await;
    // not kept: wiped from this copy once Stremio has answered
    password.zeroize();
    let (key, user) = result.map_err(ipc_error)?;
    let profile = active_profile(&state).await;
    account::signed_in(&account, &profile, key, user, remember).await?;
    first_sync(&app);
    announce(&app)
        .await
        .ok_or_else(|| IpcError::Storage("The account state is not available.".into()))
}

fn first_sync(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = sync_all(&app).await;
    });
}

/// Sign out of this profile: Stremio is told (best effort) and the key is forgotten.
/// The library and addons stay as they are (the library still marked as that
/// account's: signing in to another account starts that one's own library, keeping
/// this one beside it).
#[tauri::command]
pub async fn account_logout(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
) -> Result<AccountStatus, IpcError> {
    let _one = account.sync_lock.lock().await;
    let profile = active_profile(&state).await;
    if let Some((key, _)) = account.key(&profile) {
        let _ = tokio::time::timeout(Duration::from_secs(5), account.api.logout(&key)).await;
    }
    account.sessions.forget(&profile);
    account
        .file
        .update(|f| {
            f.user = None;
            f.sync_addons = false;
            f.addons_choice_made = false;
        })
        .await?;
    account.addons_locked.store(false, Ordering::SeqCst);
    account.set_error(None);
    drop(_one);
    announce(&app)
        .await
        .ok_or_else(|| IpcError::Storage("The account state is not available.".into()))
}

/// Switch library sync on or off, or addon sync off (on goes through
/// `account_addons_apply`, which asks how to treat the two lists first).
#[tauri::command]
pub async fn account_set_sync(
    app: AppHandle,
    account: State<'_, AccountState>,
    library: Option<bool>,
    addons: Option<bool>,
) -> Result<AccountStatus, IpcError> {
    if addons == Some(true) {
        return Err(IpcError::InvalidInput(
            "Choose how to combine your addons first (account_addons_apply).".into(),
        ));
    }
    account
        .file
        .update(|f| {
            if let Some(on) = library {
                f.sync_library = on;
            }
            if addons == Some(false) {
                f.sync_addons = false;
                f.addons_choice_made = true;
            }
        })
        .await?;
    if library == Some(true) {
        account.kick.notify_one();
    }
    announce(&app)
        .await
        .ok_or_else(|| IpcError::Storage("The account state is not available.".into()))
}

/// The two addon lists, by name, for the sign-in's addon question.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonsPreview {
    pub account_only: Vec<String>,
    pub local_only: Vec<String>,
    pub both: Vec<String>,
}

#[tauri::command]
pub async fn account_addons_preview(
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
) -> Result<AddonsPreview, IpcError> {
    let profile = active_profile(&state).await;
    let (key, _) = account
        .key(&profile)
        .ok_or_else(|| IpcError::SessionExpired("Sign in to Stremio first.".into()))?;
    let remote = account
        .api
        .addon_collection_get(&key)
        .await
        .map_err(ipc_error)?;
    let remote: Vec<_> = importable(&remote.addons, &account.builtins)
        .into_iter()
        .filter(account::usable)
        .collect();
    let local = state.addons.read().await.clone();
    let name = |raw: &zeiler_sync::RawDescriptor| {
        raw.manifest
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or(&raw.transport_url)
            .to_string()
    };
    Ok(AddonsPreview {
        account_only: remote
            .iter()
            .filter(|r| {
                !local
                    .iter()
                    .any(|l| same_url(l.transport_url.as_str(), &r.transport_url))
            })
            .map(name)
            .collect(),
        local_only: local
            .iter()
            .filter(|l| {
                !remote
                    .iter()
                    .any(|r| same_url(l.transport_url.as_str(), &r.transport_url))
            })
            .map(|l| l.manifest.name.clone())
            .collect(),
        both: remote
            .iter()
            .filter(|r| {
                local
                    .iter()
                    .any(|l| same_url(l.transport_url.as_str(), &r.transport_url))
            })
            .map(name)
            .collect(),
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AddonsChoice {
    /// The account's addons replace this profile's.
    UseAccount,
    /// The account's, plus this profile's that it lacks (uploaded).
    Merge,
    /// Don't sync addons.
    KeepLocal,
}

/// Answer the sign-in's addon question.
#[tauri::command]
pub async fn account_addons_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    choice: AddonsChoice,
) -> Result<AccountStatus, IpcError> {
    let profile = active_profile(&state).await;
    let first = match choice {
        AddonsChoice::UseAccount => Some(FirstSync::UseAccount),
        AddonsChoice::Merge => Some(FirstSync::Merge),
        AddonsChoice::KeepLocal => None,
    };
    if let Some(first) = first {
        let _one = account.sync_lock.lock().await;
        let (key, _) = account
            .key(&profile)
            .ok_or_else(|| IpcError::SessionExpired("Sign in to Stremio first.".into()))?;
        account::first_addon_sync(&account, &state, &key, first)
            .await
            .map_err(ipc_error)?;
        let _ = app.emit(ADDONS_CHANGED, ());
    }
    account
        .file
        .update(|f| {
            f.addons_choice_made = true;
            f.sync_addons = first.is_some();
            if first.is_some() {
                f.last_addons_sync = Some(account::now_ms());
            }
        })
        .await?;
    announce(&app)
        .await
        .ok_or_else(|| IpcError::Storage("The account state is not available.".into()))
}

#[tauri::command]
pub async fn account_sync_now(app: AppHandle) -> Result<SyncReport, IpcError> {
    sync_all(&app).await
}
