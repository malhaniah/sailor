use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use serde_with::{VecSkipError, serde_as};
use url::Url;
use zeiler_account::{LibraryItem, LibraryItemModified};

use crate::error::SyncError;
use crate::types::{AuthKey, CollectionResponse, LinkCode, RawDescriptor, UserSummary};

/// Stremio's account API: `POST {API_BASE}<method>` with a JSON body.
pub const API_BASE: &str = "https://api.strem.io/api/";
/// Stremio's password-less sign-in (link codes).
pub const LINK_BASE: &str = "https://link.stremio.com/api/v2/";
/// Largest answer read (a big library is a few MB).
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
/// Library items written per `datastorePut`.
const PUT_CHUNK: usize = 50;
/// API error code for an authKey that is no longer valid (stremio-core logs out on it).
const SESSION_EXPIRED_CODE: i64 = 1;

/// A client for Stremio's account API. Honest user agent (`Zeiler/<version>`), no
/// cookies, no redirects, HTTPS only outside tests.
#[derive(Debug, Clone)]
pub struct StremioApi {
    http: reqwest::Client,
    api: Url,
    link: Url,
}

#[serde_as]
#[derive(Deserialize)]
struct Envelope<T> {
    result: Option<T>,
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct AuthResponse {
    #[serde(rename = "authKey")]
    auth_key: String,
    user: UserSummary,
}

#[derive(Deserialize)]
struct LinkRead {
    #[serde(rename = "authKey")]
    auth_key: String,
}

#[serde_as]
#[derive(Deserialize)]
#[serde(transparent)]
struct Items(#[serde_as(deserialize_as = "VecSkipError<_>")] Vec<LibraryItem>);

#[serde_as]
#[derive(Deserialize)]
#[serde(transparent)]
struct Modified(#[serde_as(deserialize_as = "VecSkipError<_>")] Vec<LibraryItemModified>);

impl StremioApi {
    pub fn new() -> Result<Self, SyncError> {
        Self::with_bases(
            Url::parse(API_BASE).expect("valid URL"),
            Url::parse(LINK_BASE).expect("valid URL"),
            true,
        )
    }

    /// A client for other bases: a mock server in tests (`https_only: false`).
    pub fn with_bases(api: Url, link: Url, https_only: bool) -> Result<Self, SyncError> {
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!("Zeiler/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .https_only(https_only)
            .gzip(true);
        if !https_only {
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(SyncError::from_http)?;
        Ok(Self { http, api, link })
    }

    async fn read_body(&self, response: reqwest::Response) -> Result<Vec<u8>, SyncError> {
        let status = response.status();
        if !status.is_success() {
            return Err(SyncError::Unexpected(format!("HTTP {}", status.as_u16())));
        }
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(SyncError::from_http)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(SyncError::Unexpected("the answer is too large".into()));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    fn unwrap<T: DeserializeOwned>(body: &[u8]) -> Result<T, SyncError> {
        let envelope: Envelope<T> = serde_json::from_slice(body)
            .map_err(|_| SyncError::Unexpected("an answer Zeiler could not read".into()))?;
        match (envelope.result, envelope.error) {
            (Some(result), None) => Ok(result),
            (_, Some(error)) if error.code == SESSION_EXPIRED_CODE => {
                Err(SyncError::SessionExpired)
            }
            (_, Some(error)) => Err(SyncError::Api {
                code: error.code,
                message: if error.message.is_empty() {
                    format!("Stremio refused the request (code {}).", error.code)
                } else {
                    error.message
                },
            }),
            (None, None) => Err(SyncError::Unexpected("an empty answer".into())),
        }
    }

    async fn call<T: DeserializeOwned>(&self, method: &str, body: Value) -> Result<T, SyncError> {
        let url = self
            .api
            .join(method)
            .map_err(|e| SyncError::Unexpected(e.to_string()))?;
        let response = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(SyncError::from_http)?;
        Self::unwrap(&self.read_body(response).await?)
    }

    /// Sign in with email and password. A refusal is `Auth`.
    pub async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(AuthKey, UserSummary), SyncError> {
        let body =
            json!({ "type": "Login", "email": email, "password": password, "facebook": false });
        let auth: AuthResponse = self.call("login", body).await.map_err(|e| match e {
            SyncError::Api { message, .. } | SyncError::Auth(message) => SyncError::Auth(message),
            SyncError::SessionExpired => SyncError::Auth("Wrong email or password.".into()),
            other => other,
        })?;
        let key = AuthKey::new(auth.auth_key)
            .ok_or_else(|| SyncError::Unexpected("an unusable session key".into()))?;
        Ok((key, auth.user))
    }

    /// Exchange a token from a confirmed link code for a session.
    pub async fn login_with_token(
        &self,
        token: &AuthKey,
    ) -> Result<(AuthKey, UserSummary), SyncError> {
        let body = json!({ "type": "LoginWithToken", "token": token.expose() });
        let auth: AuthResponse = self.call("loginWithToken", body).await?;
        let key = AuthKey::new(auth.auth_key)
            .ok_or_else(|| SyncError::Unexpected("an unusable session key".into()))?;
        Ok((key, auth.user))
    }

    /// End the session on Stremio's side (best effort; the caller forgets it anyway).
    pub async fn logout(&self, key: &AuthKey) -> Result<(), SyncError> {
        let _: Value = self
            .call(
                "logout",
                json!({ "type": "Logout", "authKey": key.expose() }),
            )
            .await?;
        Ok(())
    }

    pub async fn get_user(&self, key: &AuthKey) -> Result<UserSummary, SyncError> {
        self.call(
            "getUser",
            json!({ "type": "GetUser", "authKey": key.expose() }),
        )
        .await
    }

    pub async fn addon_collection_get(
        &self,
        key: &AuthKey,
    ) -> Result<CollectionResponse, SyncError> {
        self.call(
            "addonCollectionGet",
            json!({ "type": "AddonCollectionGet", "authKey": key.expose(), "update": true }),
        )
        .await
    }

    /// Replace the account's whole addon collection.
    pub async fn addon_collection_set(
        &self,
        key: &AuthKey,
        addons: &[RawDescriptor],
    ) -> Result<(), SyncError> {
        let _: Value = self
            .call(
                "addonCollectionSet",
                json!({ "type": "AddonCollectionSet", "authKey": key.expose(), "addons": addons }),
            )
            .await?;
        Ok(())
    }

    /// `[id, mtime]` of every library item in the account.
    pub async fn datastore_meta(
        &self,
        key: &AuthKey,
    ) -> Result<Vec<LibraryItemModified>, SyncError> {
        let modified: Modified = self
            .call(
                "datastoreMeta",
                json!({ "authKey": key.expose(), "collection": "libraryItem" }),
            )
            .await?;
        Ok(modified.0)
    }

    /// Library items by id (`all` = every item; `ids` is then ignored). Items that
    /// don't parse are skipped.
    pub async fn datastore_get(
        &self,
        key: &AuthKey,
        ids: &[String],
        all: bool,
    ) -> Result<Vec<LibraryItem>, SyncError> {
        let items: Items = self
            .call(
                "datastoreGet",
                json!({ "authKey": key.expose(), "collection": "libraryItem", "ids": ids, "all": all }),
            )
            .await?;
        Ok(items.0)
    }

    /// Write library items, 50 per request.
    pub async fn datastore_put(
        &self,
        key: &AuthKey,
        items: &[LibraryItem],
    ) -> Result<(), SyncError> {
        for chunk in items.chunks(PUT_CHUNK) {
            let _: Value = self
                .call(
                    "datastorePut",
                    json!({ "authKey": key.expose(), "collection": "libraryItem", "changes": chunk }),
                )
                .await?;
        }
        Ok(())
    }

    async fn link_get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, SyncError> {
        let mut url = self
            .link
            .join(path)
            .map_err(|e| SyncError::Unexpected(e.to_string()))?;
        url.query_pairs_mut().extend_pairs(query);
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(SyncError::from_http)?;
        Self::unwrap(&self.read_body(response).await?)
    }

    /// A new link code: the user opens `link` (or scans the QR code) and confirms.
    pub async fn link_create(&self) -> Result<LinkCode, SyncError> {
        self.link_get("create", &[("type", "Create")]).await
    }

    /// The token for a confirmed link code, or `LinkPending` until it is confirmed.
    pub async fn link_read(&self, code: &str) -> Result<AuthKey, SyncError> {
        match self
            .link_get::<LinkRead>("read", &[("type", "Read"), ("code", code)])
            .await
        {
            Ok(read) => AuthKey::new(read.auth_key)
                .ok_or_else(|| SyncError::Unexpected("an unusable token".into())),
            // not confirmed yet: the link API answers with an error until it is
            Err(SyncError::Api { .. }) | Err(SyncError::SessionExpired) => {
                Err(SyncError::LinkPending)
            }
            Err(other) => Err(other),
        }
    }
}
