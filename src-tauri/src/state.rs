use std::path::PathBuf;
use std::sync::Arc;

use sailor_addon::{AddonClient, Descriptor};
use sailor_engine::{Engine, EngineConfig};
use sailor_player::Player;
use serde::{Deserialize, Serialize};
use tokio::sync::{OnceCell, RwLock};
use tracing::warn;

use crate::error::IpcError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Explicit mpv binary path; None = search PATH.
    pub mpv_path: Option<String>,
    /// Incoming BitTorrent TCP port (0 = random). Applies when the engine starts.
    pub engine_listen_port: u16,
    pub engine_upnp: bool,
    /// KiB/s; None = unlimited.
    pub upload_limit_kibps: Option<u32>,
    pub download_limit_kibps: Option<u32>,
    /// Remove torrent data when playback of it is stopped from the UI.
    pub delete_on_stop: bool,
    /// UI theme preset id (see src/lib/theme.ts).
    pub theme: String,
    /// Play the first direct stream automatically instead of always showing the picker.
    pub instant_play: bool,
    /// Hide the Anime room from navigation.
    pub hide_anime: bool,
    /// Hide the (placeholder) Live TV entry from navigation.
    pub hide_live_tv: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mpv_path: None,
            engine_listen_port: 4240,
            engine_upnp: true,
            upload_limit_kibps: None,
            download_limit_kibps: None,
            delete_on_stop: true,
            theme: "harbor-grey".to_string(),
            instant_play: false,
            hide_anime: false,
            hide_live_tv: false,
        }
    }
}

pub struct AppState {
    pub client: AddonClient,
    pub addons: RwLock<Vec<Descriptor>>,
    pub settings: RwLock<Settings>,
    pub player: Player,
    engine: OnceCell<Arc<Engine>>,
    /// Serializes all persist operations: concurrent writers would otherwise race on
    /// the same temp file and rename stale state over fresh state.
    persist_lock: tokio::sync::Mutex<()>,
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl AppState {
    pub async fn load(data_dir: PathBuf, cache_dir: PathBuf) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&data_dir).await?;
        tokio::fs::create_dir_all(&cache_dir).await?;

        let settings: Settings = read_json(&data_dir.join("settings.json")).await?;
        let addons: Vec<Descriptor> = read_json(&data_dir.join("addons.json")).await?;
        let player = Player::new(settings.mpv_path.as_deref().map(std::path::Path::new));

        Ok(Self {
            client: AddonClient::new()?,
            addons: RwLock::new(addons),
            settings: RwLock::new(settings),
            player,
            engine: OnceCell::new(),
            persist_lock: tokio::sync::Mutex::new(()),
            data_dir,
            cache_dir,
        })
    }

    /// The torrent engine, started on first use (avoids a firewall prompt at app launch).
    pub async fn engine(&self) -> Result<Arc<Engine>, IpcError> {
        self.engine
            .get_or_try_init(|| async {
                let settings = self.settings.read().await.clone();
                let config = EngineConfig {
                    cache_dir: self.cache_dir.join("torrents"),
                    state_dir: self.data_dir.join("engine"),
                    listen_port: settings.engine_listen_port,
                    enable_upnp: settings.engine_upnp,
                    upload_limit_bps: settings.upload_limit_kibps.map(|k| k.saturating_mul(1024)),
                    download_limit_bps: settings
                        .download_limit_kibps
                        .map(|k| k.saturating_mul(1024)),
                };
                Engine::new(config)
                    .await
                    .map(Arc::new)
                    .map_err(|e| IpcError::Engine(format!("{e:#}")))
            })
            .await
            .cloned()
    }

    pub fn engine_if_running(&self) -> Option<Arc<Engine>> {
        self.engine.get().cloned()
    }

    pub async fn persist_addons(&self) -> Result<(), IpcError> {
        let _guard = self.persist_lock.lock().await;
        let snapshot = self.addons.read().await.clone();
        write_json(&self.data_dir.join("addons.json"), &snapshot).await
    }

    pub async fn persist_settings(&self) -> Result<(), IpcError> {
        let _guard = self.persist_lock.lock().await;
        let snapshot = self.settings.read().await.clone();
        write_json(&self.data_dir.join("settings.json"), &snapshot).await
    }
}

/// Missing file → defaults. Corrupt file → moved aside to `.corrupt.bak` (so the next
/// persist cannot overwrite the user's real data), then defaults. Any other IO error
/// (permissions, AV sharing violation) fails startup rather than silently losing state.
async fn read_json<T: serde::de::DeserializeOwned + Default>(
    path: &std::path::Path,
) -> anyhow::Result<T> {
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

async fn write_json<T: Serialize>(path: &std::path::Path, value: &T) -> Result<(), IpcError> {
    let json = serde_json::to_vec_pretty(value)
        .map_err(|e| IpcError::Storage(format!("failed to serialize: {e}")))?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, &json)
        .await
        .map_err(|e| IpcError::Storage(format!("failed to write {}: {e}", tmp.display())))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| IpcError::Storage(format!("failed to replace {}: {e}", path.display())))?;
    Ok(())
}
