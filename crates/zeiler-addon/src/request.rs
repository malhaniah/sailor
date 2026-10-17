use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AddonError;

/// The path suffix every HTTP transport URL must end with.
pub const MANIFEST_PATH: &str = "/manifest.json";

/// JavaScript `encodeURIComponent` character set: every non-alphanumeric byte is
/// percent-encoded except `- _ . ! ~ * ' ( )`. This matches stremio-core's
/// URI_COMPONENT_ENCODE_SET, so Zeiler produces byte-identical resource URLs.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

pub fn encode_component(input: &str) -> String {
    utf8_percent_encode(input, URI_COMPONENT).to_string()
}

/// One `name=value` pair of a catalog/subtitles extra segment. Order matters and names
/// may repeat, so this is a list entry rather than a map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtraValue {
    pub name: String,
    pub value: String,
}

/// A `(resource, type, id, extra)` request path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePath {
    pub resource: String,
    pub r#type: String,
    pub id: String,
    pub extra: Vec<ExtraValue>,
}

impl ResourcePath {
    pub fn new(
        resource: impl Into<String>,
        r#type: impl Into<String>,
        id: impl Into<String>,
    ) -> Self {
        Self {
            resource: resource.into(),
            r#type: r#type.into(),
            id: id.into(),
            extra: Vec::new(),
        }
    }

    pub fn with_extra(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.push(ExtraValue {
            name: name.into(),
            value: value.into(),
        });
        self
    }
}

/// Only http(s) transports whose path ends in `/manifest.json` are supported.
pub fn validate_transport_url(url: &Url) -> Result<(), AddonError> {
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AddonError::InvalidTransportUrl(format!(
                "unsupported scheme '{other}' (only http and https addons are supported)"
            )));
        }
    }
    if !url.path().ends_with(MANIFEST_PATH) {
        return Err(AddonError::InvalidTransportUrl(format!(
            "addon URL must end with {MANIFEST_PATH}"
        )));
    }
    Ok(())
}

