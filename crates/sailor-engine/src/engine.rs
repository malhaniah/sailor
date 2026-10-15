use std::collections::HashSet;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::file_info::FileInfo;
use librqbit::http_api::{HttpApi, HttpApiOptions};
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListenerMode,
    ListenerOptions, ManagedTorrent, Session, SessionOptions,
};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use tracing::{debug_span, info};

use crate::magnet::{build_magnet, normalize_info_hash};

/// Encode set for a URL path segment (the advisory filename on the stream URL).
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b'/')
    .add(b'\\')
    .add(b'{')
    .add(b'}');

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Downloaded torrent data lives here, one subfolder per infohash.
    pub cache_dir: PathBuf,
    /// Session + DHT persistence (never rqbit's shared OS default).
    pub state_dir: PathBuf,
    /// Incoming BitTorrent TCP port (0 = random).
    pub listen_port: u16,
    pub enable_upnp: bool,
    /// Bytes per second; None = unlimited.
    pub upload_limit_bps: Option<u32>,
    pub download_limit_bps: Option<u32>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from("sailor-cache"),
            state_dir: PathBuf::from("sailor-state"),
            listen_port: 4240,
            enable_upnp: true,
            upload_limit_bps: None,
            download_limit_bps: None,
        }
    }
}

/// A Stremio torrent stream to resolve: `infoHash` + optional `fileIdx` + tracker
/// `sources`, plus the advisory filename from `behaviorHints`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentStreamRequest {
    pub info_hash: String,
    #[serde(default)]
    pub file_idx: Option<u32>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub filename: Option<String>,
    /// Regex patterns (case-insensitive) the chosen file must match when `fileIdx` is
    /// absent; invalid patterns degrade to substring matching.
    #[serde(default)]
    pub file_must_include: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedStream {
    /// Loopback HTTP URL (with embedded credentials) to hand to the player; supports Range.
    pub stream_url: String,
    pub info_hash: String,
    pub file_idx: usize,
    pub file_name: String,
    pub file_size: u64,
}

/// The in-process torrent engine: one librqbit session plus its loopback HTTP endpoint.
pub struct Engine {
    session: Arc<Session>,
    cache_dir: PathBuf,
    http_addr: SocketAddr,
    auth_user: String,
    auth_pass: String,
}

