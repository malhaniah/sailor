//! Playback for Zeiler.
//!
//! v0 drives an external `mpv` process (system-installed, or a user-configured binary).
//! The embedded libmpv surface — video rendered under a transparent webview via the
//! `wid`/render API per OS — is the planned `embedded` backend and will reuse this
//! crate's request types.

#[cfg(feature = "embedded")]
pub mod embedded;
pub mod mpv_error;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tracing::info;

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error(
        "mpv was not found. Install mpv (https://mpv.io) or set its path in Settings, then try again"
    )]
    NoPlayer,
    #[error("failed to launch mpv: {0}")]
    Spawn(#[from] std::io::Error),
}

/// A playback request. Headers come from a stream's `behaviorHints.proxyHeaders.request`
/// and apply to every HTTP request mpv makes for this file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaybackRequest {
    pub url: String,
    pub title: Option<String>,
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub start_at_sec: Option<f64>,
}

/// Build the mpv CLI arguments for a request. The URL itself is NOT part of the argv —
/// it is fed through stdin (`--playlist=-`) so loopback stream URLs carrying per-run
/// credentials never show up in the process list. Each header goes through
/// `--http-header-fields-append` so commas in values cannot corrupt the list option.
/// (Headers remain on argv — a known limitation until the embedded player lands.)
pub fn build_mpv_args(request: &PlaybackRequest) -> Vec<String> {
    let mut args = vec![
        "--force-window=immediate".to_string(),
        "--keep-open=yes".to_string(),
        "--playlist=-".to_string(),
    ];
    if let Some(title) = &request.title {
        args.push(format!("--title={title}"));
        args.push(format!("--force-media-title={title}"));
    }
    if let Some(ua) = &request.user_agent {
        args.push(format!("--user-agent={ua}"));
    }
    if let Some(referrer) = &request.referrer {
        args.push(format!("--referrer={referrer}"));
    }
    for (name, value) in &request.headers {
        args.push(format!("--http-header-fields-append={name}: {value}"));
    }
    if let Some(start) = request.start_at_sec {
        args.push(format!("--start={start}"));
    }
    args
}

/// Strip userinfo from a URL for logging (`http://user:pass@host/…` → `http://***@host/…`).
pub fn redact_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        let authority_end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..authority_end].rfind('@') {
            return format!("{}***{}", &url[..scheme_end + 3], &rest[at..]);
        }
    }
    url.to_string()
}

/// Locate an mpv binary: an explicit configured path first, then `mpv(.exe)` on PATH.
pub fn locate_mpv(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = configured {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        return None;
    }
    let exe = if cfg!(windows) { "mpv.exe" } else { "mpv" };
    let on_path = std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(exe))
            .find(|candidate| candidate.is_file())
    });
    // Apps launched from Finder / the Start Menu / a desktop launcher don't inherit the
    // shell's PATH, so a Homebrew/winget/scoop mpv is invisible to them without this.
    on_path.or_else(|| known_locations().into_iter().find(|p| p.is_file()))
}

/// Standard install locations of mpv per OS, most likely first.
fn known_locations() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        let env_dir = |var: &str| std::env::var_os(var).map(PathBuf::from);
        if let Some(local) = env_dir("LOCALAPPDATA") {
            paths.push(local.join(r"Microsoft\WinGet\Links\mpv.exe"));
            // winget portable packages live in a source-suffixed folder
            if let Ok(entries) = std::fs::read_dir(local.join(r"Microsoft\WinGet\Packages")) {
                paths.extend(
                    entries
                        .flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with("shinchiro.mpv"))
                        .map(|e| e.path().join("mpv.exe")),
                );
            }
        }
        if let Some(home) = env_dir("USERPROFILE") {
            paths.push(home.join(r"scoop\shims\mpv.exe"));
            paths.push(home.join(r"scoop\apps\mpv\current\mpv.exe"));
        }
        if let Some(program_data) = env_dir("ProgramData") {
            paths.push(program_data.join(r"chocolatey\bin\mpv.exe"));
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = env_dir(var) {
                paths.push(dir.join(r"mpv\mpv.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        for p in [
            "/opt/homebrew/bin/mpv",
            "/usr/local/bin/mpv",
            "/opt/local/bin/mpv",
            "/Applications/mpv.app/Contents/MacOS/mpv",
        ] {
            paths.push(PathBuf::from(p));
        }
    } else {
        for p in ["/usr/bin/mpv", "/usr/local/bin/mpv", "/snap/bin/mpv"] {
            paths.push(PathBuf::from(p));
        }
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".local/bin/mpv"));
        }
    }
    paths
}

