//! Local profile commands. A switch swaps the addons, library, per-title player
//! choices and per-profile settings in place, then tells the UI (`profiles://changed`
//! and `settings://changed`), which remounts its shell for the new profile.
//!
//! A PIN guards switching to a profile, editing it and deleting it. It is a
//! convenience lock against casual use (the files are not encrypted), checked here,
//! with a growing wait after wrong attempts.

use std::collections::HashSet;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::account::AccountState;
use crate::commands::addons::UpdateCache;
use crate::commands::mpv::PlayerState;
use crate::commands::settings::SETTINGS_CHANGED;
use crate::error::IpcError;
use crate::iptv::IptvState;
use crate::library::LibraryState;
use crate::profiles::{self, Profile, ProfileView};
use crate::state::AppState;
use crate::subtitles::prefs::PlayerPrefs;

pub const PROFILES_CHANGED: &str = "profiles://changed";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesState {
    pub active_id: String,
    /// The active profile's PIN was entered in this run (or it has none).
    pub unlocked: bool,
    pub profiles: Vec<ProfileView>,
}

async fn snapshot(state: &AppState) -> ProfilesState {
    let file = state.profiles.read().await.clone();
    let active_id = file.active_id.clone().unwrap_or_default();
    let has_pin = file
        .profiles
        .iter()
        .any(|p| p.id == active_id && p.pin_hash.is_some());
    let unlocked = !has_pin
        || state
            .unlocked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&active_id);
    ProfilesState {
        profiles: file
            .profiles
            .iter()
            .map(|p| profiles::view(state.data_dir(), p, &active_id))
            .collect(),
        active_id,
        unlocked,
    }
}

async fn changed(app: &AppHandle, state: &AppState) -> ProfilesState {
    let snapshot = snapshot(state).await;
    let _ = app.emit(PROFILES_CHANGED, &snapshot);
    snapshot
}

fn find<'a>(profiles: &'a [Profile], id: &str) -> Result<&'a Profile, IpcError> {
    profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| IpcError::NotFound("That profile no longer exists.".into()))
}

/// Allow a change to `profile` when it has no PIN, is the active profile and was
/// unlocked when it was chosen, or `pin` is right. A wrong PIN counts towards the
/// wait. Only the active profile is ever "unlocked": switching away locks the one left.
fn authorize(state: &AppState, profile: &Profile, pin: Option<&str>) -> Result<(), IpcError> {
    let Some(hash) = profile.pin_hash.as_deref() else {
        return Ok(());
    };
    let unlocked = state
        .unlocked
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(&profile.id);
    if pin.is_none() && unlocked {
        return Ok(());
    }
    state.pin_attempts.check(&profile.id)?;
    match pin {
        Some(pin) if profiles::verify_pin(hash, pin) => {
            state.pin_attempts.succeeded(&profile.id);
            Ok(())
        }
        Some(_) => {
            state.pin_attempts.failed(&profile.id);
            Err(IpcError::Locked("That PIN is not right.".into()))
        }
        None => Err(IpcError::Locked(format!(
            "Enter the PIN for {} first.",
            profile.name
        ))),
    }
}

#[tauri::command]
pub async fn profile_list(state: State<'_, AppState>) -> Result<ProfilesState, IpcError> {
    Ok(snapshot(&state).await)
}

/// A new profile, with default preferences and no addons.
#[tauri::command]
pub async fn profile_create(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    color: String,
) -> Result<ProfilesState, IpcError> {
    let _op = state.profile_op.lock().await;
    let mut next = state.profiles.read().await.clone();
    if next.profiles.len() >= profiles::MAX_PROFILES {
        return Err(IpcError::InvalidInput(format!(
            "Zeiler keeps at most {} profiles.",
            profiles::MAX_PROFILES
        )));
    }
    let name = profiles::clean_name(&name, &next.profiles, None)?;
    profiles::check_color(&color)?;
    let profile = profiles::new_profile(name, color);
    tokio::fs::create_dir_all(profiles::profile_dir(state.data_dir(), &profile.id))
        .await
        .map_err(|e| IpcError::Storage(format!("Could not create the profile's folder: {e}")))?;
    next.profiles.push(profile);
    state.save_profiles(next).await?;
    Ok(changed(&app, &state).await)
}

/// Rename or recolour a profile.
#[tauri::command]
pub async fn profile_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    color: Option<String>,
    pin: Option<String>,
) -> Result<ProfilesState, IpcError> {
    let _op = state.profile_op.lock().await;
    let mut next = state.profiles.read().await.clone();
    authorize(&state, find(&next.profiles, &id)?, pin.as_deref())?;
    let name = name
        .map(|n| profiles::clean_name(&n, &next.profiles, Some(&id)))
        .transpose()?;
    if let Some(color) = &color {
        profiles::check_color(color)?;
    }
    let profile = next
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .expect("found above");
    if let Some(name) = name {
        profile.name = name;
    }
    if let Some(color) = color {
        profile.color = color;
    }
    state.save_profiles(next).await?;
    Ok(changed(&app, &state).await)
}

