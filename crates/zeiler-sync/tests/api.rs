//! The account API client against a local mock of api.strem.io and link.stremio.com.
//! No real account is ever used: the request shapes are asserted against stremio-core's
//! (recorded in the fixtures), and the answers are canned.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;
use zeiler_sync::{AuthKey, StremioApi, SyncError};

type Routes = Vec<(&'static str, Value)>;

/// One request as the mock saw it: path (with query) and JSON body.
type Seen = Arc<Mutex<Vec<(String, Value)>>>;

async fn mock(routes: Routes) -> (StremioApi, Seen) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let routes = routes.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                // headers, then a content-length body
                let (head_end, length) = loop {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                    if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while buf.len() < head_end + length {
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let body: Value = serde_json::from_slice(&buf[head_end..]).unwrap_or(Value::Null);
                log.lock().unwrap().push((target.clone(), body));
                let path = target.split('?').next().unwrap_or("").to_string();
                let answer = routes
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, v)| v.to_string());
                let (status, body) = match answer {
                    Some(body) => ("200 OK", body),
                    None => ("404 Not Found", String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let api = StremioApi::with_bases(
        Url::parse(&format!("{base}/api/")).unwrap(),
        Url::parse(&format!("{base}/link/")).unwrap(),
        false,
    )
    .unwrap();
    (api, seen)
}

fn key() -> AuthKey {
    AuthKey::new("test-auth-key").unwrap()
}

const USER: &str = include_str!("fixtures/user.json");
const COLLECTION: &str = include_str!("fixtures/addon-collection.json");
const LIBRARY: &str = include_str!("fixtures/datastore-get.json");

#[tokio::test]
async fn password_login_sends_stremio_cores_body_and_keeps_only_the_summary() {
    let user: Value = serde_json::from_str(USER).unwrap();
    let (api, seen) = mock(vec![(
        "/api/login",
        json!({ "result": { "authKey": "k1", "user": user } }),
    )])
    .await;
    let (key, user) = api.login("a@example.com", "pw").await.unwrap();
    assert_eq!(key.expose(), "k1");
    assert_eq!(user.id, "5f0000000000000000000001");
    assert_eq!(user.email, "a@example.com");
    let (path, body) = seen.lock().unwrap()[0].clone();
    assert_eq!(path, "/api/login");
    assert_eq!(
        body,
        json!({ "type": "Login", "email": "a@example.com", "password": "pw", "facebook": false })
    );
}

#[tokio::test]
async fn a_refused_login_is_an_auth_error() {
    let (api, _) = mock(vec![(
        "/api/login",
        json!({ "error": { "code": 2, "message": "Wrong passphrase" } }),
    )])
    .await;
    assert!(matches!(
        api.login("a@example.com", "bad").await,
        Err(SyncError::Auth(m)) if m == "Wrong passphrase"
    ));
}

#[tokio::test]
async fn error_code_1_means_the_session_ended() {
    let (api, _) = mock(vec![(
        "/api/getUser",
        json!({ "error": { "code": 1, "message": "Session does not exist" } }),
    )])
    .await;
    assert!(matches!(
        api.get_user(&key()).await,
        Err(SyncError::SessionExpired)
    ));
}

#[tokio::test]
async fn link_codes_are_created_then_pending_then_exchanged() {
    let user: Value = serde_json::from_str(USER).unwrap();
    let (api, seen) = mock(vec![
        (
            "/link/create",
            json!({ "result": { "code": "ABCD", "link": "https://link.stremio.com/?code=ABCD", "qrcode": "data:image/png;base64,AAAA" } }),
        ),
        ("/link/read", json!({ "error": { "code": 404, "message": "not yet" } })),
        ("/api/loginWithToken", json!({ "result": { "authKey": "k2", "user": user } })),
    ])
    .await;
    let code = api.link_create().await.unwrap();
    assert_eq!(code.code, "ABCD");
    assert!(matches!(
        api.link_read("ABCD").await,
        Err(SyncError::LinkPending)
    ));
    let (key, _) = api
        .login_with_token(&AuthKey::new("token").unwrap())
        .await
        .unwrap();
    assert_eq!(key.expose(), "k2");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[0].0, "/link/create?type=Create");
    assert_eq!(seen[1].0, "/link/read?type=Read&code=ABCD");
    assert_eq!(
        seen[2].1,
        json!({ "type": "LoginWithToken", "token": "token" })
    );
}

#[tokio::test]
async fn the_addon_collection_round_trips_unchanged() {
    let collection: Value = serde_json::from_str(COLLECTION).unwrap();
    let (api, seen) = mock(vec![
        ("/api/addonCollectionGet", json!({ "result": collection })),
        (
            "/api/addonCollectionSet",
            json!({ "result": { "success": true } }),
        ),
    ])
    .await;
    let got = api.addon_collection_get(&key()).await.unwrap();
    assert_eq!(got.addons.len(), 3, "the malformed entry is skipped");
    api.addon_collection_set(&key(), &got.addons).await.unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen[0].1,
        json!({ "type": "AddonCollectionGet", "authKey": "test-auth-key", "update": true })
    );
    // what goes back is exactly what came in (minus the unreadable entry)
    let sent = &seen[1].1;
    assert_eq!(sent["type"], "AddonCollectionSet");
    let mut expected = collection["addons"].as_array().unwrap().clone();
    expected.retain(|a| a.get("transportUrl").is_some());
    assert_eq!(sent["addons"], Value::Array(expected));
}

#[tokio::test]
async fn library_meta_get_and_put_use_the_datastore_shapes() {
    let library: Value = serde_json::from_str(LIBRARY).unwrap();
    let (api, seen) = mock(vec![
        (
            "/api/datastoreMeta",
            json!({ "result": [["tt0111161", 1760000000000_i64], ["tt0068646", "2025-10-09T08:53:20.000Z"]] }),
        ),
        ("/api/datastoreGet", json!({ "result": library })),
        ("/api/datastorePut", json!({ "result": { "success": true } })),
    ])
    .await;
    let meta = api.datastore_meta(&key()).await.unwrap();
    assert_eq!(meta[0].1, 1_760_000_000_000);
    assert_eq!(meta[1].1, 1_760_000_000_000);
    let items = api.datastore_get(&key(), &[], true).await.unwrap();
    assert_eq!(items.len(), 1, "the broken item is skipped");
    let many: Vec<_> = (0..120).map(|_| items[0].clone()).collect();
    api.datastore_put(&key(), &many).await.unwrap();

    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen[0].1,
        json!({ "authKey": "test-auth-key", "collection": "libraryItem" })
    );
    assert_eq!(
        seen[1].1,
        json!({ "authKey": "test-auth-key", "collection": "libraryItem", "ids": [], "all": true })
    );
    let puts: Vec<_> = seen
        .iter()
        .filter(|(p, _)| p == "/api/datastorePut")
        .collect();
    assert_eq!(puts.len(), 3, "50 per request");
    let first = &puts[0].1["changes"][0];
    // strict out: stremio-core's field names and integer times
    assert_eq!(first["_id"], "tt0111161");
    assert!(first["state"]["timeOffset"].is_u64());
    assert!(first["state"].get("video_id").is_some());
    assert_eq!(first["_mtime"], "2025-10-09T08:53:20Z");
}

#[tokio::test]
async fn errors_never_carry_the_url_or_the_key() {
    let (api, _) = mock(vec![]).await;
    let error = api.get_user(&key()).await.unwrap_err().to_string();
    assert!(
        !error.contains("test-auth-key") && !error.contains("127.0.0.1"),
        "{error}"
    );
}
