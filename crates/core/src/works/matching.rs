//! The matching cascade: which items of all accounts are the same work.

use std::collections::{BTreeSet, HashMap};

use super::normalize;

/// Smallest duration difference that still counts as the same recording, in seconds.
const MIN_TOLERANCE: f64 = 300.0;
/// Largest relative duration difference that still counts as the same recording.
const RELATIVE_TOLERANCE: f64 = 0.02;

/// An item as the cascade sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Candidate {
    pub account_id: String,
    pub item_id: String,
    pub server_id: String,
    pub title: String,
    pub title_sort: String,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub duration: Option<f64>,
    pub abridged: Option<bool>,
    /// `epub`, `pdf`, ... for items with an ebook.
    pub ebook_format: Option<String>,
}

/// Why an item is in its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchMethod {
    /// Alone in its work.
    Single,
    /// The same item on the same server, seen by another user.
    SameItem,
    Asin,
    Isbn,
    /// Normalised title and primary author, duration within tolerance.
    TitleAuthor,
    /// Merged by the user.
    Manual,
}

impl MatchMethod {
    /// How sure the match is, from 0 to 1.
    pub fn confidence(self) -> f64 {
        match self {
            Self::Single | Self::SameItem | Self::Manual => 1.0,
            Self::Asin => 0.95,
            Self::Isbn => 0.9,
            Self::TitleAuthor => 0.8,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::SameItem => "same_item",
            Self::Asin => "asin",
            Self::Isbn => "isbn",
            Self::TitleAuthor => "title_author",
            Self::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "same_item" => Self::SameItem,
            "asin" => Self::Asin,
            "isbn" => Self::Isbn,
            "title_author" => Self::TitleAuthor,
            "manual" => Self::Manual,
            _ => Self::Single,
        }
    }
}

/// A user decision about one item; it always beats the cascade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Override {
    /// The item stands alone; the cascade never matches it.
    Split { account_id: String, item_id: String },
    /// The item's work and the target's work are one.
    Merge {
        account_id: String,
        item_id: String,
        target_account_id: String,
        target_item_id: String,
    },
}

/// Groups candidates into works. Each group lists candidate indexes with their method,
/// in candidate order; groups are ordered by their first member.
pub fn group(candidates: &[Candidate], overrides: &[Override]) -> Vec<Vec<(usize, MatchMethod)>> {
    let mut sets = Sets::new(candidates.iter().map(Keys::of).collect());
    let split: Vec<bool> = split_flags(candidates, overrides);
    let usable = |i: &usize| !split[*i];
    for bucket in buckets(candidates, usable, |c| {
        Some(format!("{}\n{}", c.server_id, c.item_id))
    }) {
        sets.union_all(&bucket, MatchMethod::SameItem, false);
    }
    for bucket in buckets(candidates, usable, |c| {
        c.asin.as_deref().and_then(normalize::asin)
    }) {
        sets.union_all(&bucket, MatchMethod::Asin, false);
    }
    for bucket in buckets(candidates, usable, |c| {
        c.isbn.as_deref().and_then(normalize::isbn13)
    }) {
        sets.union_all(&bucket, MatchMethod::Isbn, true);
    }
    for bucket in buckets(candidates, usable, title_author_key) {
        match_durations(&mut sets, candidates, bucket);
    }
    apply_merges(&mut sets, candidates, overrides);
    sets.groups()
}

/// Never on title alone: without a primary author there is no key.
fn title_author_key(candidate: &Candidate) -> Option<String> {
    let title = normalize::title(&candidate.title, &candidate.title_sort);
    let author = candidate
        .author
        .as_deref()
        .and_then(normalize::primary_author)?;
    (!title.is_empty()).then(|| format!("{title}\n{author}"))
}