impl Engine {
    pub async fn new(config: EngineConfig) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&config.cache_dir).await?;
        tokio::fs::create_dir_all(&config.state_dir).await?;

        let options = SessionOptions {
            dht: Some(DhtSessionConfig {
                persistence: Some(DhtPersistenceConfig {
                    config_filename: Some(config.state_dir.join("dht.json")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            // No session persistence: a streaming client must not resurrect and re-seed
            // every previously played torrent on startup. Cached file data stays on disk
            // (unless delete-on-stop removed it) and is re-checked if the same stream is
            // played again. DHT routing-table persistence (below) is kept.
            persistence: None,
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                listen_addr: (Ipv6Addr::UNSPECIFIED, config.listen_port).into(),
                enable_upnp_port_forwarding: config.enable_upnp,
                ..Default::default()
            }),
            ratelimits: LimitsConfig {
                upload_bps: config.upload_limit_bps.and_then(NonZeroU32::new),
                download_bps: config.download_limit_bps.and_then(NonZeroU32::new),
            },
            ..Default::default()
        };

        let session = Session::new_with_opts(config.cache_dir.clone(), options)
            .await
            .context("failed to start torrent session")?;

        // Loopback-only HTTP endpoint for the player. Read-only (writes go through the
        // Session in-process) and guarded by per-run basic-auth credentials so other
        // local processes/pages cannot enumerate it.
        let auth_user = uuid::Uuid::new_v4().simple().to_string();
        let auth_pass = uuid::Uuid::new_v4().simple().to_string();
        let api = Api::new(session.clone(), None, None);
        let http_api = HttpApi::new(
            api,
            Some(HttpApiOptions {
                read_only: true,
                basic_auth: Some((auth_user.clone(), auth_pass.clone())),
                ..Default::default()
            }),
        );
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Default::default(),
        )
        .context("failed to bind engine stream endpoint")?;
        let http_addr = listener.bind_addr();
        session.spawn(
            debug_span!("engine_http"),
            "engine-http-api",
            http_api.make_http_api_and_run(listener, None),
        );
        info!(%http_addr, "torrent engine ready");

        Ok(Self {
            session,
            cache_dir: config.cache_dir,
            http_addr,
            auth_user,
            auth_pass,
        })
    }

    /// Resolve a Stremio torrent stream into a playable loopback URL. Blocks until the
    /// torrent's metadata is known and its files are checked.
    pub async fn resolve(&self, request: TorrentStreamRequest) -> anyhow::Result<ResolvedStream> {
        let hash = normalize_info_hash(&request.info_hash)?;
        let magnet = build_magnet(&hash, &request.sources)?;

        // When fileIdx is absent the add starts paused with nothing selected; the right
        // file is picked from the metadata below, then the torrent is narrowed and
        // started. This avoids briefly selecting (and downloading) the whole torrent.
        let add_options = AddTorrentOptions {
            overwrite: true,
            paused: request.file_idx.is_none(),
            only_files: request.file_idx.map(|i| vec![i as usize]),
            output_folder: Some(self.cache_dir.join(&hash).to_string_lossy().into_owned()),
            ..Default::default()
        };
        let response = tokio::time::timeout(
            Duration::from_secs(90),
            self.session
                .add_torrent(AddTorrent::from_url(&magnet), Some(add_options)),
        )
        .await
        .context("timed out finding peers for this torrent")?
        .context("failed to add torrent")?;

        let (handle, freshly_added) = match response {
            AddTorrentResponse::Added(_, handle) => (handle, true),
            AddTorrentResponse::AlreadyManaged(_, handle) => (handle, false),
            AddTorrentResponse::ListOnly(_) => bail!("unexpected list-only response"),
        };

        let prepared = self
            .prepare_stream(&handle, request.file_idx.map(|i| i as usize), &request)
            .await;
        let (file_idx, file_name, file_size) = match prepared {
            Ok(selected) => selected,
            Err(err) => {
                // Don't leave an orphan silently downloading/seeding after an aborted
                // resolve — but never delete a torrent that predates this call.
                if freshly_added {
                    let _ = self
                        .session
                        .delete(TorrentIdOrHash::Hash(handle.info_hash()), true)
                        .await;
                }
                return Err(err);
            }
        };

        let encoded_name = utf8_percent_encode(&file_name, PATH_SEGMENT).to_string();
        let stream_url = format!(
            "http://{user}:{pass}@{addr}/torrents/{hash}/stream/{file_idx}/{encoded_name}",
            user = self.auth_user,
            pass = self.auth_pass,
            addr = self.http_addr,
        );

        Ok(ResolvedStream {
            stream_url,
            info_hash: hash,
            file_idx,
            file_name,
            file_size,
        })
    }

    /// Wait for initialization, pick the file to stream, narrow the selection, and make
    /// sure the torrent is running.
    async fn prepare_stream(
        &self,
        handle: &Arc<ManagedTorrent>,
        requested_idx: Option<usize>,
        request: &TorrentStreamRequest,
    ) -> anyhow::Result<(usize, String, u64)> {
        tokio::time::timeout(Duration::from_secs(600), handle.wait_until_initialized())
            .await
            .context("timed out checking torrent files")?
            .context("torrent failed to initialize")?;

        let wanted_name = request.filename.clone();
        let must_include = request.file_must_include.clone();
        let selected = handle.with_metadata(|meta| {
            select_file(
                &meta.file_infos,
                requested_idx,
                wanted_name.as_deref(),
                &must_include,
            )
        })??;

        if handle.only_files().as_deref() != Some(&[selected.0]) {
            self.session
                .update_only_files(handle, &HashSet::from([selected.0]))
                .await
                .context("failed to select file")?;
        }
        if handle.is_paused() {
            self.session
                .unpause(handle)
                .await
                .context("failed to start torrent")?;
        }
        Ok(selected)
    }

    /// Live stats for a resolved torrent, as the librqbit stats JSON.
    pub fn stats(&self, info_hash: &str) -> anyhow::Result<serde_json::Value> {
        let handle = self.handle(info_hash)?;
        Ok(serde_json::to_value(handle.stats())?)
    }

    /// Remove a torrent; optionally delete its cached data.
    pub async fn remove(&self, info_hash: &str, delete_files: bool) -> anyhow::Result<()> {
        let hash = normalize_info_hash(info_hash)?;
        let id = TorrentIdOrHash::Hash(Id20::from_str(&hash).map_err(anyhow::Error::msg)?);
        self.session.delete(id, delete_files).await
    }

    /// Graceful shutdown; flushes session persistence.
    pub async fn shutdown(&self) {
        self.session.stop().await;
    }

    fn handle(&self, info_hash: &str) -> anyhow::Result<Arc<ManagedTorrent>> {
        let hash = normalize_info_hash(info_hash)?;
        let id = TorrentIdOrHash::Hash(Id20::from_str(&hash).map_err(anyhow::Error::msg)?);
        self.session.get(id).context("torrent not found")
    }
}

/// Pick the file to stream: the requested index when valid, else a filename match
/// (Stremio `behaviorHints.filename`), else a `fileMustInclude` pattern match, else the
/// largest non-padding file.
fn select_file(
    files: &[FileInfo],
    requested_idx: Option<usize>,
    filename: Option<&str>,
    must_include: &[String],
) -> anyhow::Result<(usize, String, u64)> {
    fn display_name(file: &FileInfo) -> String {
        file.relative_filename
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.relative_filename.to_string_lossy().into_owned())
    }

    if let Some(idx) = requested_idx {
        let file = files
            .get(idx)
            .with_context(|| format!("fileIdx {idx} out of range ({} files)", files.len()))?;
        if !file.attrs.padding {
            return Ok((idx, display_name(file), file.len));
        }
        // fall through: a padding file can never be the video
    }

    if let Some(name) = filename
        && let Some((idx, file)) = files.iter().enumerate().find(|(_, f)| {
            !f.attrs.padding
                && f.relative_filename
                    .file_name()
                    .map(|n| n.to_string_lossy() == name)
                    .unwrap_or(false)
        })
    {
        return Ok((idx, display_name(file), file.len));
    }

    if !must_include.is_empty() {
        let matchers: Vec<_> = must_include
            .iter()
            .map(|p| {
                (
                    regex::RegexBuilder::new(p).case_insensitive(true).build(),
                    p,
                )
            })
            .collect();
        let path_matches = |path: &str| {
            matchers.iter().all(|(regex, pattern)| match regex {
                Ok(regex) => regex.is_match(path),
                Err(_) => path.to_lowercase().contains(&pattern.to_lowercase()),
            })
        };
        if let Some((idx, file)) = files
            .iter()
            .enumerate()
            .find(|(_, f)| !f.attrs.padding && path_matches(&f.relative_filename.to_string_lossy()))
        {
            return Ok((idx, display_name(file), file.len));
        }
    }

    files
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.attrs.padding)
        .max_by_key(|(_, f)| f.len)
        .map(|(idx, f)| (idx, display_name(f), f.len))
        .context("torrent has no selectable files")
}
