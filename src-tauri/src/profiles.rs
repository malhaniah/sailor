//! Local profiles: `profiles.json` in the data dir, and one folder per profile under
//! `profiles/<id>/` holding that profile's addons, library, per-title player choices,
//! preferences (`prefs.json`, the per-profile settings keys) and avatar.
//!
//! The first run of a build with profiles creates a primary profile and *copies* the
//! existing per-user files into its folder (never moves them), so an older build that
//! still reads them from the data dir keeps working. A PIN is a convenience lock against
//! casual use, not security: the files on disk are not encrypted. It is stored as an
//! Argon2id hash, checked in Rust, and wrong attempts back off.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_with::{DefaultOnError, VecSkipError, serde_as};
use tracing::info;

use crate::error::IpcError;
use crate::persist::{read_json, write_json};

pub const PROFILES_FILE: &str = "profiles.json";
const PROFILES_DIR: &str = "profiles";
pub const PREFS_FILE: &str = "prefs.json";
const FILE_VERSION: u32 = 1;
/// The per-user files a profile owns, copied from the data dir by the migration.
pub const PER_PROFILE_FILES: [&str; 3] = ["addons.json", "library.json", "player-prefs.json"];
/// The settings keys that belong to a profile (docs/ARCHITECTURE.md, "Settings
/// schema"); every other key is the device's. Keys not in `Settings` yet are skipped.
pub const PROFILE_KEYS: [&str; 12] = [
    "theme",
    "instantPlay",
    "hideAnime",
    "hideLiveTv",
    "showAdult",
    "hideP2pAddons",
    "homeRows",
    "subtitles",
    "audio",
    "skip",
    "bingeWatching",
    "upNextSeconds",
];
pub const MAX_PROFILES: usize = 8;
const MAX_NAME_CHARS: usize = 32;
const MAX_AVATAR_BYTES: usize = 512 * 1024;
/// Colours a profile can wear (ids the UI maps to its palette).
pub const COLORS: [&str; 8] = [
    "slate", "red", "orange", "amber", "green", "teal", "blue", "violet",
];

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfilesFile {
    pub version: u32,
    pub active_id: Option<String>,
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    pub profiles: Vec<Profile>,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub color: String,
    /// The avatar's file name in the profile folder, when it has one.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub is_primary: bool,
    /// Argon2id PHC string.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub pin_hash: Option<String>,
    /// Unix milliseconds.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub created_at: i64,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A profile id: a v4 UUID without dashes, so it is a safe folder name.
fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn is_profile_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn profile_dir(data_dir: &Path, id: &str) -> PathBuf {
    data_dir.join(PROFILES_DIR).join(id)
}

/// The profile-scoped part of a settings object (top-level camelCase keys).
pub fn profile_subset(settings: &Map<String, Value>) -> Map<String, Value> {
    settings
        .iter()
        .filter(|(k, _)| PROFILE_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Load `profiles.json`, creating the primary profile on first run (copying the
/// per-user files and the profile keys of `settings.json` into its folder). A damaged
/// or empty file adopts the profile folders already on disk instead of starting over.
pub async fn load_or_migrate(data_dir: &Path) -> anyhow::Result<ProfilesFile> {
    let path = data_dir.join(PROFILES_FILE);
    let mut file: ProfilesFile = read_json(&path).await?;
    file.profiles
        .retain(|p| is_profile_id(&p.id) && !p.name.trim().is_empty());
    let mut changed = false;
    if file.profiles.is_empty() {
        file.profiles = adopt_folders(data_dir).await;
        changed = true;
    }
    if file.profiles.is_empty() {
        remove_partial_folders(data_dir).await;
        file.profiles.push(migrate_primary(data_dir).await?);
        changed = true;
    }
    if !file.profiles.iter().any(|p| p.is_primary) {
        file.profiles[0].is_primary = true;
        changed = true;
    }
    let active_ok = file
        .active_id
        .as_deref()
        .is_some_and(|id| file.profiles.iter().any(|p| p.id == id));
    if !active_ok {
        file.active_id = file
            .profiles
            .iter()
            .find(|p| p.is_primary)
            .map(|p| p.id.clone());
        changed = true;
    }
    file.version = FILE_VERSION;
    for profile in &file.profiles {
        tokio::fs::create_dir_all(profile_dir(data_dir, &profile.id)).await?;
    }
    if changed {
        write_json(&path, &file).await?;
    }
    Ok(file)
}

async fn adopt_folders(data_dir: &Path) -> Vec<Profile> {
    let mut found: Vec<String> = Vec::new();
    let Ok(mut dir) = tokio::fs::read_dir(data_dir.join(PROFILES_DIR)).await else {
        return Vec::new();
    };
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_profile_id(&name) && entry.path().is_dir() {
            found.push(name);
        }
    }
    found.sort();
    found
        .into_iter()
        .enumerate()
        .map(|(i, id)| Profile {
            id,
            name: if i == 0 {
                "Main".into()
            } else {
                format!("Profile {}", i + 1)
            },
            color: COLORS[i % COLORS.len()].into(),
            avatar: None,
            is_primary: i == 0,
            pin_hash: None,
            created_at: now_ms(),
        })
        .collect()
}

/// Leftovers of a first run that crashed halfway (see `migrate_primary`).
async fn remove_partial_folders(data_dir: &Path) {
    let Ok(mut dir) = tokio::fs::read_dir(data_dir.join(PROFILES_DIR)).await else {
        return;
    };
    while let Ok(Some(entry)) = dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name.ends_with(".partial") {
            let _ = tokio::fs::remove_dir_all(entry.path()).await;
        }
    }
}

/// The primary profile's folder is filled under a temporary name and renamed into
/// place only when complete, so a crash halfway leaves no folder that looks like a
/// profile (`adopt_folders` ignores the partial one; the next start copies again).
async fn migrate_primary(data_dir: &Path) -> anyhow::Result<Profile> {
    let id = new_id();
    let partial = data_dir.join(PROFILES_DIR).join(format!(".{id}.partial"));
    tokio::fs::create_dir_all(&partial).await?;
    for name in PER_PROFILE_FILES {
        let from = data_dir.join(name);
        if tokio::fs::try_exists(&from).await.unwrap_or(false) {
            // a failed copy fails startup (profiles.json is not written yet, so the
            // next start tries again) rather than starting the profile empty
            tokio::fs::copy(&from, partial.join(name))
                .await
                .map_err(|e| {
                    anyhow::anyhow!("could not copy {name} into the primary profile: {e}")
                })?;
            info!("copied {name} into the primary profile");
        }
    }
    // the profile keys of settings.json become the profile's preferences
    let settings: Value = match tokio::fs::read(data_dir.join("settings.json")).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    if let Value::Object(settings) = settings {
        write_json(
            &partial.join(PREFS_FILE),
            &Value::Object(profile_subset(&settings)),
        )
        .await?;
    }
    tokio::fs::rename(&partial, profile_dir(data_dir, &id)).await?;
    Ok(Profile {
        id,
        name: "Main".into(),
        color: COLORS[0].into(),
        avatar: None,
        is_primary: true,
        pin_hash: None,
        created_at: now_ms(),
    })
}

// ---- PIN -------------------------------------------------------------------------------

/// A PIN is 4 to 8 digits.
pub fn check_pin_format(pin: &str) -> Result<(), IpcError> {
    if (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(IpcError::InvalidInput("A PIN is 4 to 8 digits.".into()))
    }
}

pub fn hash_pin(pin: &str) -> Result<String, IpcError> {
    check_pin_format(pin)?;
    // 16 random bytes from the OS (a v4 UUID is 122 of them)
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes())
        .map_err(|e| IpcError::Storage(format!("Could not store the PIN: {e}")))?;
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| IpcError::Storage(format!("Could not store the PIN: {e}")))
}

