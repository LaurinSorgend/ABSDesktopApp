//! The work layer: items of all accounts that are the same book, as one work.
//!
//! [`rebuild`] runs the matching cascade over every book item of the enabled accounts
//! and stores the result. Works keep their id as long as most of their members stay.

mod matching;
pub mod normalize;
mod source;

use std::collections::{HashMap, HashSet};

pub use matching::{Candidate, MatchMethod, Override, group};
pub use source::{
    MIRROR_PROGRESS_SETTING, MemberState, PREFERRED_ACCOUNT_SETTING, mirror_targets, pick,
};

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

/// The member to play: see [`pick`]. `connected` says which accounts are online.
pub fn playback_source(
    store: &Store<'_>,
    work_id: i64,
    connected: impl Fn(&str) -> bool,
) -> DbResult<Option<ItemKey>> {
    let Some(work) = store.works().work(work_id)? else {
        return Ok(None);
    };
    let members = member_states(store, work_id, connected)?;
    let global = store.settings().get(PREFERRED_ACCOUNT_SETTING)?;
    let chosen = pick(
        &members,
        work.preferred_account_id.as_deref(),
        global.as_deref(),
    );
    Ok(chosen.map(|m| m.key.clone()))
}

/// Members of the played item's work on other accounts with the same recording length,
/// when the mirror setting is on.
pub fn mirror_keys(store: &Store<'_>, played: &ItemKey, duration: f64) -> DbResult<Vec<ItemKey>> {
    if !store.settings().get_or(MIRROR_PROGRESS_SETTING, false)? {
        return Ok(Vec::new());
    }
    let Some(work_id) = store.works().work_of(&played.0, &played.1)? else {
        return Ok(Vec::new());
    };
    let members = store.works().member_states(work_id)?;
    Ok(mirror_targets(&members, played, duration)
        .into_iter()
        .map(|m| m.key.clone())
        .collect())
}

fn member_states(
    store: &Store<'_>,
    work_id: i64,
    connected: impl Fn(&str) -> bool,
) -> DbResult<Vec<MemberState>> {
    let mut members = store.works().member_states(work_id)?;
    for member in &mut members {
        member.connected = connected(&member.key.0);
    }
    Ok(members)
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
