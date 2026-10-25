//! The work layer: items of all accounts that are the same book, as one work.
//!
//! [`rebuild`] runs the matching cascade over every book item of the enabled accounts
//! and stores the result. Works keep their id as long as most of their members stay.

mod matching;
pub mod normalize;

use std::collections::{HashMap, HashSet};

pub use matching::{Candidate, MatchMethod, Override, group};

use crate::db::{DbResult, ItemKey, NewWork, Store};

/// What a rebuild found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorksReport {
    pub works: usize,
    /// Works with more than one member.
    pub merged: usize,
}

/// Matches all items again and replaces the stored works. Run it inside one transaction.
pub fn rebuild(store: &Store<'_>) -> DbResult<WorksReport> {
    let works = store.works();
    let candidates = works.candidates()?;
    let overrides = works.overrides()?;
    let groups = group(&candidates, &overrides);
    let previous = works.member_index()?;
    let assigned = assign_ids(&candidates, &groups, &previous);
    let new: Vec<NewWork> = groups
        .iter()
        .zip(assigned)
        .map(|(members, id)| new_work(&candidates, members, id))
        .collect();
    works.replace(&new)?;
    Ok(WorksReport {
        works: new.len(),
        merged: new.iter().filter(|w| w.members.len() > 1).count(),
    })
}

/// Each group keeps the old id most of its members had, unless a bigger group took it.
fn assign_ids(
    candidates: &[Candidate],
    groups: &[Vec<(usize, MatchMethod)>],
    previous: &HashMap<ItemKey, i64>,
) -> Vec<Option<i64>> {
    let votes: Vec<Vec<(i64, usize)>> = groups
        .iter()
        .map(|members| {
            let mut counts: HashMap<i64, usize> = HashMap::new();
            for (i, _) in members {
                if let Some(id) = previous.get(&key(&candidates[*i])) {
                    *counts.entry(*id).or_default() += 1;
                }
            }
            let mut counts: Vec<(i64, usize)> = counts.into_iter().collect();
            counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            counts
        })
        .collect();
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|g| std::cmp::Reverse(groups[*g].len()));
    let mut taken = HashSet::new();
    let mut ids = vec![None; groups.len()];
    for g in order {
        ids[g] = votes[g]
            .iter()
            .map(|(id, _)| *id)
            .find(|id| taken.insert(*id));
    }
    ids
}

fn new_work(
    candidates: &[Candidate],
    members: &[(usize, MatchMethod)],
    id: Option<i64>,
) -> NewWork {
    let first = &candidates[members[0].0];
    NewWork {
        id,
        title: first.title.clone(),
        author: first.author.clone(),
        cover: key(first),
        members: members
            .iter()
            .map(|(i, method)| (key(&candidates[*i]), *method))
            .collect(),
    }
}

fn key(candidate: &Candidate) -> ItemKey {
    (candidate.account_id.clone(), candidate.item_id.clone())
}

#[cfg(test)]
mod tests;
