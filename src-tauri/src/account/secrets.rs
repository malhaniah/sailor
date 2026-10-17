//! Where a Stremio session key lives: the OS keychain (macOS Keychain, Windows
//! Credential Manager, the Secret Service on Linux), or memory only when the user
//! didn't ask to be remembered or the keychain isn't available. Never a file, never
//! settings, never IPC. One entry per profile (`stremio-auth:<profileId>`).
//!
//! Shared with track B's IPTV credentials (M9) if it needs one.

use std::collections::HashMap;
use std::sync::Mutex;

/// The keychain service name.
pub const SERVICE: &str = "io.zeiler.desktop";

/// Where the key ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Storage {
    /// The OS keychain: kept across restarts.
    Os,
    /// Memory: gone when Zeiler quits.
    Memory,
}

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Option<String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, account: &str);
}

/// Secrets for this run only (the tests' keychain).
#[derive(Default)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "the stand-in keychain of the tests")
)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Option<String> {
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(account)
            .cloned()
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) {
        self.secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(account);
    }
}

/// The OS keychain. Secrets are stored as their UTF-8 bytes (`set_secret`), not as a
/// password string: Windows Credential Manager holds 2560 bytes, which UTF-16 would
/// halve (the same store as M9's IPTV credentials, whose playlist URLs get long).
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Option<String> {
        let bytes = keyring::Entry::new(SERVICE, account)
            .ok()?
            .get_secret()
            .ok()?;
        String::from_utf8(bytes).ok()
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|entry| entry.set_secret(secret.as_bytes()))
            .map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) {
        if let Ok(entry) = keyring::Entry::new(SERVICE, account) {
            let _ = entry.delete_credential();
        }
    }
}

/// Session keys by profile: the keychain when the user asked to be remembered (and it
/// works), memory otherwise. A key read from the keychain is kept in memory too, so
/// the keychain is asked once per profile and run (a keychain prompt can block), not
/// on every status check.
pub struct Sessions {
    os: Box<dyn SecretStore>,
    cache: Mutex<HashMap<String, (String, Storage)>>,
}

fn account(profile_id: &str) -> String {
    format!("stremio-auth:{profile_id}")
}

impl Sessions {
    pub fn new(os: Box<dyn SecretStore>) -> Self {
        Self {
            os,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<String, (String, Storage)>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keep `secret` for `profile_id`; where it went.
    pub fn store(&self, profile_id: &str, secret: &str, remember: bool) -> Storage {
        let account = account(profile_id);
        self.forget(profile_id);
        let storage = if remember {
            match self.os.set(&account, secret) {
                Ok(()) => Storage::Os,
                Err(e) => {
                    tracing::warn!(
                        "the keychain is not available ({e}); keeping the session in memory"
                    );
                    Storage::Memory
                }
            }
        } else {
            Storage::Memory
        };
        self.cache().insert(account, (secret.to_string(), storage));
        storage
    }

    pub fn load(&self, profile_id: &str) -> Option<(String, Storage)> {
        let account = account(profile_id);
        if let Some(found) = self.cache().get(&account) {
            return Some(found.clone());
        }
        let secret = self.os.get(&account)?;
        self.cache().insert(account, (secret.clone(), Storage::Os));
        Some((secret, Storage::Os))
    }

    pub fn forget(&self, profile_id: &str) {
        let account = account(profile_id);
        self.cache().remove(&account);
        self.os.delete(&account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keychain that refuses every write (no Secret Service daemon, say).
    struct Broken;
    impl SecretStore for Broken {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Err("no keychain".into())
        }
        fn delete(&self, _: &str) {}
    }

    #[test]
    fn remembered_sessions_go_to_the_keychain_and_others_stay_in_memory() {
        let sessions = Sessions::new(Box::new(MemoryStore::default()));
        assert_eq!(sessions.store("p1", "k1", true), Storage::Os);
        assert_eq!(sessions.load("p1"), Some(("k1".into(), Storage::Os)));
        assert_eq!(sessions.store("p2", "k2", false), Storage::Memory);
        assert_eq!(sessions.load("p2"), Some(("k2".into(), Storage::Memory)));
        assert_eq!(sessions.load("p3"), None, "per profile");
        sessions.forget("p1");
        assert_eq!(sessions.load("p1"), None);
    }

    #[test]
    fn a_broken_keychain_falls_back_to_memory_never_to_a_file() {
        let sessions = Sessions::new(Box::new(Broken));
        assert_eq!(sessions.store("p1", "k1", true), Storage::Memory);
        assert_eq!(sessions.load("p1"), Some(("k1".into(), Storage::Memory)));
    }

    /// Counts keychain reads.
    #[derive(Default)]
    struct Counting {
        inner: MemoryStore,
        reads: std::sync::atomic::AtomicUsize,
    }
    impl SecretStore for Counting {
        fn get(&self, account: &str) -> Option<String> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.get(account)
        }
        fn set(&self, account: &str, secret: &str) -> Result<(), String> {
            self.inner.set(account, secret)
        }
        fn delete(&self, account: &str) {
            self.inner.delete(account)
        }
    }

    #[test]
    fn the_keychain_is_read_once_per_profile() {
        let keychain = std::sync::Arc::new(Counting::default());
        keychain.set("stremio-auth:p1", "k1").unwrap();
        struct Shared(std::sync::Arc<Counting>);
        impl SecretStore for Shared {
            fn get(&self, a: &str) -> Option<String> {
                self.0.get(a)
            }
            fn set(&self, a: &str, s: &str) -> Result<(), String> {
                self.0.set(a, s)
            }
            fn delete(&self, a: &str) {
                self.0.delete(a)
            }
        }
        let sessions = Sessions::new(Box::new(Shared(keychain.clone())));
        for _ in 0..3 {
            assert_eq!(sessions.load("p1"), Some(("k1".into(), Storage::Os)));
        }
        assert_eq!(keychain.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn signing_in_again_without_remember_drops_the_keychain_copy() {
        let sessions = Sessions::new(Box::new(MemoryStore::default()));
        sessions.store("p1", "old", true);
        sessions.store("p1", "new", false);
        sessions.forget("p1");
        assert_eq!(sessions.load("p1"), None);
    }
}