/// Normalize user-pasted input into a transport URL. Accepts `stremio://host/.../manifest.json`
/// (rewritten to https, keeping host:port, path and query) as well as plain http(s) URLs,
/// and the links people actually copy: an addon's configure page (`…/configure`,
/// `…/#/configure`), a trailing `/`, or the addon's base URL without `/manifest.json`
/// (appended). The rules follow Harbor's `parseAddonUrl` (src/lib/addon-store.ts, MIT).
pub fn normalize_transport_input(input: &str) -> Result<Url, AddonError> {
    let trimmed = input.trim();
    let rewritten = match trimmed.get(..10) {
        Some(scheme) if scheme.eq_ignore_ascii_case("stremio://") => {
            format!("https://{}", &trimmed[10..])
        }
        _ => trimmed.to_string(),
    };
    let mut url = Url::parse(&rewritten)
        .map_err(|e| AddonError::InvalidTransportUrl(format!("not a valid URL: {e}")))?;
    // check the scheme before touching the path, so the error names the real problem
    if !matches!(url.scheme(), "http" | "https") {
        validate_transport_url(&url)?;
    }
    // a configure page routed in the fragment (single-page apps): `#/configure`
    if let Some(fragment) = url.fragment() {
        let route = fragment.trim_matches('/');
        if route.is_empty() || route == "configure" {
            url.set_fragment(None);
        }
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    if let Some(base) = path.strip_suffix("/configure") {
        path = base.to_string();
    }
    if !path.ends_with(MANIFEST_PATH) {
        path.push_str(MANIFEST_PATH);
    }
    url.set_path(&path);
    validate_transport_url(&url)?;
    Ok(url)
}

/// Build a resource URL from a transport URL, exactly as the reference implementation does:
/// build `/{resource}/{type}/{id}[/{extra}].json` with each segment component-encoded, then
/// string-replace `/manifest.json` in the transport URL. The string replace (rather than URL
/// joining) preserves config path prefixes and any query string on the transport URL.
pub fn build_resource_url(transport: &Url, path: &ResourcePath) -> Result<Url, AddonError> {
    validate_transport_url(transport)?;
    let mut replacement = format!(
        "/{}/{}/{}",
        encode_component(&path.resource),
        encode_component(&path.r#type),
        encode_component(&path.id)
    );
    if !path.extra.is_empty() {
        let extra = path
            .extra
            .iter()
            .map(|e| {
                format!(
                    "{}={}",
                    encode_component(&e.name),
                    encode_component(&e.value)
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        replacement.push('/');
        replacement.push_str(&extra);
    }
    replacement.push_str(".json");
    let built = transport.as_str().replace(MANIFEST_PATH, &replacement);
    Url::parse(&built).map_err(|e| AddonError::Other(format!("built an invalid URL: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn plain_catalog() {
        let url = build_resource_url(
            &transport("https://addon.example.com/manifest.json"),
            &ResourcePath::new("catalog", "movie", "top"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/catalog/movie/top.json"
        );
    }

    #[test]
    fn catalog_with_skip_and_genre() {
        let url = build_resource_url(
            &transport("https://addon.example.com/manifest.json"),
            &ResourcePath::new("catalog", "movie", "top")
                .with_extra("genre", "Action")
                .with_extra("skip", "100"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/catalog/movie/top/genre=Action&skip=100.json"
        );
    }

    #[test]
    fn search_encodes_spaces_as_percent20() {
        let url = build_resource_url(
            &transport("https://addon.example.com/manifest.json"),
            &ResourcePath::new("catalog", "movie", "top").with_extra("search", "big buck bunny"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/catalog/movie/top/search=big%20buck%20bunny.json"
        );
    }

    #[test]
    fn series_episode_id_colon_is_encoded() {
        let url = build_resource_url(
            &transport("https://addon.example.com/manifest.json"),
            &ResourcePath::new("stream", "series", "tt0108778:1:1"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/stream/series/tt0108778%3A1%3A1.json"
        );
    }

    #[test]
    fn extra_values_with_reserved_chars_are_encoded() {
        let url = build_resource_url(
            &transport("https://addon.example.com/manifest.json"),
            &ResourcePath::new("subtitles", "series", "tt1:1:1")
                .with_extra("filename", "Show S01E01 & more=yes.mkv"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/subtitles/series/tt1%3A1%3A1/filename=Show%20S01E01%20%26%20more%3Dyes.mkv.json"
        );
    }

    #[test]
    fn config_prefix_in_transport_url_is_preserved() {
        let url = build_resource_url(
            &transport("https://addon.example.com/%7B%22k%22%3A%22v%22%7D/manifest.json"),
            &ResourcePath::new("stream", "movie", "tt1254207"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/%7B%22k%22%3A%22v%22%7D/stream/movie/tt1254207.json"
        );
    }

    #[test]
    fn query_string_on_transport_url_is_preserved() {
        let url = build_resource_url(
            &transport("https://addon.example.com/manifest.json?key=secret"),
            &ResourcePath::new("meta", "movie", "tt1254207"),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://addon.example.com/meta/movie/tt1254207.json?key=secret"
        );
    }

    #[test]
    fn rejects_non_manifest_urls() {
        assert!(validate_transport_url(&transport("https://addon.example.com/addon")).is_err());
        assert!(
            validate_transport_url(&transport("ftp://addon.example.com/manifest.json")).is_err()
        );
    }

    #[test]
    fn normalizes_stremio_scheme_keeping_port_and_query() {
        let url =
            normalize_transport_input("stremio://127.0.0.1:7000/manifest.json?cfg=1").unwrap();
        assert_eq!(url.as_str(), "https://127.0.0.1:7000/manifest.json?cfg=1");
    }

    #[test]
    fn normalizes_the_links_people_copy() {
        let cases = [
            (
                "https://a.example/manifest.json",
                "https://a.example/manifest.json",
            ),
            (
                "  https://a.example/manifest.json\n",
                "https://a.example/manifest.json",
            ),
            (
                "https://a.example/configure",
                "https://a.example/manifest.json",
            ),
            (
                "https://a.example/configure/",
                "https://a.example/manifest.json",
            ),
            (
                "https://a.example/#/configure",
                "https://a.example/manifest.json",
            ),
            (
                "https://a.example/cfg/#/configure/",
                "https://a.example/cfg/manifest.json",
            ),
            (
                "https://a.example/abc%7B%7D/configure",
                "https://a.example/abc%7B%7D/manifest.json",
            ),
            ("https://a.example", "https://a.example/manifest.json"),
            ("https://a.example/", "https://a.example/manifest.json"),
            (
                "https://a.example/sub/path/",
                "https://a.example/sub/path/manifest.json",
            ),
            (
                "https://a.example/sub?key=1",
                "https://a.example/sub/manifest.json?key=1",
            ),
            (
                "STREMIO://a.example:8080/x/manifest.json",
                "https://a.example:8080/x/manifest.json",
            ),
            (
                "stremio://a.example/configure",
                "https://a.example/manifest.json",
            ),
        ];
        for (input, expected) in cases {
            let url = normalize_transport_input(input)
                .unwrap_or_else(|e| panic!("{input} should normalize: {e}"));
            assert_eq!(url.as_str(), expected, "{input}");
        }
    }

    #[test]
    fn normalizing_still_refuses_other_schemes_and_garbage() {
        for input in [
            "ftp://a.example/manifest.json",
            "file:///etc/passwd",
            "not a url",
            "",
        ] {
            assert!(
                matches!(
                    normalize_transport_input(input),
                    Err(AddonError::InvalidTransportUrl(_))
                ),
                "{input}"
            );
        }
        let err = normalize_transport_input("ftp://a.example/x").unwrap_err();
        assert!(err.to_string().contains("scheme"), "{err}");
    }

    #[test]
    fn unicode_is_utf8_percent_encoded() {
        assert_eq!(encode_component("día"), "d%C3%ADa");
        assert_eq!(
            encode_component("a_b-c.d!e~f*g'h(i)j"),
            "a_b-c.d!e~f*g'h(i)j"
        );
    }
}
