//! The work page: the copies of one book across accounts, with merge, split and the
//! account to play from.

use abs_core::db::{ItemKey, ItemSummary, WorkMember};
use abs_core::engine::Command;
use abs_core::works::{self, MatchMethod};
use slint::{Image, ModelRc, VecModel};

use super::App;
use super::detail::duration_text;
use crate::covers::LARGE_WIDTH;
use crate::{MatchData, MemberData, Page, Strings};

const MATCH_LIMIT: u64 = 30;

/// Which work the page shows. It is followed through one of its members, because a
/// merge or split can give the work another id.
#[derive(Default)]
pub struct WorkView {
    member: Option<ItemKey>,
    work_id: Option<i64>,
    /// Accounts of the members, in the order of the account choice after "general choice".
    accounts: Vec<String>,
    cover: Option<ItemKey>,
}

impl App {
    pub fn open_work(&mut self, work_id: i64) {
        let first = self.db.store().works().members(work_id).unwrap_or_default();
        let Some(first) = first.into_iter().next() else {
            return;
        };
        self.work.member = Some((first.account_id, first.item_id));
        self.ui(|ui, _| {
            ui.set_work_match_text("".into());
            ui.set_work_match_results(ModelRc::default());
        });
        self.refresh_work();
        self.navigate(Page::Work);
    }

    /// Fills the page again, as after the works were matched again.
    pub(super) fn refresh_work(&mut self) {
        let Some((account_id, item_id)) = self.work.member.clone() else {
            return;
        };
        let works = self.db.store().works();
        let Some(work_id) = works.work_of(&account_id, &item_id).ok().flatten() else {
            return;
        };
        let Some(work) = works.work(work_id).ok().flatten() else {
            return;
        };
        let members = works.members(work_id).unwrap_or_default();
        self.work.work_id = Some(work_id);
        self.work.accounts = members.iter().map(|m| m.account_id.clone()).collect();
        self.work.accounts.dedup();
        let rows: Vec<MemberData> = members
            .iter()
            .filter_map(|m| self.member_row(m, members.len()))
            .collect();
        let preferred = work
            .preferred_account_id
            .as_deref()
            .and_then(|a| self.work.accounts.iter().position(|x| x == a))
            .map_or(0, |i| i as i32 + 1);
        let options = self.work_account_options();
        let play_note = self.play_note(work_id);
        self.ui(|ui, _| {
            ui.set_work_id(work_id as i32);
            ui.set_work_title(work.title.as_str().into());
            ui.set_work_author(work.author.clone().unwrap_or_default().into());
            ui.set_work_members(ModelRc::new(VecModel::from(rows)));
            ui.set_work_account_options(ModelRc::new(VecModel::from(options)));
            ui.set_work_account_index(preferred);
            ui.set_work_play_note(play_note.into());
        });
        self.load_work_cover(work.cover);
    }

    fn load_work_cover(&mut self, cover: Option<ItemKey>) {
        self.work.cover = cover.clone();
        let image = cover.and_then(|(account_id, item_id)| {
            let etag = self.summary(&account_id, &item_id)?.cover_etag;
            self.covers
                .want(&account_id, &item_id, etag.as_deref(), LARGE_WIDTH)
        });
        self.ui(|ui, _| {
            ui.set_work_has_cover(image.is_some());
            ui.set_work_cover(image.unwrap_or_default());
        });
    }

    pub(super) fn work_cover(&self, item_id: &str, image: Option<&Image>) {
        let shows = self.work.cover.as_ref().is_some_and(|(_, i)| i == item_id);
        if let (true, Some(image)) = (shows, image) {
            self.ui(|ui, _| {
                ui.set_work_cover(image.clone());
                ui.set_work_has_cover(true);
            });
        }
    }

    fn member_row(&self, member: &WorkMember, count: usize) -> Option<MemberData> {
        let item = self.summary(&member.account_id, &member.item_id)?;
        let account = self.accounts.iter().find(|a| a.id == member.account_id)?;
        let library = self.library_name(&member.account_id, &item.library_id);
        let decision = self
            .db
            .store()
            .works()
            .override_of(&member.account_id, &member.item_id)
            .ok()
            .flatten();
        let progress = self
            .db
            .store()
            .progress()
            .get(&member.account_id, &member.item_id)
            .ok()
            .flatten();
        let place = format!("{} · {library}", self.account_place(account));
        self.ui(|_, s| MemberData {
            account_id: member.account_id.as_str().into(),
            item_id: member.item_id.as_str().into(),
            title: item.title.as_str().into(),
            place: place.into(),
            narrator: item.narrator.clone().unwrap_or_default().into(),
            duration_text: duration_text(item.duration.unwrap_or_default(), s).into(),
            progress_text: member_progress(progress.as_ref(), s).into(),
            match_text: match_text(member.method, decision.as_deref(), s).into(),
            can_split: count > 1,
            can_reset: decision.is_some(),
        })
    }

    fn work_account_options(&self) -> Vec<slint::SharedString> {
        let general = self
            .ui(|_, s| s.invoke_general_choice())
            .unwrap_or_default();
        let mut options = vec![general];
        for id in &self.work.accounts {
            if let Some(account) = self.accounts.iter().find(|a| &a.id == id) {
                options.push(self.account_place(account).into());
            }
        }
        options
    }

    fn play_note(&self, work_id: i64) -> String {
        let source =
            works::playback_source(&self.db.store(), work_id, |a| self.is_account_connected(a));
        let Some((account_id, _)) = source.ok().flatten() else {
            return String::new();
        };
        let Some(account) = self.accounts.iter().find(|a| a.id == account_id) else {
            return String::new();
        };
        let place = self.account_place(account);
        self.ui(|_, s| s.invoke_plays_from(place.into()).into())
            .unwrap_or_default()
    }

