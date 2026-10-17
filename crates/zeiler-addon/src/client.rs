use std::time::Duration;

use serde::de::DeserializeOwned;
use url::Url;

use crate::error::AddonError;
use crate::manifest::{Descriptor, Manifest};
use crate::request::{ResourcePath, build_resource_url, validate_transport_url};
use crate::resource::{CatalogResponse, MetaResponse, StreamsResponse, SubtitlesResponse};

/// Make a manifest's relative `logo`/`background` absolute against its transport URL,
/// and drop one that isn't an http(s) image (`javascript:`, `file:`), which the UI
/// would otherwise put in an `<img src>`.
pub(crate) fn resolve_relative_images(manifest: &mut serde_json::Value, transport_url: &Url) {
    for key in ["logo", "background"] {
        let Some(serde_json::Value::String(value)) = manifest.get(key) else {
            continue;
        };
        let resolved = match Url::parse(value) {
            Ok(absolute) => Some(absolute),
            Err(_) if value.is_empty() => None,
            Err(_) => transport_url.join(value).ok(),
        };
        manifest[key] = match resolved {
            Some(url) if matches!(url.scheme(), "http" | "https") => {
                serde_json::Value::String(url.into())
            }
            _ => serde_json::Value::Null,
        };
    }
}

/// Hard cap on any addon response body. Addons are untrusted; the spec defines no limits.
pub const MAX_RESPONSE_BYTES: usize = 20 * 1024 * 1024;

/// HTTP client for the addon protocol. Follows redirects (Cinemeta answers 307 for
/// paginated catalogs), speaks gzip, sends a fixed `Zeiler/<version>` user agent, and
/// never sends cookies or custom headers.
#[derive(Debug, Clone)]
pub struct AddonClient {
    http: reqwest::Client,
}

impl AddonClient {
    pub fn new() -> Result<Self, AddonError> {
        Ok(Self {
            http: Self::http_builder().build()?,
        })
    }

    /// A client that ignores proxy settings, for tests against a loopback server (a
    /// proxy from the environment would otherwise get those requests). Not for
    /// production use.
    #[doc(hidden)]
    pub fn for_loopback_tests() -> Self {
        Self {
            http: Self::http_builder().no_proxy().build().unwrap(),
        }
    }

    fn http_builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .user_agent(concat!("Zeiler/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, url: Url) -> Result<T, AddonError> {
        let body = self.get_bytes(url, MAX_RESPONSE_BYTES).await?;
        Ok(serde_json::from_slice(&body)?)
    }

    /// GET `url` and return its body, refusing a non-success status and a body over
    /// `limit` bytes.
    pub(crate) async fn get_bytes(&self, url: Url, limit: usize) -> Result<Vec<u8>, AddonError> {
        // reqwest's errors name the URL, and a transport URL can carry the user's keys:
        // they go back to the UI and the log without it
        let response = self
            .http
            .get(url.clone())
            .send()
            .await
            .map_err(|e| AddonError::Http(e.without_url()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(AddonError::BadStatus {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }
        if let Some(len) = response.content_length()
            && len > limit as u64
        {
            return Err(AddonError::TooLarge { limit_bytes: limit });
        }
        // Stream the body and enforce the cap incrementally: with gzip/chunked responses
        // content_length() is None, and the cap must bound decompressed allocation, not
        // just check after the fact.
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AddonError::Http(e.without_url()))?
        {
            if body.len() + chunk.len() > limit {
                return Err(AddonError::TooLarge { limit_bytes: limit });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// Fetch and parse a manifest from its transport URL. A relative `logo` or
    /// `background` (`/logo.png`, `images/logo.png`) is resolved against the transport
    /// URL, as Stremio's apps and Harbor's `resolveAddonLogo` do, instead of being
    /// dropped as an invalid URL.
    pub async fn manifest(&self, transport_url: &Url) -> Result<Manifest, AddonError> {
        validate_transport_url(transport_url)?;
        let mut raw: serde_json::Value = self.get_json(transport_url.clone()).await?;
        resolve_relative_images(&mut raw, transport_url);
        Ok(serde_json::from_value(raw)?)
    }

    /// Fetch a manifest and wrap it as a Descriptor for the given transport URL.
    pub async fn descriptor(&self, transport_url: &Url) -> Result<Descriptor, AddonError> {
        let manifest = self.manifest(transport_url).await?;
        Ok(Descriptor {
            manifest,
            transport_url: transport_url.clone(),
            flags: Default::default(),
        })
    }

    /// Fetch an arbitrary resource; the response type picks the envelope.
    pub async fn resource<T: DeserializeOwned>(
        &self,
        transport_url: &Url,
        path: &ResourcePath,
    ) -> Result<T, AddonError> {
        let url = build_resource_url(transport_url, path)?;
        self.get_json(url).await
    }

    pub async fn catalog(
        &self,
        transport_url: &Url,
        path: &ResourcePath,
    ) -> Result<CatalogResponse, AddonError> {
        self.resource(transport_url, path).await
    }

    pub async fn meta(
        &self,
        transport_url: &Url,
        path: &ResourcePath,
    ) -> Result<MetaResponse, AddonError> {
        self.resource(transport_url, path).await
    }

    pub async fn streams(
        &self,
        transport_url: &Url,
        path: &ResourcePath,
    ) -> Result<StreamsResponse, AddonError> {
        self.resource(transport_url, path).await
    }

    pub async fn subtitles(
        &self,
        transport_url: &Url,
        path: &ResourcePath,
    ) -> Result<SubtitlesResponse, AddonError> {
        self.resource(transport_url, path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_manifest_images_resolve_against_the_transport_url() {
        let transport = Url::parse("https://a.example/cfg%7B%7D/manifest.json?k=1").unwrap();
        let mut manifest = serde_json::json!({
            "id": "x", "name": "X",
            "logo": "/images/logo.png",
            "background": "bg.jpg"
        });
        resolve_relative_images(&mut manifest, &transport);
        assert_eq!(manifest["logo"], "https://a.example/images/logo.png");
        assert_eq!(manifest["background"], "https://a.example/cfg%7B%7D/bg.jpg");

        let mut absolute = serde_json::json!({
            "logo": "https://cdn.example/logo.png",
            "background": "javascript:alert(1)"
        });
        resolve_relative_images(&mut absolute, &transport);
        assert_eq!(absolute["logo"], "https://cdn.example/logo.png");
        assert!(absolute["background"].is_null());
        let parsed: Manifest = serde_json::from_value(serde_json::json!({
            "id": "x", "name": "X", "logo": 5
        }))
        .unwrap();
        assert!(parsed.logo.is_none());
    }
}