pub fn verify_pin(hash: &str, pin: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(pin.as_bytes(), &parsed)
            .is_ok()
    })
}

/// Wrong PIN attempts per profile, with a growing wait after the third.
#[derive(Default)]
pub struct PinAttempts {
    failures: std::sync::Mutex<HashMap<String, (u32, Instant)>>,
}

impl PinAttempts {
    fn wait_for(failures: u32) -> Duration {
        match failures {
            0..=2 => Duration::ZERO,
            n => Duration::from_secs((1u64 << (n - 3).min(6)).min(60)),
        }
    }

    /// Refuse while the profile is waiting out earlier failures.
    pub fn check(&self, id: &str) -> Result<(), IpcError> {
        let failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((count, at)) = failures.get(id) {
            let wait = Self::wait_for(*count);
            let elapsed = at.elapsed();
            if elapsed < wait {
                let left = (wait - elapsed).as_secs().max(1);
                return Err(IpcError::Locked(format!(
                    "Too many wrong PINs. Try again in {left} s."
                )));
            }
        }
        Ok(())
    }

    pub fn failed(&self, id: &str) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let entry = failures
            .entry(id.to_string())
            .or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
    }

    pub fn succeeded(&self, id: &str) {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
    }
}

// ---- avatars ---------------------------------------------------------------------------

