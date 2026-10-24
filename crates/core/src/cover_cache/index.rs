//! Byte-capped LRU bookkeeping for cached cover files. No file IO here.

use std::collections::{BTreeMap, HashMap};

/// One cover size of one item of one account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot {
    pub account_id: String,
    pub item_id: String,
    pub width: u32,
}

#[derive(Debug, Clone)]
struct Entry {
    etag: String,
    size: u64,
    tick: u64,
}

/// What changed on [`Index::insert`]: files to delete.
#[derive(Debug, Default, PartialEq)]
pub struct Removed {
    pub files: Vec<(Slot, String)>,
}

#[derive(Debug, Default)]
pub struct Index {
    entries: HashMap<Slot, Entry>,
    order: BTreeMap<u64, Slot>,
    total: u64,
    next_tick: u64,
    capacity: u64,
}

impl Index {
    pub fn new(capacity: u64) -> Self {
        Self {
            capacity,
            ..Self::default()
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.total
    }

    /// Marks the slot as used if it holds `etag`. False on a miss or a stale etag.
    pub fn touch(&mut self, slot: &Slot, etag: &str) -> bool {
        let tick = self.tick();
        let Some(entry) = self.entries.get_mut(slot) else {
            return false;
        };
        if entry.etag != etag {
            return false;
        }
        self.order.remove(&entry.tick);
        entry.tick = tick;
        self.order.insert(tick, slot.clone());
        true
    }

    /// Records a new file, replacing the slot's old one, then evicts down to capacity.
    /// The new file itself is never evicted.
    pub fn insert(&mut self, slot: Slot, etag: &str, size: u64) -> Removed {
        let mut removed = Removed::default();
        if let Some(old) = self.remove(&slot)
            && old != etag
        {
            removed.files.push((slot.clone(), old));
        }
        let tick = self.tick();
        self.order.insert(tick, slot.clone());
        let entry = Entry {
            etag: etag.to_owned(),
            size,
            tick,
        };
        self.entries.insert(slot, entry);
        self.total += size;
        removed.files.extend(self.evict(tick));
        removed
    }

    /// Changes the capacity and returns the evicted files.
    pub fn set_capacity(&mut self, capacity: u64) -> Removed {
        self.capacity = capacity;
        Removed {
            files: self.evict(u64::MAX),
        }
    }

    /// Forgets every slot of an account.
    pub fn remove_account(&mut self, account_id: &str) {
        let slots: Vec<_> = self
            .entries
            .keys()
            .filter(|s| s.account_id == account_id)
            .cloned()
            .collect();
        for slot in slots {
            self.remove(&slot);
        }
    }

    /// Forgets a slot and returns its etag.
    pub fn remove(&mut self, slot: &Slot) -> Option<String> {
        let entry = self.entries.remove(slot)?;
        self.order.remove(&entry.tick);
        self.total -= entry.size;
        Some(entry.etag)
    }

    fn evict(&mut self, keep_tick: u64) -> Vec<(Slot, String)> {
        let mut evicted = Vec::new();
        while self.total > self.capacity {
            let Some((&tick, _)) = self.order.first_key_value() else {
                break;
            };
            if tick == keep_tick {
                break;
            }
            let slot = self.order[&tick].clone();
            if let Some(etag) = self.remove(&slot) {
                evicted.push((slot, etag));
            }
        }
        evicted
    }

    fn tick(&mut self) -> u64 {
        self.next_tick += 1;
        self.next_tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(item: &str) -> Slot {
        Slot {
            account_id: "a".into(),
            item_id: item.into(),
            width: 400,
        }
    }

    #[test]
    fn evicts_least_recently_used_first() {
        let mut index = Index::new(250);
        index.insert(slot("1"), "e", 100);
        index.insert(slot("2"), "e", 100);
        assert!(index.touch(&slot("1"), "e"));
        let removed = index.insert(slot("3"), "e", 100);
        assert_eq!(removed.files, [(slot("2"), "e".to_owned())]);
        assert_eq!(index.total_bytes(), 200);
    }

    #[test]
    fn stale_etag_misses_and_is_replaced() {
        let mut index = Index::new(1000);
        index.insert(slot("1"), "old", 100);
        assert!(!index.touch(&slot("1"), "new"));
        let removed = index.insert(slot("1"), "new", 120);
        assert_eq!(removed.files, [(slot("1"), "old".to_owned())]);
        assert_eq!(index.total_bytes(), 120);
        assert!(index.touch(&slot("1"), "new"));
    }

    #[test]
    fn oversized_entry_stays_and_capacity_shrinks() {
        let mut index = Index::new(50);
        assert!(index.insert(slot("1"), "e", 80).files.is_empty());
        index.insert(slot("2"), "e", 10);
        assert_eq!(index.total_bytes(), 10);
        let removed = index.set_capacity(0);
        assert_eq!(removed.files.len(), 1);
        assert_eq!(index.total_bytes(), 0);
    }
}
