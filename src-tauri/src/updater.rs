//! Automatic updates: install on the next launch, never mid-session.
//!
//! - At startup (release builds, when enabled) Zeiler checks for an update. If one is
//!   available it is installed right away, before the user has started anything: a
//!   package already downloaded during an earlier session is reused, otherwise it is
//!   downloaded first.
//! - While running, Zeiler re-checks every few hours and downloads quietly into the
//!   cache; the UI shows that the update is ready and will install on next launch.
//!
//! Packages are signature-verified by the plugin inside `install()`, so a cached
//! package is checked against the signature of the freshly fetched update manifest.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tracing::{info, warn};

use crate::error::IpcError;
use crate::state::AppState;

const RECHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    /// Version downloaded and waiting to install on next launch.
    pub ready: Option<String>,
}

fn cache_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_cache_dir().ok().map(|d| d.join("updates"))
}

fn cached_package(app: &AppHandle, version: &str) -> Option<PathBuf> {
    cache_dir(app).map(|d| d.join(format!("{version}.pkg")))
}

/// Download `update` into the cache unless it is already there.
async fn ensure_downloaded(app: &AppHandle, update: &Update) -> anyhow::Result<Vec<u8>> {
    let path = cached_package(app, &update.version)
        .ok_or_else(|| anyhow::anyhow!("no cache directory"))?;
    if let Ok(bytes) = tokio::fs::read(&path).await {
        return Ok(bytes);
    }
    let bytes = update.download(|_, _| {}, || {}).await?;
    if let Some(dir) = path.parent() {
        // start clean: only the newest package is kept
        let _ = tokio::fs::remove_dir_all(dir).await;
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(&path, &bytes).await?;
    Ok(bytes)
}

/// Install a verified package and come back up on the new version. On Windows the
/// NSIS installer takes over: the process exits here and the installer relaunches it.
fn install_and_restart(app: &AppHandle, update: &Update, bytes: &[u8]) -> anyhow::Result<()> {
    let _ = app.emit("update://installing", &update.version);
    update.install(bytes)?;
    app.restart();
}

async fn check(app: &AppHandle) -> anyhow::Result<Option<Update>> {
    Ok(app.updater()?.check().await?)
}

/// Spawned once from setup.
pub fn start(app: AppHandle) {
    if cfg!(debug_assertions) {
        return; // dev builds must never replace themselves with a release
    }
    tauri::async_runtime::spawn(async move {
        let auto = match app.try_state::<AppState>() {
            Some(state) => state.settings.read().await.auto_update,
            None => true,
        };
        if !auto {
            return;
        }

        // 1. startup: install whatever is available, before the user starts anything
        match check(&app).await {
            Ok(Some(update)) => {
                info!(version = %update.version, "update available at launch, installing");
                match ensure_downloaded(&app, &update).await {
                    Ok(bytes) => {
                        if let Err(e) = install_and_restart(&app, &update, &bytes) {
                            warn!("update install failed: {e:#}");
                            let _ = app.emit("update://failed", format!("{e:#}"));
                        }
                    }
                    Err(e) => warn!("update download failed: {e:#}"),
                }
            }
            Ok(None) => {
                if let Some(dir) = cache_dir(&app) {
                    let _ = tokio::fs::remove_dir_all(dir).await;
                }
            }
            Err(e) => warn!("update check failed: {e:#}"),
        }

        // 2. while running: download quietly, install on next launch
        loop {
            tokio::time::sleep(RECHECK_EVERY).await;
            let still_on = match app.try_state::<AppState>() {
                Some(state) => state.settings.read().await.auto_update,
                None => true,
            };
            if !still_on {
                continue;
            }
            match check(&app).await {
                Ok(Some(update)) => match ensure_downloaded(&app, &update).await {
                    Ok(_) => {
                        info!(version = %update.version, "update downloaded; installs on next launch");
                        let _ = app.emit("update://ready", &update.version);
                    }
                    Err(e) => warn!("update download failed: {e:#}"),
                },
                Ok(None) => {}
                Err(e) => warn!("update check failed: {e:#}"),
            }
        }
    });
}

fn updates_disabled_in_dev() -> Result<(), IpcError> {
    if cfg!(debug_assertions) {
        Err(IpcError::InvalidInput(
            "Updates are disabled in development builds.".into(),
        ))
    } else {
        Ok(())
    }
}

/// Current version and any downloaded-but-not-installed update.
#[tauri::command]
pub async fn update_status(app: AppHandle) -> Result<UpdateStatus, IpcError> {
    let current = app.package_info().version.to_string();
    let mut ready = None;
    if let Some(dir) = cache_dir(&app)
        && let Ok(mut entries) = tokio::fs::read_dir(dir).await
    {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(version) = entry.file_name().to_string_lossy().strip_suffix(".pkg")
                && version != current
            {
                ready = Some(version.to_string());
            }
        }
    }
    Ok(UpdateStatus { current, ready })
}

/// Check now; downloads the update if there is one. Returns its version.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, IpcError> {
    updates_disabled_in_dev()?;
    let update = check(&app)
        .await
        .map_err(|e| IpcError::Storage(format!("couldn't check for updates: {e:#}")))?;
    let Some(update) = update else {
        return Ok(None);
    };
    ensure_downloaded(&app, &update)
        .await
        .map_err(|e| IpcError::Storage(format!("couldn't download the update: {e:#}")))?;
    Ok(Some(update.version.clone()))
}

/// Install the available update now and restart.
#[tauri::command]
pub async fn update_install_now(app: AppHandle) -> Result<(), IpcError> {
    updates_disabled_in_dev()?;
    let update = check(&app)
        .await
        .map_err(|e| IpcError::Storage(format!("couldn't check for updates: {e:#}")))?
        .ok_or_else(|| IpcError::NotFound("Zeiler is already up to date.".into()))?;
    let bytes = ensure_downloaded(&app, &update)
        .await
        .map_err(|e| IpcError::Storage(format!("couldn't download the update: {e:#}")))?;
    install_and_restart(&app, &update, &bytes)
        .map_err(|e| IpcError::Storage(format!("couldn't install the update: {e:#}")))
}