/// An avatar's image type from its first bytes: PNG, JPEG, WebP or GIF.
pub fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("jpg"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("webp"),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some("gif"),
        _ => None,
    }
}

fn mime(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/gif",
    }
}

/// Decode a `data:image/…;base64,…` URL into checked image bytes and their extension.
pub fn decode_avatar(data_url: &str) -> Result<(Vec<u8>, &'static str), IpcError> {
    let bad =
        || IpcError::InvalidInput("The picture must be a PNG, JPEG, WebP or GIF image.".into());
    let (head, body) = data_url.split_once(',').ok_or_else(bad)?;
    if !head.starts_with("data:image/") || !head.ends_with(";base64") {
        return Err(bad());
    }
    if body.len() > MAX_AVATAR_BYTES * 4 / 3 + 4 {
        return Err(IpcError::InvalidInput(
            "The picture is too large (512 KB at most).".into(),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| bad())?;
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(IpcError::InvalidInput(
            "The picture is too large (512 KB at most).".into(),
        ));
    }
    let extension = image_extension(&bytes).ok_or_else(bad)?;
    Ok((bytes, extension))
}

pub fn avatar_data_url(bytes: &[u8], extension: &str) -> String {
    format!(
        "data:{};base64,{}",
        mime(extension),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

// ---- validation and views --------------------------------------------------------------

pub fn clean_name(
    name: &str,
    others: &[Profile],
    except: Option<&str>,
) -> Result<String, IpcError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(IpcError::InvalidInput(format!(
            "A profile name is 1 to {MAX_NAME_CHARS} characters."
        )));
    }
    if others
        .iter()
        .any(|p| Some(p.id.as_str()) != except && p.name.trim().eq_ignore_ascii_case(name))
    {
        return Err(IpcError::Conflict(format!(
            "There is already a profile called {name}."
        )));
    }
    Ok(name.to_string())
}

pub fn check_color(color: &str) -> Result<(), IpcError> {
    if COLORS.contains(&color) {
        Ok(())
    } else {
        Err(IpcError::InvalidInput(
            "That is not one of the profile colours.".into(),
        ))
    }
}

/// A profile as the UI sees it: never the PIN hash.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub color: String,
    /// A `data:` URL of the avatar picture.
    pub avatar: Option<String>,
    pub is_primary: bool,
    /// It has a PIN.
    pub locked: bool,
    pub active: bool,
}

/// The view of `profile` for the UI (its avatar read from its folder).
/// An avatar's file name as Zeiler writes it (`avatar.<ext>`); anything else in a
/// hand-edited `profiles.json` is ignored rather than joined into a path.
pub fn is_avatar_file(name: &str) -> bool {
    ["avatar.png", "avatar.jpg", "avatar.webp", "avatar.gif"].contains(&name)
}

