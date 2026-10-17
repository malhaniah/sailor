use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_with::{DefaultOnError, VecSkipError, serde_as};

/// A Stremio session key. Never serialized, logged or shown: `Debug` is redacted, and
/// it leaves this crate only towards the keychain.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthKey(String);

impl AuthKey {
    pub fn new(key: impl Into<String>) -> Option<Self> {
        let key = key.into();
        let ok = !key.is_empty() && key.len() <= 512 && key.bytes().all(|b| b.is_ascii_graphic());
        ok.then_some(Self(key))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AuthKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthKey(<redacted>)")
    }
}

/// The parts of Stremio's user object Zeiler keeps. Everything else (a Trakt token,
/// GDPR fields) is dropped on the way in.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub email: String,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub avatar: Option<String>,
    /// When premium ends (RFC 3339), if the account has it.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default, rename = "premium_expire")]
    pub premium_expire: Option<String>,
}

/// An entry of the account's addon collection, kept raw so that pushing it back loses
/// nothing (unknown manifest fields, `flags`, anything Stremio adds later).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawDescriptor {
    pub manifest: Value,
    pub transport_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flags: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl RawDescriptor {
    pub fn manifest_id(&self) -> Option<&str> {
        self.manifest.get("id").and_then(Value::as_str)
    }

    /// Stremio marks its own addons `protected` (they can't be removed in its apps).
    pub fn is_protected(&self) -> bool {
        self.flags
            .as_ref()
            .and_then(|f| f.get("protected"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}

/// `addonCollectionGet`'s answer.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub addons: Vec<RawDescriptor>,
    #[serde(default)]
    pub last_modified: Option<Value>,
}

/// A password-less sign-in code (link.stremio.com).
#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkCode {
    pub code: String,
    pub link: String,
    /// A QR image of the link, when Stremio sends one (a `data:` or https URL).
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub qrcode: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn auth_keys_are_redacted_and_checked() {
        let key = AuthKey::new("abc123").unwrap();
        assert_eq!(format!("{key:?}"), "AuthKey(<redacted>)");
        assert_eq!(key.expose(), "abc123");
        assert!(AuthKey::new("").is_none());
        assert!(AuthKey::new("has space").is_none());
    }

    #[test]
    fn the_user_object_keeps_only_the_summary() {
        let user: UserSummary = serde_json::from_value(json!({
            "_id": "u1", "email": "a@example.com", "avatar": null,
            "trakt": {"access_token": "secret"}, "premium_expire": "2027-01-01T00:00:00Z",
            "gdpr_consent": {"tos": true}
        }))
        .unwrap();
        let out = serde_json::to_string(&user).unwrap();
        assert!(!out.contains("secret") && !out.contains("gdpr"));
        assert_eq!(user.premium_expire.as_deref(), Some("2027-01-01T00:00:00Z"));
    }

    #[test]
    fn raw_descriptors_round_trip_without_loss() {
        let raw = json!({
            "manifest": {"id": "org.a", "name": "A", "futureField": [1, 2]},
            "transportUrl": "https://a.example/manifest.json",
            "flags": {"official": false, "protected": false, "extraFlag": 1},
            "transportName": "http"
        });
        let parsed: RawDescriptor = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
        assert_eq!(parsed.manifest_id(), Some("org.a"));
        assert!(!parsed.is_protected());
    }
}
