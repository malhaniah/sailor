//! An in-memory cache of catalog pages, so switching rooms or coming back to Home
//! doesn't ask every addon again. Keyed by the resource URL (which holds the extras),
//! kept for [`TTL`], at most [`CAPACITY`] pages; the oldest page goes first. Nothing is
//! written to disk: a restart starts fresh.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use url::Url;
use zeiler_addon::CatalogResponse;

pub const TTL: Duration = Duration::from_secs(15 * 60);
pub const CAPACITY: usize = 256;

#[derive(Default)]
pub struct CatalogCache {
    pages: Mutex<HashMap<Url, (Instant, CatalogResponse)>>,
}

impl CatalogCache {
    /// The cached page for `url`, while it is fresh.
    pub fn get(&self, url: &Url, now: Instant) -> Option<CatalogResponse> {
        let pages = self.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages
            .get(url)
            .filter(|(at, _)| now.saturating_duration_since(*at) < TTL)
            .map(|(_, page)| page.clone())
    }

    pub fn put(&self, url: Url, page: CatalogResponse, now: Instant) {
        let mut pages = self.pages.lock().unwrap_or_else(|e| e.into_inner());
        pages.retain(|_, (at, _)| now.saturating_duration_since(*at) < TTL);
        if !pages.contains_key(&url)
            && pages.len() >= CAPACITY
            && let Some(oldest) = pages
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(url, _)| url.clone())
        {
            pages.remove(&oldest);
        }
        pages.insert(url, (now, page));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: usize) -> CatalogResponse {
        serde_json::from_value(serde_json::json!({
            "metas": (0..n).map(|i| serde_json::json!({"id": format!("tt{i}"), "type": "movie", "name": "x"})).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    fn url(i: usize) -> Url {
        Url::parse(&format!(
            "https://a.example/catalog/movie/top/skip={i}.json"
        ))
        .unwrap()
    }

    #[test]
    fn a_page_is_served_until_it_expires() {
        let cache = CatalogCache::default();
        let start = Instant::now();
        cache.put(url(0), page(3), start);
        assert_eq!(
            cache
                .get(&url(0), start + Duration::from_secs(60))
                .unwrap()
                .metas
                .len(),
            3
        );
        assert!(cache.get(&url(1), start).is_none());
        assert!(cache.get(&url(0), start + TTL).is_none());
    }

    #[test]
    fn the_oldest_page_makes_room() {
        let cache = CatalogCache::default();
        let start = Instant::now();
        for i in 0..CAPACITY {
            cache.put(url(i), page(1), start + Duration::from_millis(i as u64));
        }
        let later = start + Duration::from_secs(1);
        cache.put(url(CAPACITY), page(1), later);
        assert!(cache.get(&url(0), later).is_none(), "oldest evicted");
        assert!(cache.get(&url(1), later).is_some());
        assert!(cache.get(&url(CAPACITY), later).is_some());
        assert_eq!(cache.pages.lock().unwrap().len(), CAPACITY);
    }
}
