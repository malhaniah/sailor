//! The opt-in `stremio://` handler (Windows and Linux).
//!
//! When `stremioDeepLink` is switched on, Zeiler registers itself for the scheme at
//! runtime (tauri-plugin-deep-link's `register`; Harbor's `deeplink_set_stremio` does
//! the same, MIT), and unregisters when it is switched off. The plugin itself only
//! routes the schemes in tauri.conf.json, which stays empty: a static scheme would claim
//! `stremio://` from an installed Stremio app at all times, and on macOS that is the
//! only way (runtime registration is unsupported there), so macOS is left out until the
//! owner decides.
//!
//! The OS starts a second Zeiler with the link as its only argument; the single-instance
//! plugin hands that argv to the running one, which passes the link to the UI as a
//! `deeplink://stremio` event (or keeps it until the UI asks, when the link started the
//! app). A link never installs anything: the UI opens the install confirmation with it,
//! and Rust parses it there like a pasted URL. Links are ignored while the setting is
//! off, and the same link twice within 2.5 s counts once. Links may carry an addon's
//! credentials, so they are never logged.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::IpcError;
use crate::state::AppState;

pub const STREMIO_LINK: &str = "deeplink://stremio";
const SCHEME: &str = "stremio";
const REPEAT_WINDOW: Duration = Duration::from_millis(2500);

/// A `stremio://` link passed as a program's only argument (how the OS opens it).
/// `args` includes the program name, as `std::env::args` does.
pub(crate) fn stremio_link_from_args<S: AsRef<str>>(args: &[S]) -> Option<String> {
    let [_, link] = args else {
        return None;
    };
    let link = link.as_ref().trim();
    let scheme_end = link.find("://")?;
    (link[..scheme_end].eq_ignore_ascii_case(SCHEME)
        && link.len() > scheme_end + 3
        && link.len() <= 4096)
        .then(|| link.to_string())
}

#[derive(Default)]
pub struct DeepLinkState {
    /// A link that arrived before the UI was listening (it started the app).
    pending: Mutex<Option<String>>,
    last: Mutex<Option<(String, Instant)>>,
}

impl DeepLinkState {
    /// Whether `link` repeats the previous one within the window (and remember it).
    fn is_repeat(&self, link: &str) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let repeat = last
            .as_ref()
            .is_some_and(|(prev, at)| prev == link && at.elapsed() < REPEAT_WINDOW);
        *last = Some((link.to_string(), Instant::now()));
        repeat
    }
}

/// Handle a program's argv: at startup (`first_launch`, the UI isn't listening yet)
/// or forwarded from a second instance.
pub(crate) async fn handle_args(app: AppHandle, args: Vec<String>, first_launch: bool) {
    let Some(link) = stremio_link_from_args(&args) else {
        return;
    };
    let enabled = match app.try_state::<AppState>() {
        Some(state) => state.settings.read().await.stremio_deep_link,
        None => false,
    };
    let Some(links) = app.try_state::<DeepLinkState>() else {
        return;
    };
    if !enabled || links.is_repeat(&link) {
        return;
    }
    if first_launch {
        *links.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(link);
    } else {
        let _ = app.emit(STREMIO_LINK, link);
    }
}

/// At startup with the handler on: register again, so the OS points at this copy of
/// Zeiler (an update or a moved AppImage changes the path). Best effort.
pub(crate) fn refresh_registration(app: &AppHandle, enabled: bool) {
    if enabled && status(app, enabled).supported {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = register(&app, true) {
                tracing::warn!("{e}");
            }
        });
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLinkStatus {
    /// Runtime registration works on this OS (Windows, Linux).
    pub supported: bool,
    /// The setting.
    pub enabled: bool,
    /// The OS currently sends `stremio://` links to Zeiler.
    pub registered: bool,
}

