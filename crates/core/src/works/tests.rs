use super::*;
use crate::db::{Db, ItemRow, LibraryRow, NewAccount};

const HOUR: f64 = 3600.0;

/// Two accounts on two servers, each with one book library, "home" added first.
fn two_accounts() -> (Db, String, String) {
    let mut db = Db::open_in_memory().unwrap();
    let ids = db
        .write(|store| {
            let mut ids = Vec::new();
            for name in ["home", "friend"] {
                let accounts = store.accounts();
                let server = accounts.add_server(name, &[])?;
                let account = accounts.add_account(&NewAccount {
                    server_id: &server,
                    user_id: "u",
                    username: name,
                })?;
                let library = LibraryRow {
                    remote_id: "lib".into(),
                    name: "Books".into(),
                    media_type: "book".into(),
                    display_order: 0,
                    raw_json: "{}".into(),
                };
                store.catalog().replace_libraries(&account.id, &[library])?;
                ids.push(account.id);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Ok(ids)
        })
        .unwrap();
    (db, ids[0].clone(), ids[1].clone())
}

fn book(id: &str, title: &str, duration: f64) -> ItemRow {
    ItemRow {
        remote_id: id.into(),
        library_id: "lib".into(),
        title: title.into(),
        title_sort: title.into(),
        author: Some("Jane Austen".into()),
        narrator: None,
        series: None,
        asin: None,
        isbn: None,
        duration: Some(duration),
        cover_etag: None,
        added_at: 0,
        updated_at: 0,
        raw_json: "{}".into(),
    }
}

fn add(db: &mut Db, account: &str, items: &[ItemRow]) -> WorksReport {
    db.write(|store| {
        store.catalog().upsert_items(account, items)?;
        rebuild(store)
    })
    .unwrap()
}

#[test]
fn rebuild_merges_across_accounts_and_keeps_ids() {
    let (mut db, home, friend) = two_accounts();
    add(
        &mut db,
        &home,
        &[
            book("h1", "Emma", 10.0 * HOUR),
            book("h2", "Persuasion", 8.0 * HOUR),
        ],
    );
    let emma = db.store().works().work_of(&home, "h1").unwrap().unwrap();
    let report = add(&mut db, &friend, &[book("f1", "Emma", 10.1 * HOUR)]);
    assert_eq!(
        report,
        WorksReport {
            works: 2,
            merged: 1
        }
    );
    let works = db.store().works();
    assert_eq!(
        works.work_of(&friend, "f1").unwrap(),
        Some(emma),
        "the work keeps its id"
    );
    let members = works.members(emma).unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].account_id, home);
    assert_eq!(members[1].method, MatchMethod::TitleAuthor);
    assert_eq!(
        works.work(emma).unwrap().unwrap().cover,
        Some((home.clone(), "h1".into()))
    );
}

#[test]
fn disabled_accounts_and_removed_items_leave_their_works() {
    let (mut db, home, friend) = two_accounts();
    add(&mut db, &home, &[book("h1", "Emma", 10.0 * HOUR)]);
    add(&mut db, &friend, &[book("f1", "Emma", 10.0 * HOUR)]);
    db.write(|store| {
        store.accounts().set_enabled(&friend, false)?;
        rebuild(store)
    })
    .unwrap();
    assert_eq!(db.store().works().work_of(&friend, "f1").unwrap(), None);
    assert_eq!(db.store().works().counts().unwrap(), (1, 0));
    db.write(|store| {
        store.catalog().remove_item(&home, "h1")?;
        rebuild(store)
    })
    .unwrap();
    assert_eq!(db.store().works().counts().unwrap(), (0, 0));
}

#[test]
fn overrides_survive_rebuilds() {
    let (mut db, home, friend) = two_accounts();
    add(&mut db, &home, &[book("h1", "Emma", 10.0 * HOUR)]);
    add(
        &mut db,
        &friend,
        &[
            book("f1", "Emma", 10.0 * HOUR),
            book("f2", "Emma (Dramatised)", 3.0 * HOUR),
        ],
    );
    let report = db
        .write(|store| {
            let works = store.works();
            works.split(&friend, "f1")?;
            works.merge((&friend, "f2"), (&home, "h1"))?;
            rebuild(store)
        })
        .unwrap();
    assert_eq!(
        report,
        WorksReport {
            works: 2,
            merged: 1
        }
    );
    let works = db.store().works();
    let emma = works.work_of(&home, "h1").unwrap();
    assert_eq!(works.work_of(&friend, "f2").unwrap(), emma);
    assert_ne!(works.work_of(&friend, "f1").unwrap(), emma);
    let report = add(&mut db, &friend, &[book("f1", "Emma", 10.0 * HOUR)]);
    assert_eq!(
        report.merged, 1,
        "a split item stays out after the next sync"
    );
}

#[test]
fn rebuild_of_a_thousand_items_is_quick() {
    let (mut db, home, friend) = two_accounts();
    let items: Vec<ItemRow> = (0..500)
        .map(|i| book(&format!("b{i}"), &format!("Book {i}"), HOUR + f64::from(i)))
        .collect();
    add(&mut db, &home, &items);
    let started = std::time::Instant::now();
    let report = add(&mut db, &friend, &items);
    assert_eq!(
        report,
        WorksReport {
            works: 500,
            merged: 500
        }
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn browse_merges_by_work_within_the_chosen_libraries() {
    use crate::db::{BrowseQuery, ItemSort, ProgressFilter, ProgressRow};
    let (mut db, home, friend) = two_accounts();
    add(
        &mut db,
        &home,
        &[
            book("h1", "Emma", 10.0 * HOUR),
            book("h2", "Persuasion", 8.0 * HOUR),
        ],
    );
    add(&mut db, &friend, &[book("f1", "Emma", 10.0 * HOUR)]);
    let started = ProgressRow {
        item_id: "f1".into(),
        current_time: 600.0,
        progress: 0.4,
        is_finished: false,
        ebook_location: None,
        ebook_progress: None,
        updated_at: 1,
    };
    db.store().progress().upsert(&friend, &started).unwrap();
    let both = [
        (home.clone(), "lib".to_owned()),
        (friend.clone(), "lib".to_owned()),
    ];
    let query = |libraries, merged, filter| BrowseQuery {
        libraries,
        merged,
        sort: ItemSort::Title,
        desc: false,
        filter,
    };
    let works = db.store().works();
    let merged = works
        .browse(&query(&both, true, ProgressFilter::All))
        .unwrap();
    let titles: Vec<(&str, &str, i64)> = merged
        .iter()
        .map(|r| (r.item.title.as_str(), r.item.account_id.as_str(), r.copies))
        .collect();
    assert_eq!(
        titles,
        [("Emma", home.as_str(), 2), ("Persuasion", home.as_str(), 1)]
    );
    assert_eq!(
        merged[0].item.progress, 0.4,
        "the furthest member's progress"
    );
    let copies = works
        .browse(&query(&both, false, ProgressFilter::All))
        .unwrap();
    assert_eq!(copies.len(), 3);
    let started = works
        .browse(&query(&both, true, ProgressFilter::InProgress))
        .unwrap();
    assert_eq!(started.len(), 1);
    let friend_only = [(friend.clone(), "lib".to_owned())];
    let only = works
        .browse(&query(&friend_only, true, ProgressFilter::All))
        .unwrap();
    assert_eq!(only.len(), 1);
    assert_eq!(
        (only[0].item.account_id.as_str(), only[0].copies),
        (friend.as_str(), 1)
    );
}
