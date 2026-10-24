//! A least-recently-used map capped by total byte size.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

pub struct ByteLru<K, V> {
    capacity: usize,
    used: usize,
    entries: HashMap<K, (V, usize)>,
    order: VecDeque<K>,
}

impl<K: Hash + Eq + Clone, V> ByteLru<K, V> {
    /// An empty cache holding at most `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// The value, marked as recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if !self.entries.contains_key(key) {
            return None;
        }
        self.order.retain(|k| k != key);
        self.order.push_back(key.clone());
        self.entries.get(key).map(|(value, _)| value)
    }

    /// Inserts a value and returns the keys evicted to make room.
    pub fn insert(&mut self, key: K, value: V, bytes: usize) -> Vec<K> {
        if let Some((_, old)) = self.entries.remove(&key) {
            self.used -= old;
            self.order.retain(|k| *k != key);
        }
        self.entries.insert(key.clone(), (value, bytes));
        self.order.push_back(key);
        self.used += bytes;
        self.evict()
    }

    fn evict(&mut self) -> Vec<K> {
        let mut evicted = Vec::new();
        while self.used > self.capacity && self.order.len() > 1 {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if let Some((_, bytes)) = self.entries.remove(&key) {
                self.used -= bytes;
                evicted.push(key);
            }
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::ByteLru;

    #[test]
    fn evicts_least_recently_used_by_bytes() {
        let mut lru = ByteLru::new(100);
        lru.insert(1, (), 40);
        lru.insert(2, (), 40);
        lru.get(&1);
        assert_eq!(lru.insert(3, (), 40), vec![2]);
        assert!(lru.get(&1).is_some());
    }
}