/// External-mpv player. Keeps at most one child process; starting a new playback stops
/// the previous one.
pub struct Player {
    binary: Mutex<Option<PathBuf>>,
    child: Mutex<Option<Child>>,
}

impl Player {
    pub fn new(configured: Option<&Path>) -> Self {
        Self {
            binary: Mutex::new(locate_mpv(configured)),
            child: Mutex::new(None),
        }
    }

    /// Re-run binary discovery (after the user changes the configured path).
    pub fn relocate(&self, configured: Option<&Path>) {
        *self.binary.lock().unwrap() = locate_mpv(configured);
    }

    pub fn binary(&self) -> Option<PathBuf> {
        self.binary.lock().unwrap().clone()
    }

    pub async fn play(&self, request: &PlaybackRequest) -> Result<(), PlayerError> {
        let binary = self.binary().ok_or(PlayerError::NoPlayer)?;
        let args = build_mpv_args(request);
        info!(player = %binary.display(), url = %redact_url(&request.url), "launching external player");
        let mut child = Command::new(&binary)
            .args(&args)
            .stdin(Stdio::piped())
            .kill_on_drop(false)
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(request.url.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.shutdown().await?;
        }
        // Swap atomically under the lock so concurrent plays can never orphan a child,
        // then stop whatever was playing before.
        let previous = self.child.lock().unwrap().replace(child);
        if let Some(mut previous) = previous {
            let _ = previous.start_kill();
        }
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_args_with_headers_and_title_but_no_url() {
        let args = build_mpv_args(&PlaybackRequest {
            url: "https://user:secret@127.0.0.1:9999/v.mp4".into(),
            title: Some("Big Buck Bunny".into()),
            headers: vec![("Referer".into(), "https://site.example".into())],
            user_agent: Some("Zeiler".into()),
            referrer: None,
            start_at_sec: Some(42.5),
        });
        assert!(args.contains(&"--playlist=-".to_string()));
        assert!(args.contains(&"--force-media-title=Big Buck Bunny".to_string()));
        assert!(args.contains(&"--user-agent=Zeiler".to_string()));
        assert!(
            args.contains(&"--http-header-fields-append=Referer: https://site.example".to_string())
        );
        assert!(args.contains(&"--start=42.5".to_string()));
        // the URL (and its credentials) must never be an argument
        assert!(args.iter().all(|a| !a.contains("secret")));
    }

    #[test]
    fn redacts_userinfo() {
        assert_eq!(
            redact_url("http://u:p@127.0.0.1:1/x/y?z=1"),
            "http://***@127.0.0.1:1/x/y?z=1"
        );
        assert_eq!(redact_url("https://host/a@b"), "https://host/a@b");
        assert_eq!(redact_url("not a url"), "not a url");
    }

    #[test]
    fn locate_with_bogus_configured_path_returns_none() {
        assert!(locate_mpv(Some(Path::new("Z:/definitely/not/mpv.exe"))).is_none());
    }

    #[test]
    fn known_locations_cover_the_platform_package_managers() {
        let paths: Vec<String> = known_locations()
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        if cfg!(windows) {
            assert!(
                paths
                    .iter()
                    .any(|p| p.ends_with("Microsoft/WinGet/Links/mpv.exe"))
            );
            assert!(paths.iter().any(|p| p.ends_with("scoop/shims/mpv.exe")));
        } else if cfg!(target_os = "macos") {
            assert!(paths.iter().any(|p| p == "/opt/homebrew/bin/mpv"));
        } else {
            assert!(paths.iter().any(|p| p == "/usr/bin/mpv"));
        }
    }
}
