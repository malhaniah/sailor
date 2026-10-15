use std::time::Duration;

use serde::de::DeserializeOwned;
use url::Url;

use crate::error::AddonError;
use crate::manifest::{Descriptor, Manifest};
use crate::request::{ResourcePath, build_resource_url, validate_transport_url};
use crate::resource::{CatalogResponse, MetaResponse, StreamsResponse, SubtitlesResponse};

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
        let http = reqwest::Client::builder()
            .user_agent(concat!("Zeiler/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true)
            .build()?;
        Ok(Self { http })
    }

    async fn get_json<T: DeserializeOwned>(&self, url: Url) -> Result<T, AddonError> {
        let response = self.http.get(url.clone()).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(AddonError::BadStatus {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }
        if let Some(len) = response.content_length()
            && len > MAX_RESPONSE_BYTES as u64
        {
            return Err(AddonError::TooLarge {
                limit_bytes: MAX_RESPONSE_BYTES,
            });
        }
        // Stream the body and enforce the cap incrementally: with gzip/chunked responses
        // content_length() is None, and the cap must bound decompressed allocation, not
        // just check after the fact.
        let mut body = Vec::new();
        let mut response = response;
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(AddonError::TooLarge {
                    limit_bytes: MAX_RESPONSE_BYTES,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(serde_json::from_slice(&body)?)
    }

    /// Fetch and parse a manifest from its transport URL.
    pub async fn manifest(&self, transport_url: &Url) -> Result<Manifest, AddonError> {
        validate_transport_url(transport_url)?;
        self.get_json(transport_url.clone()).await
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