pub fn view(data_dir: &Path, profile: &Profile, active_id: &str) -> ProfileView {
    let avatar = profile
        .avatar
        .as_deref()
        .filter(|f| is_avatar_file(f))
        .and_then(|file| {
            let extension = file.rsplit('.').next()?;
            let bytes = std::fs::read(profile_dir(data_dir, &profile.id).join(file)).ok()?;
            (image_extension(&bytes) == Some(extension)).then(|| avatar_data_url(&bytes, extension))
        });
    ProfileView {
        id: profile.id.clone(),
        name: profile.name.clone(),
        color: profile.color.clone(),
        avatar,
        is_primary: profile.is_primary,
        locked: profile.pin_hash.is_some(),
        active: profile.id == active_id,
    }
}

/// A new profile (not yet saved).
pub fn new_profile(name: String, color: String) -> Profile {
    Profile {
        id: new_id(),
        name,
        color,
        avatar: None,
        is_primary: false,
        pin_hash: None,
        created_at: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::tests::temp_dir;
    use serde_json::json;

    #[tokio::test]
    async fn first_run_copies_files_and_profile_keys_into_a_primary_profile() {
        let dir = temp_dir("profiles-migrate");
        std::fs::write(dir.join("addons.json"), "[]").unwrap();
        std::fs::write(dir.join("library.json"), r#"{"items":{}}"#).unwrap();
        std::fs::write(
            dir.join("settings.json"),
            r#"{"theme":"ocean","mpvPath":"/opt/mpv","hideAnime":true,"subtitles":{"langs":["ar"]}}"#,
        )
        .unwrap();

        let file = load_or_migrate(&dir).await.unwrap();

        assert_eq!(file.profiles.len(), 1);
        let primary = &file.profiles[0];
        assert!(primary.is_primary && primary.pin_hash.is_none());
        assert_eq!(file.active_id.as_deref(), Some(primary.id.as_str()));
        let folder = profile_dir(&dir, &primary.id);
        assert_eq!(
            std::fs::read_to_string(folder.join("addons.json")).unwrap(),
            "[]"
        );
        assert!(folder.join("library.json").exists());
        assert!(
            !folder.join("player-prefs.json").exists(),
            "only what existed"
        );
        // copied, not moved: an older build still finds its files
        assert!(dir.join("addons.json").exists() && dir.join("library.json").exists());
        // the profile keys, and only those, became the profile's preferences
        let prefs: Value =
            serde_json::from_str(&std::fs::read_to_string(folder.join(PREFS_FILE)).unwrap())
                .unwrap();
        assert_eq!(
            prefs,
            json!({"theme": "ocean", "hideAnime": true, "subtitles": {"langs": ["ar"]}})
        );

        // the next start reads the same profile, without migrating again
        let again = load_or_migrate(&dir).await.unwrap();
        assert_eq!(again.profiles, file.profiles);
    }

    #[tokio::test]
    async fn a_half_done_migration_is_not_adopted_and_runs_again() {
        let dir = temp_dir("profiles-partial");
        std::fs::write(dir.join("addons.json"), "[]").unwrap();
        // a crash after copying, before the rename and profiles.json
        std::fs::create_dir_all(dir.join("profiles/.abc.partial")).unwrap();
        let file = load_or_migrate(&dir).await.unwrap();
        assert_eq!(file.profiles.len(), 1);
        let folder = profile_dir(&dir, &file.profiles[0].id);
        assert!(folder.join("addons.json").exists());
        assert!(
            !dir.join("profiles/.abc.partial").exists(),
            "the leftover is removed"
        );
    }

    #[tokio::test]
    async fn a_damaged_profiles_file_adopts_the_folders_on_disk() {
        let dir = temp_dir("profiles-adopt");
        let first = load_or_migrate(&dir).await.unwrap();
        let id = first.profiles[0].id.clone();
        std::fs::write(dir.join(PROFILES_FILE), "{ broken").unwrap();

        let recovered = load_or_migrate(&dir).await.unwrap();

        assert_eq!(recovered.profiles.len(), 1);
        assert_eq!(
            recovered.profiles[0].id, id,
            "the same folder, not a new one"
        );
        assert!(recovered.profiles[0].is_primary);
    }

    #[tokio::test]
    async fn an_unknown_active_id_falls_back_to_the_primary() {
        let dir = temp_dir("profiles-active");
        let mut file = load_or_migrate(&dir).await.unwrap();
        file.active_id = Some("f".repeat(32));
        write_json(&dir.join(PROFILES_FILE), &file).await.unwrap();
        let fixed = load_or_migrate(&dir).await.unwrap();
        assert_eq!(fixed.active_id, Some(fixed.profiles[0].id.clone()));
    }

    #[test]
    fn pins_are_digits_hashed_with_argon2id_and_verified() {
        assert!(check_pin_format("1234").is_ok());
        assert!(check_pin_format("12345678").is_ok());
        for bad in ["123", "123456789", "12a4", "", "١٢٣٤"] {
            assert!(check_pin_format(bad).is_err(), "{bad}");
        }
        let hash = hash_pin("2468").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(!hash.contains("2468"));
        assert!(verify_pin(&hash, "2468"));
        assert!(!verify_pin(&hash, "2469"));
        assert!(!verify_pin("not a hash", "2468"));
        assert_ne!(hash_pin("2468").unwrap(), hash, "salted");
    }

    #[test]
    fn wrong_pins_back_off_after_three() {
        let attempts = PinAttempts::default();
        for _ in 0..3 {
            assert!(attempts.check("p").is_ok());
            attempts.failed("p");
        }
        assert!(matches!(attempts.check("p"), Err(IpcError::Locked(_))));
        assert!(attempts.check("other").is_ok());
        attempts.succeeded("p");
        assert!(attempts.check("p").is_ok());
        assert_eq!(PinAttempts::wait_for(3), Duration::from_secs(1));
        assert_eq!(PinAttempts::wait_for(20), Duration::from_secs(60));
    }

    #[test]
    fn avatars_must_be_small_real_images() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        let url = avatar_data_url(&png, "png");
        assert!(url.starts_with("data:image/png;base64,"));
        let (bytes, ext) = decode_avatar(&url).unwrap();
        assert_eq!((bytes.as_slice(), ext), (&png[..], "png"));

        let html = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b"<svg onload=alert(1)>")
        );
        assert!(decode_avatar(&html).is_err(), "magic bytes, not the label");
        assert!(decode_avatar("data:text/html;base64,AAAA").is_err());
        assert!(decode_avatar("https://example.com/a.png").is_err());
        let big = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(vec![0u8; MAX_AVATAR_BYTES + 1])
        );
        assert!(decode_avatar(&big).is_err());
        assert_eq!(image_extension(b"GIF89a.."), Some("gif"));
        assert_eq!(image_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(image_extension(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
    }

    #[test]
    fn names_are_short_unique_and_trimmed() {
        let existing = vec![new_profile("Kids".into(), "red".into())];
        assert_eq!(clean_name("  Sam ", &existing, None).unwrap(), "Sam");
        assert!(matches!(
            clean_name("kids", &existing, None),
            Err(IpcError::Conflict(_))
        ));
        let id = existing[0].id.clone();
        assert_eq!(clean_name("KIDS", &existing, Some(&id)).unwrap(), "KIDS");
        assert!(clean_name("", &existing, None).is_err());
        assert!(clean_name(&"x".repeat(33), &existing, None).is_err());
        assert!(clean_name("a\nb", &existing, None).is_err());
        assert!(check_color("teal").is_ok() && check_color("#fff").is_err());
    }

    #[test]
    fn the_profile_subset_keeps_only_profile_keys() {
        let settings =
            json!({"theme": "x", "mpvPath": "/m", "showAdult": true, "autoUpdate": false});
        let subset = profile_subset(settings.as_object().unwrap());
        assert_eq!(
            Value::Object(subset),
            json!({"theme": "x", "showAdult": true})
        );
    }
}