/// The link that started the app, once.
#[tauri::command]
pub fn deeplink_take_pending(links: State<'_, DeepLinkState>) -> Option<String> {
    links
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

#[tauri::command]
pub async fn deeplink_stremio_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DeepLinkStatus, IpcError> {
    let enabled = state.settings.read().await.stremio_deep_link;
    Ok(status(&app, enabled))
}

#[cfg(any(windows, target_os = "linux"))]
fn status(app: &AppHandle, enabled: bool) -> DeepLinkStatus {
    use tauri_plugin_deep_link::DeepLinkExt;
    DeepLinkStatus {
        supported: true,
        enabled,
        registered: app.deep_link().is_registered(SCHEME).unwrap_or(false),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn status(_app: &AppHandle, enabled: bool) -> DeepLinkStatus {
    DeepLinkStatus {
        supported: false,
        enabled,
        registered: false,
    }
}

/// Register (or unregister) Zeiler for `stremio://` links, and store the choice. The
/// OS change comes first, so a failure leaves the setting as it was.
#[tauri::command]
pub async fn deeplink_set_stremio(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<DeepLinkStatus, IpcError> {
    register(&app, enabled)?;
    let saved = state
        .update_settings(
            |s| {
                let mut next = s.clone();
                next.stremio_deep_link = enabled;
                Ok(next)
            },
            |after| {
                let _ = app.emit(crate::commands::settings::SETTINGS_CHANGED, after);
            },
        )
        .await;
    if let Err(e) = saved {
        // put the OS back the way the stored setting says
        if let Err(undo) = register(&app, !enabled) {
            tracing::warn!("{undo}");
        }
        return Err(e);
    }
    Ok(status(&app, enabled))
}

#[cfg(any(windows, target_os = "linux"))]
fn registration_error(enabled: bool, e: impl std::fmt::Display) -> IpcError {
    IpcError::Storage(format!(
        "Could not {} Zeiler for stremio:// links: {e}",
        if enabled { "register" } else { "unregister" }
    ))
}

/// Linux: the plugin's register adds Zeiler's handler as the default, and its
/// unregister drops only Zeiler's own entry, so another handler comes back by itself.
#[cfg(target_os = "linux")]
fn register(app: &AppHandle, enabled: bool) -> Result<(), IpcError> {
    use tauri_plugin_deep_link::DeepLinkExt;
    let result = if enabled {
        app.deep_link().register(SCHEME)
    } else {
        app.deep_link().unregister(SCHEME)
    };
    result.map_err(|e| registration_error(enabled, e))
}

/// Windows: the scheme lives in `HKCU\Software\Classes\stremio`, which a Stremio app
/// may own. The plugin's register overwrites that key and its unregister deletes it
/// (and tries the machine-wide one), so Zeiler does the switching off itself: the
/// handler that was there before is saved when Zeiler takes over and put back when it
/// lets go; with none saved, only Zeiler's own per-user key is removed, which leaves a
/// machine-wide registration (a per-machine Stremio install) in effect again.
#[cfg(windows)]
fn register(app: &AppHandle, enabled: bool) -> Result<(), IpcError> {
    use tauri_plugin_deep_link::DeepLinkExt;
    let backup = app
        .path()
        .app_data_dir()
        .map_err(|e| registration_error(enabled, e))?
        .join("stremio-handler-backup.json");
    if enabled {
        if !backup.exists()
            && let Some(previous) = windows_handler::read()
            && !windows_handler::is_ours(&previous)
        {
            let json = serde_json::to_vec(&previous).map_err(|e| registration_error(true, e))?;
            std::fs::write(&backup, json).map_err(|e| registration_error(true, e))?;
        }
        return app
            .deep_link()
            .register(SCHEME)
            .map_err(|e| registration_error(true, e));
    }
    // another program took the scheme over meanwhile: leave it alone
    if windows_handler::read().is_some_and(|current| !windows_handler::is_ours(&current)) {
        let _ = std::fs::remove_file(&backup);
        return Ok(());
    }
    let previous = std::fs::read(&backup)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<windows_handler::Handler>(&bytes).ok());
    match previous {
        Some(previous) => windows_handler::restore(&previous),
        None => windows_handler::remove(),
    }
    .map_err(|e| registration_error(false, e))?;
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

#[cfg(windows)]
mod windows_handler {
    use serde::{Deserialize, Serialize};
    use windows_registry::CURRENT_USER;

    const KEY: &str = "Software\\Classes\\stremio";

    /// The per-user `stremio` key's values.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Handler {
        pub description: Option<String>,
        pub icon: Option<String>,
        pub command: Option<String>,
    }

    fn value(path: &str) -> Option<String> {
        CURRENT_USER.open(path).ok()?.get_string("").ok()
    }

    pub fn read() -> Option<Handler> {
        CURRENT_USER.open(KEY).ok()?;
        Some(Handler {
            description: value(KEY),
            icon: value(&format!("{KEY}\\DefaultIcon")),
            command: value(&format!("{KEY}\\shell\\open\\command")),
        })
    }

    /// Zeiler's own registration (the plugin writes this exe's path as the command).
    pub fn is_ours(handler: &Handler) -> bool {
        let Ok(exe) = tauri::utils::platform::current_exe() else {
            return false;
        };
        let exe = exe.display().to_string();
        let exe = exe.strip_prefix("\\\\?\\").unwrap_or(&exe);
        handler
            .command
            .as_deref()
            .is_some_and(|c| c.to_lowercase().contains(&exe.to_lowercase()))
    }

    pub fn restore(handler: &Handler) -> windows_registry::Result<()> {
        let key = CURRENT_USER.create(KEY)?;
        key.set_string("", handler.description.as_deref().unwrap_or(""))?;
        key.set_string("URL Protocol", "")?;
        if let Some(icon) = &handler.icon {
            CURRENT_USER
                .create(format!("{KEY}\\DefaultIcon"))?
                .set_string("", icon)?;
        }
        if let Some(command) = &handler.command {
            CURRENT_USER
                .create(format!("{KEY}\\shell\\open\\command"))?
                .set_string("", command)?;
        }
        Ok(())
    }

    pub fn remove() -> windows_registry::Result<()> {
        if CURRENT_USER.open(KEY).is_ok() {
            CURRENT_USER.remove_tree(KEY)?;
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn register(_app: &AppHandle, _enabled: bool) -> Result<(), IpcError> {
    Err(IpcError::Unsupported(
        "Opening stremio:// links isn't available on this system yet.".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_lone_stremio_argument_is_a_link() {
        assert_eq!(
            stremio_link_from_args(&["zeiler", "stremio://a.example/manifest.json"]).as_deref(),
            Some("stremio://a.example/manifest.json")
        );
        assert_eq!(
            stremio_link_from_args(&["zeiler", "STREMIO://a.example/x"]).as_deref(),
            Some("STREMIO://a.example/x")
        );
        for args in [
            vec!["zeiler"],
            vec!["zeiler", "--flag", "stremio://a.example/manifest.json"],
            vec!["zeiler", "https://a.example/manifest.json"],
            vec!["zeiler", "stremio://"],
            vec!["zeiler", "stremiox://a.example"],
            vec!["zeiler", "/path/to/file.mkv"],
        ] {
            assert_eq!(stremio_link_from_args(&args), None, "{args:?}");
        }
        let long = format!("stremio://a.example/{}", "x".repeat(5000));
        assert_eq!(stremio_link_from_args(&["zeiler", long.as_str()]), None);
    }

    #[test]
    fn a_repeat_within_the_window_counts_once() {
        let links = DeepLinkState::default();
        assert!(!links.is_repeat("stremio://a"));
        assert!(links.is_repeat("stremio://a"));
        assert!(!links.is_repeat("stremio://b"));
        assert!(!links.is_repeat("stremio://a"));
    }
}
