//! Which member of a work to play, and which members can share its position.

use crate::db::ItemKey;

/// Setting key for the account to play from when nothing else decides.
pub const PREFERRED_ACCOUNT_SETTING: &str = "preferred_account";
/// Setting key: copy the played position to members with the same recording.
pub const MIRROR_PROGRESS_SETTING: &str = "mirror_progress";

/// Largest duration difference, in seconds, at which two members still share a position.
const MIRROR_TOLERANCE: f64 = 30.0;
/// Largest relative duration difference at which two members still share a position.
const MIRROR_RELATIVE: f64 = 0.005;

/// A member as the source choice sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberState {
    pub key: ItemKey,
    pub duration: Option<f64>,
    /// When the progress last changed; `None` when never started.
    pub progress_updated_at: Option<i64>,
    pub downloaded: bool,
    /// The account is connected now.
    pub connected: bool,
}

impl MemberState {
    fn playable(&self) -> bool {
        self.connected || self.downloaded
    }
}

/// The member with the most recent progress, else a downloaded one, else the work's
/// preferred account, else the global one, else the first. Members that can play right
/// now (connected or downloaded) come first; the rest only when none can.
pub fn pick<'m>(
    members: &'m [MemberState],
    work_preference: Option<&str>,
    global_preference: Option<&str>,
) -> Option<&'m MemberState> {
    let playable: Vec<&MemberState> = members.iter().filter(|m| m.playable()).collect();
    let pool = if playable.is_empty() {
        members.iter().collect()
    } else {
        playable
    };
    let by_account =
        |account: Option<&str>| account.and_then(|a| pool.iter().copied().find(|m| m.key.0 == a));
    pool.iter()
        .copied()
        .filter(|m| m.progress_updated_at.is_some())
        .max_by_key(|m| m.progress_updated_at)
        .or_else(|| pool.iter().copied().find(|m| m.downloaded))
        .or_else(|| by_account(work_preference))
        .or_else(|| by_account(global_preference))
        .or_else(|| pool.first().copied())
}

/// Other members whose duration is so close to the played one's that a position means
/// the same place: within 30 s or 0.5 %, whichever is larger.
pub fn mirror_targets<'m>(
    members: &'m [MemberState],
    played: &ItemKey,
    duration: f64,
) -> Vec<&'m MemberState> {
    members
        .iter()
        .filter(|m| m.key != *played && m.key.0 != played.0)
        .filter(|m| {
            m.duration.is_some_and(|d| {
                (d - duration).abs() <= MIRROR_TOLERANCE.max(duration * MIRROR_RELATIVE)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(account: &str, connected: bool) -> MemberState {
        MemberState {
            key: (account.into(), format!("{account}-item")),
            duration: Some(36_000.0),
            progress_updated_at: None,
            downloaded: false,
            connected,
        }
    }

    fn picked(members: &[MemberState], work: Option<&str>, global: Option<&str>) -> String {
        pick(members, work, global).unwrap().key.0.clone()
    }

    #[test]
    fn most_recent_progress_wins() {
        let mut members = vec![member("a", true), member("b", true), member("c", true)];
        members[0].progress_updated_at = Some(10);
        members[1].progress_updated_at = Some(20);
        members[2].downloaded = true;
        assert_eq!(picked(&members, Some("c"), Some("c")), "b");
    }

    #[test]
    fn then_a_download_then_the_preferences() {
        let mut members = vec![member("a", true), member("b", true), member("c", true)];
        assert_eq!(picked(&members, None, None), "a");
        assert_eq!(picked(&members, None, Some("c")), "c");
        assert_eq!(picked(&members, Some("b"), Some("c")), "b");
        members[2].downloaded = true;
        assert_eq!(picked(&members, Some("b"), None), "c");
    }

    #[test]
    fn members_that_can_play_come_first() {
        let mut members = vec![member("a", false), member("b", true)];
        members[0].progress_updated_at = Some(30);
        assert_eq!(picked(&members, None, None), "b");
        members[0].downloaded = true;
        assert_eq!(picked(&members, None, None), "a");
        let offline = vec![member("a", false), member("b", false)];
        assert_eq!(picked(&offline, Some("b"), None), "b");
        assert!(pick(&[], None, None).is_none());
    }

    #[test]
    fn mirrors_only_to_other_accounts_with_the_same_length() {
        let mut members = vec![
            member("a", true),
            member("b", true),
            member("c", true),
            member("d", true),
        ];
        members[1].duration = Some(36_150.0);
        members[2].duration = Some(36_200.0);
        members[3].duration = None;
        let targets = mirror_targets(&members, &members[0].key.clone(), 36_000.0);
        let accounts: Vec<&str> = targets.iter().map(|m| m.key.0.as_str()).collect();
        assert_eq!(accounts, ["b"]);
    }
}