/// Set, change or remove a profile's PIN. A profile that has one needs it
/// (`current_pin`) for any change; `new_pin: None` removes it.
#[tauri::command]
pub async fn profile_set_pin(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    current_pin: Option<String>,
    new_pin: Option<String>,
) -> Result<ProfilesState, IpcError> {
    let _op = state.profile_op.lock().await;
    let mut next = state.profiles.read().await.clone();
    let profile = find(&next.profiles, &id)?;
    // so nobody can lock someone else's profile from their own
    if next.active_id.as_deref() != Some(id.as_str()) {
        return Err(IpcError::InvalidInput(
            "Switch to a profile to set or change its PIN.".into(),
        ));
    }
    if profile.pin_hash.is_some() {
        let pin = current_pin
            .as_deref()
            .ok_or_else(|| IpcError::Locked("Enter the current PIN first.".into()))?;
        authorize(&state, profile, Some(pin))?;
    }
    let hash = new_pin.as_deref().map(profiles::hash_pin).transpose()?;
    let profile = next
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
        .expect("found above");
    profile.pin_hash = hash;
    state.save_profiles(next).await?;
    // whoever just set it (on the active profile) knows it
    *state.unlocked.lock().unwrap_or_else(|e| e.into_inner()) = HashSet::from([id]);
    Ok(changed(&app, &state).await)
}

/// Set a profile's picture (a `data:` URL of a PNG, JPEG, WebP or GIF of at most
/// 512 KB), or remove it with `None`.
#[tauri::command]
pub async fn profile_set_avatar(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    data_url: Option<String>,
    pin: Option<String>,
) -> Result<ProfilesState, IpcError> {
    let _op = state.profile_op.lock().await;
    let mut next = state.profiles.read().await.clone();
    let profile = find(&next.profiles, &id)?;
    authorize(&state, profile, pin.as_deref())?;
    let dir = profiles::profile_dir(state.data_dir(), &id);
    let old = profile.avatar.clone();
    let file = match data_url {
        Some(url) => {
            let (bytes, extension) = profiles::decode_avatar(&url)?;
            let name = format!("avatar.{extension}");
            crate::persist::write_bytes(&dir.join(&name), bytes).await?;
            Some(name)
        }
        None => None,
    };
    if let Some(old) = old.filter(|old| Some(old) != file.as_ref() && profiles::is_avatar_file(old))
    {
        let _ = tokio::fs::remove_file(dir.join(old)).await;
    }
    next.profiles
        .iter_mut()
        .find(|p| p.id == id)
        .expect("found above")
        .avatar = file;
    state.save_profiles(next).await?;
    Ok(changed(&app, &state).await)
}

/// Switch to another profile. Refused while a player runs (its progress belongs to
/// the profile that started it).
#[tauri::command]
#[allow(
    clippy::too_many_arguments,
    reason = "Tauri injects each managed state"
)]
pub async fn profile_select(
    app: AppHandle,
    state: State<'_, AppState>,
    library: State<'_, LibraryState>,
    prefs: State<'_, PlayerPrefs>,
    player: State<'_, PlayerState>,
    updates: State<'_, UpdateCache>,
    account: State<'_, AccountState>,
    iptv: State<'_, IptvState>,
    id: String,
    pin: Option<String>,
) -> Result<ProfilesState, IpcError> {
    // lock order everywhere: `sync_lock`, then `profile_op` (an addon pull holds the
    // first and writes the addons under the second); no account sync runs across the
    // switch (one profile's key, another's library)
    let _sync = account.sync_lock.lock().await;
    let _op = state.profile_op.lock().await;
    let file = state.profiles.read().await.clone();
    let profile = find(&file.profiles, &id)?;
    authorize(&state, profile, pin.as_deref())?;
    let unlock = |state: &AppState| {
        *state.unlocked.lock().unwrap_or_else(|e| e.into_inner()) = HashSet::from([id.clone()]);
    };
    if file.active_id.as_deref() == Some(id.as_str()) {
        unlock(&state);
        return Ok(changed(&app, &state).await);
    }
    if player.is_playing() {
        return Err(IpcError::Conflict(
            "Stop playback before switching profiles.".into(),
        ));
    }
    // the last progress of the profile being left goes to its own library first
    // (including batches still queued from a playback stopped a moment ago); if that
    // doesn't finish, the switch waits for another try
    let _ = player.close_progress();
    if !library.settle(std::time::Duration::from_secs(3)).await {
        return Err(IpcError::Conflict(
            "Zeiler is still saving where you stopped watching. Try again in a moment.".into(),
        ));
    }
    // the stores that read a file switch first, and are put back if anything after
    // them fails, so a failed switch leaves everything on the profile being left
    let old = state.profile_dir();
    let new = profiles::profile_dir(state.data_dir(), &id);
    let storage = |e: anyhow::Error| IpcError::Storage(format!("{e:#}"));
    library.switch_to(&new).await.map_err(storage)?;
    if let Err(e) = prefs.switch_to(&new).await {
        let _ = library.switch_to(&old).await;
        return Err(storage(e));
    }
    if let Err(e) = account.switch_to(&new).await {
        let _ = library.switch_to(&old).await;
        let _ = prefs.switch_to(&old).await;
        return Err(storage(e));
    }
    if let Err(e) = iptv.switch_to(&new).await {
        let _ = library.switch_to(&old).await;
        let _ = prefs.switch_to(&old).await;
        let _ = account.switch_to(&old).await;
        return Err(storage(e));
    }
    if let Err(e) = state
        .switch_profile(&id, |after| {
            let _ = app.emit(SETTINGS_CHANGED, after);
        })
        .await
    {
        let _ = library.switch_to(&old).await;
        let _ = prefs.switch_to(&old).await;
        let _ = account.switch_to(&old).await;
        let _ = iptv.switch_to(&old).await;
        return Err(e);
    }
    unlock(&state);
    updates.clear();
    // addon changes not yet sent belonged to the profile left
    account.clear_pending();
    Ok(changed(&app, &state).await)
}