/// Within one title and author: items whose durations are close to the first of a run.
/// Items without a duration (ebooks) only match each other.
fn match_durations(sets: &mut Sets, candidates: &[Candidate], mut bucket: Vec<usize>) {
    let duration = |i: usize| candidates[i].duration.filter(|d| *d > 0.0);
    bucket.sort_by(|a, b| {
        duration(*a)
            .partial_cmp(&duration(*b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut anchor = bucket[0];
    for &next in &bucket[1..] {
        if close(duration(anchor), duration(next)) {
            sets.union(anchor, next, MatchMethod::TitleAuthor, true);
        } else {
            anchor = next;
        }
    }
}

fn close(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() <= MIN_TOLERANCE.max(a.max(b) * RELATIVE_TOLERANCE),
        _ => false,
    }
}

/// Candidate indexes sharing a key, for keys with more than one candidate.
fn buckets(
    candidates: &[Candidate],
    usable: impl Fn(&usize) -> bool,
    key: impl Fn(&Candidate) -> Option<String>,
) -> Vec<Vec<usize>> {
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    let mut order = Vec::new();
    for (i, candidate) in candidates.iter().enumerate().filter(|(i, _)| usable(i)) {
        if let Some(key) = key(candidate) {
            let entry = by_key.entry(key.clone()).or_default();
            if entry.is_empty() {
                order.push(key);
            }
            entry.push(i);
        }
    }
    order
        .into_iter()
        .filter_map(|key| by_key.remove(&key))
        .filter(|bucket| bucket.len() > 1)
        .collect()
}

fn split_flags(candidates: &[Candidate], overrides: &[Override]) -> Vec<bool> {
    candidates
        .iter()
        .map(|c| {
            overrides.iter().any(|o| {
                matches!(o, Override::Split { account_id, item_id }
                    if *account_id == c.account_id && *item_id == c.item_id)
            })
        })
        .collect()
}

fn apply_merges(sets: &mut Sets, candidates: &[Candidate], overrides: &[Override]) {
    let index: HashMap<(&str, &str), usize> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| ((c.account_id.as_str(), c.item_id.as_str()), i))
        .collect();
    for o in overrides {
        if let Override::Merge {
            account_id,
            item_id,
            target_account_id,
            target_item_id,
        } = o
            && let Some(&from) = index.get(&(account_id.as_str(), item_id.as_str()))
            && let Some(&to) = index.get(&(target_account_id.as_str(), target_item_id.as_str()))
        {
            sets.union(to, from, MatchMethod::Manual, false);
            sets.method[from] = Some(MatchMethod::Manual);
        }
    }
}

/// Values whose disagreement keeps two works apart: ASINs, ISBNs, narrators,
/// abridged or not, and ebook format.
#[derive(Debug, Default)]
struct Keys([BTreeSet<String>; 5]);

impl Keys {
    fn of(candidate: &Candidate) -> Self {
        let one = |value: Option<String>| value.into_iter().collect::<BTreeSet<_>>();
        let narrators = candidate.narrator.as_deref().map(normalize::names);
        Self([
            one(candidate.asin.as_deref().and_then(normalize::asin)),
            one(candidate.isbn.as_deref().and_then(normalize::isbn13)),
            narrators.unwrap_or_default().into_iter().collect(),
            one(candidate.abridged.map(|a| a.to_string())),
            one(candidate.ebook_format.as_deref().map(normalize::text)),
        ])
    }

    /// Both sides know a value of some kind and share none of it.
    fn conflicts(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(&other.0)
            .any(|(a, b)| !a.is_empty() && !b.is_empty() && a.is_disjoint(b))
    }

    fn absorb(&mut self, other: Self) {
        for (mine, theirs) in self.0.iter_mut().zip(other.0) {
            mine.extend(theirs);
        }
    }
}

/// Union-find over candidates, with the merged keys of each set at its root.
struct Sets {
    parent: Vec<usize>,
    keys: Vec<Keys>,
    method: Vec<Option<MatchMethod>>,
}

impl Sets {
    fn new(keys: Vec<Keys>) -> Self {
        Self {
            parent: (0..keys.len()).collect(),
            method: vec![None; keys.len()],
            keys,
        }
    }

    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut node = i;
        while self.parent[node] != root {
            let next = self.parent[node];
            self.parent[node] = root;
            node = next;
        }
        root
    }

    fn union_all(&mut self, members: &[usize], method: MatchMethod, checked: bool) {
        for &other in &members[1..] {
            self.union(members[0], other, method, checked);
        }
    }

    /// Joins the sets of `a` and `b`. With `checked`, sets whose keys conflict stay apart.
    fn union(&mut self, a: usize, b: usize, method: MatchMethod, checked: bool) -> bool {
        let (root_a, root_b) = (self.find(a), self.find(b));
        if root_a == root_b {
            return false;
        }
        if checked && self.keys[root_a].conflicts(&self.keys[root_b]) {
            return false;
        }
        let (keep, gone) = (root_a.min(root_b), root_a.max(root_b));
        self.parent[gone] = keep;
        let keys = std::mem::take(&mut self.keys[gone]);
        self.keys[keep].absorb(keys);
        for i in [a, b] {
            self.method[i].get_or_insert(method);
        }
        true
    }

    fn groups(mut self) -> Vec<Vec<(usize, MatchMethod)>> {
        let mut by_root: HashMap<usize, usize> = HashMap::new();
        let mut groups: Vec<Vec<(usize, MatchMethod)>> = Vec::new();
        for i in 0..self.parent.len() {
            let root = self.find(i);
            let slot = *by_root.entry(root).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[slot].push((i, self.method[i].unwrap_or(MatchMethod::Single)));
        }
        groups
    }
}

#[cfg(test)]
mod tests;
