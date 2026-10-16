//! A small least-recently-used map (a few hundred entries; linear bookkeeping is fine).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub(crate) struct Lru<K, V> {
    map: HashMap<K, V>,
    /// Least recently used first.
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V> Lru<K, V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        if self.map.contains_key(key) {
            self.touch(key);
        }
        self.map.get(key)
    }

    pub(crate) fn put(&mut self, key: K, value: V) {
        if self.map.insert(key.clone(), value).is_some() {
            self.touch(&key);
            return;
        }
        self.order.push_back(key);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
    }

    fn touch(&mut self, key: &K) {
        if let Some(at) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(at).expect("in range");
            self.order.push_back(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Lru;

    #[test]
    fn the_least_recently_used_entry_goes_first() {
        let mut lru = Lru::new(2);
        lru.put("a", 1);
        lru.put("b", 2);
        assert_eq!(lru.get(&"a"), Some(&1)); // a is now the most recent
        lru.put("c", 3);
        assert_eq!(lru.get(&"b"), None, "b was the least recently used");
        assert_eq!(lru.get(&"a"), Some(&1));
        assert_eq!(lru.get(&"c"), Some(&3));
        lru.put("a", 10); // replacing refreshes it too
        lru.put("d", 4);
        assert_eq!(lru.get(&"c"), None);
        assert_eq!(lru.get(&"a"), Some(&10));
    }
}