    /// Plays the copy the source rules pick.
    pub fn play_work(&mut self) {
        let Some(work_id) = self.work.work_id else {
            return;
        };
        let source =
            works::playback_source(&self.db.store(), work_id, |a| self.is_account_connected(a));
        if let Some((account_id, item_id)) = source.ok().flatten() {
            self.play_account_item(&account_id, &item_id, None);
        }
    }

    pub fn set_work_account(&mut self, index: usize) {
        let Some(work_id) = self.work.work_id else {
            return;
        };
        let account_id = index
            .checked_sub(1)
            .and_then(|i| self.work.accounts.get(i).cloned());
        self.engine.send(Command::SetPreferredAccount {
            work_id,
            account_id,
        });
    }

    pub fn split_member(&mut self, account_id: &str, item_id: &str) {
        let splitting_followed = self
            .work
            .member
            .as_ref()
            .is_some_and(|(a, i)| a == account_id && i == item_id);
        if splitting_followed {
            self.follow_other_member(account_id, item_id);
        }
        self.engine.send(Command::SplitFromWork {
            account_id: account_id.to_owned(),
            item_id: item_id.to_owned(),
        });
    }

    fn follow_other_member(&mut self, account_id: &str, item_id: &str) {
        let Some(work_id) = self.work.work_id else {
            return;
        };
        let members = self.db.store().works().members(work_id).unwrap_or_default();
        self.work.member = members
            .into_iter()
            .find(|m| !(m.account_id == account_id && m.item_id == item_id))
            .map(|m| (m.account_id, m.item_id));
    }

    pub fn reset_member(&mut self, account_id: &str, item_id: &str) {
        self.engine.send(Command::ResetWorkMatch {
            account_id: account_id.to_owned(),
            item_id: item_id.to_owned(),
        });
    }

    /// Books of all accounts matching `text` that are not in this work.
    pub fn search_work_match(&mut self, text: &str) {
        let work_id = self.work.work_id;
        let store = self.db.store();
        let found = store
            .catalog()
            .search(None, text, MATCH_LIMIT)
            .unwrap_or_default();
        let rows: Vec<MatchData> = found
            .into_iter()
            .filter(|item| {
                let theirs = store
                    .works()
                    .work_of(&item.account_id, &item.remote_id)
                    .ok()
                    .flatten();
                theirs != work_id
            })
            .filter_map(|item| self.match_row(item))
            .collect();
        self.ui(|ui, _| ui.set_work_match_results(ModelRc::new(VecModel::from(rows))));
    }

    fn match_row(&self, item: ItemSummary) -> Option<MatchData> {
        let account = self.accounts.iter().find(|a| a.id == item.account_id)?;
        let library = self.library_name(&item.account_id, &item.library_id);
        Some(MatchData {
            account_id: item.account_id.as_str().into(),
            item_id: item.remote_id.as_str().into(),
            title: match &item.author {
                Some(author) => format!("{} · {author}", item.title).into(),
                None => item.title.as_str().into(),
            },
            place: format!("{} · {library}", self.account_place(account)).into(),
        })
    }

    /// Puts the found book's work and this work together.
    pub fn merge_into_work(&mut self, account_id: &str, item_id: &str) {
        let Some((target_account_id, target_item_id)) = self.work.member.clone() else {
            return;
        };
        self.engine.send(Command::MergeWorks {
            account_id: account_id.to_owned(),
            item_id: item_id.to_owned(),
            target_account_id,
            target_item_id,
        });
        self.ui(|ui, _| {
            ui.set_work_match_text("".into());
            ui.set_work_match_results(ModelRc::default());
        });
    }

    /// The works were matched again: update the pages that show them.
    pub(super) fn works_changed(&mut self) {
        self.refresh_works();
        if self.ui(|ui, _| ui.get_page()) == Some(Page::Work) {
            self.refresh_work();
        }
        self.refresh_detail_work();
    }

    fn summary(&self, account_id: &str, item_id: &str) -> Option<ItemSummary> {
        self.db
            .store()
            .catalog()
            .items_by_ids(account_id, &[item_id])
            .ok()?
            .into_iter()
            .next()
    }

    fn library_name(&self, account_id: &str, library_id: &str) -> String {
        self.db
            .store()
            .catalog()
            .libraries(account_id)
            .unwrap_or_default()
            .into_iter()
            .find(|l| l.remote_id == library_id)
            .map(|l| l.name)
            .unwrap_or_default()
    }
}

fn match_text(method: MatchMethod, decision: Option<&str>, s: &Strings<'_>) -> String {
    let text = match (decision, method) {
        (Some("split"), _) => s.invoke_match_split(),
        (_, MatchMethod::SameItem) => s.invoke_match_same_item(),
        (_, MatchMethod::Asin) => s.invoke_match_asin(),
        (_, MatchMethod::Isbn) => s.invoke_match_isbn(),
        (_, MatchMethod::TitleAuthor) => s.invoke_match_title(),
        (_, MatchMethod::Manual) => s.invoke_match_manual(),
        (_, MatchMethod::Single) => "".into(),
    };
    text.into()
}

fn member_progress(progress: Option<&abs_core::db::ProgressRow>, s: &Strings<'_>) -> String {
    match progress {
        Some(p) if p.is_finished => s.invoke_finished().into(),
        Some(p) if p.progress > 0.0 => format!("{} %", (p.progress * 100.0).round()),
        _ => s.invoke_not_started().into(),
    }
}
