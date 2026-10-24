use std::collections::{HashMap, VecDeque};

/// Least-recently-used cache that evicts by total byte size, not entry count.
pub struct ByteLru<V> {
    capacity: usize,
    used: usize,
    entries: HashMap<usize, (V, usize)>,
    order: VecDeque<usize>,
}

impl<V> ByteLru<V> {
    /// Creates an empty cache holding at most `capacity` bytes.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            used: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Bytes currently held.
    pub fn used(&self) -> usize {
        self.used
    }

    /// Returns whether `key` is cached and marks it as recently used.
    pub fn touch(&mut self, key: usize) -> bool {
        if !self.entries.contains_key(&key) {
            return false;
        }
        self.order.retain(|k| *k != key);
        self.order.push_back(key);
        true
    }

    /// Returns the cached value without changing its recency.
    pub fn peek(&self, key: usize) -> Option<&V> {
        self.entries.get(&key).map(|(value, _)| value)
    }

    /// Inserts a value and returns the keys evicted to make room.
    pub fn insert(&mut self, key: usize, value: V, bytes: usize) -> Vec<usize> {
        if let Some((_, old)) = self.entries.remove(&key) {
            self.used -= old;
            self.order.retain(|k| *k != key);
        }
        self.entries.insert(key, (value, bytes));
        self.order.push_back(key);
        self.used += bytes;
        self.evict()
    }

    fn evict(&mut self) -> Vec<usize> {
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
        lru.touch(1);
        let evicted = lru.insert(3, (), 40);
        assert_eq!(evicted, vec![2]);
        assert_eq!(lru.used(), 80);
    }
}
