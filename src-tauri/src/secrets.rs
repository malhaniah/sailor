//! IPTV accounts in the OS keychain: M12's store (`account/secrets.rs`, the same
//! `io.zeiler.desktop` service, values as UTF-8 bytes) with a fallback of IPTV's own. When
//! the keychain can't be used (no Secret Service running, a denied prompt), a secret
//! lives in memory for this run only, and the UI says so; it never falls back to
//! plaintext on disk.
//!
//! Keys name what they hold (`iptv:<source id>`; M12's are `stremio-auth:<profile id>`).
//! Values are small JSON strings. Nothing here logs a value.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tracing::warn;

use crate::account::secrets::SecretStore;
pub use crate::account::secrets::Storage as SecretStorage;

/// The secrets: the keychain, or memory when the keychain fails. A failed write moves
/// that secret to memory (and says so); a read looks in memory first, then in the
/// keychain. Keychain calls run off the async runtime (a D-Bus round trip, a macOS
/// prompt can block).
pub struct Secrets {
    os: Arc<dyn SecretStore>,
    memory: Mutex<HashMap<String, String>>,
}

impl Secrets {
    pub fn new(os: Arc<dyn SecretStore>) -> Self {
        Self {
            os,
            memory: Mutex::new(HashMap::new()),
        }
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.memory.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn SecretStore) -> T + Send + 'static,
    ) -> T {
        let os = self.os.clone();
        tokio::task::spawn_blocking(move || f(os.as_ref()))
            .await
            .expect("keychain task panicked")
    }

    /// The secret and where it was found. A keychain that can't be read reads as empty.
    pub async fn locate(&self, key: &str) -> Option<(String, SecretStorage)> {
        if let Some(value) = self.memory().get(key).cloned() {
            return Some((value, SecretStorage::Memory));
        }
        let owned = key.to_string();
        self.blocking(move |os| os.get(&owned))
            .await
            .map(|value| (value, SecretStorage::Os))
    }

    /// Stores `value`; says where it went.
    pub async fn set(&self, key: &str, value: &str) -> SecretStorage {
        let (k, v) = (key.to_string(), value.to_string());
        match self.blocking(move |os| os.set(&k, &v)).await {
            Ok(()) => {
                self.memory().remove(key);
                SecretStorage::Os
            }
            Err(e) => {
                warn!(key, error = %e, "the keychain is unavailable; keeping this secret in memory");
                self.memory().insert(key.to_string(), value.to_string());
                SecretStorage::Memory
            }
        }
    }

    /// Removes `key` wherever it is (M12's store reports no delete errors).
    pub async fn delete(&self, key: &str) {
        self.memory().remove(key);
        let owned = key.to_string();
        self.blocking(move |os| os.delete(&owned)).await;
    }
}

#[cfg(test)]
mod tests {
    use crate::account::secrets::MemoryStore;

    use super::*;

    /// A keychain that refuses every write (no Secret Service, a denied prompt).
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

    #[tokio::test]
    async fn the_keychain_is_used_when_it_works() {
        let secrets = Secrets::new(Arc::new(MemoryStore::default()));
        assert_eq!(secrets.set("iptv:a", "s3cret").await, SecretStorage::Os);
        assert_eq!(
            secrets.locate("iptv:a").await,
            Some(("s3cret".to_string(), SecretStorage::Os))
        );
        secrets.delete("iptv:a").await;
        assert_eq!(secrets.locate("iptv:a").await, None);
    }

    #[tokio::test]
    async fn a_broken_keychain_falls_back_to_memory_never_to_disk() {
        let secrets = Secrets::new(Arc::new(Broken));
        assert_eq!(secrets.set("iptv:a", "s3cret").await, SecretStorage::Memory);
        assert_eq!(
            secrets.locate("iptv:a").await,
            Some(("s3cret".to_string(), SecretStorage::Memory))
        );
        secrets.delete("iptv:a").await;
        assert_eq!(
            secrets.locate("iptv:a").await,
            None,
            "the memory copy is gone"
        );
    }
}