/// Delete a profile and its folder (addons, library, preferences, picture). The
/// primary profile and the active one can't be deleted.
#[tauri::command]
pub async fn profile_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    account: State<'_, AccountState>,
    iptv: State<'_, IptvState>,
    id: String,
    pin: Option<String>,
) -> Result<ProfilesState, IpcError> {
    let _op = state.profile_op.lock().await;
    let mut next = state.profiles.read().await.clone();
    let profile = find(&next.profiles, &id)?;
    if profile.is_primary {
        return Err(IpcError::InvalidInput(
            "The main profile can't be deleted.".into(),
        ));
    }
    if next.active_id.as_deref() == Some(id.as_str()) {
        return Err(IpcError::InvalidInput(
            "Switch to another profile before deleting this one.".into(),
        ));
    }
    authorize(&state, profile, pin.as_deref())?;
    next.profiles.retain(|p| p.id != id);
    state.save_profiles(next).await?;
    // its Stremio session and IPTV accounts go too (keychain entries would outlive it)
    account.sessions.forget(&id);
    iptv.delete_secrets_of(&profiles::profile_dir(state.data_dir(), &id))
        .await;
    // the profile is gone from profiles.json first: a folder left behind by a failed
    // removal is only disk space, never a half-deleted profile
    if let Err(e) = tokio::fs::remove_dir_all(profiles::profile_dir(state.data_dir(), &id)).await {
        tracing::warn!("could not remove a deleted profile's folder: {e}");
    }
    state
        .unlocked
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    Ok(changed(&app, &state).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn state_with_locked_profile(test: &str) -> (AppState, Profile) {
        let dir = crate::persist::tests::temp_dir(test);
        let state = AppState::load(dir.join("data"), dir.join("cache"))
            .await
            .unwrap();
        let mut locked = profiles::new_profile("Parent".into(), "red".into());
        locked.pin_hash = Some(profiles::hash_pin("2468").unwrap());
        let mut file = state.profiles.read().await.clone();
        file.profiles.push(locked.clone());
        state.save_profiles(file).await.unwrap();
        (state, locked)
    }

    #[tokio::test]
    async fn a_locked_profile_needs_its_pin_unless_it_is_the_unlocked_active_one() {
        let (state, locked) = state_with_locked_profile("profiles-authorize").await;
        assert!(matches!(
            authorize(&state, &locked, None),
            Err(IpcError::Locked(_))
        ));
        assert!(matches!(
            authorize(&state, &locked, Some("1111")),
            Err(IpcError::Locked(_))
        ));
        assert!(authorize(&state, &locked, Some("2468")).is_ok());
        // a right PIN for an edit doesn't unlock the profile for later calls
        assert!(authorize(&state, &locked, None).is_err());
        // only the active profile is ever in the set
        *state.unlocked.lock().unwrap() = HashSet::from([locked.id.clone()]);
        assert!(authorize(&state, &locked, None).is_ok());
        *state.unlocked.lock().unwrap() = HashSet::from(["someone-else".to_string()]);
        assert!(authorize(&state, &locked, None).is_err());
    }

    #[tokio::test]
    async fn an_active_profile_without_a_pin_reports_unlocked() {
        let (state, _) = state_with_locked_profile("profiles-unlocked").await;
        state.unlocked.lock().unwrap().clear();
        assert!(snapshot(&state).await.unlocked, "the primary has no PIN");
    }
}
