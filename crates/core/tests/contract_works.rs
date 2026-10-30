//! Work matching over both Docker servers. Run with `cargo test -p abs-core --features integration`.
//!
//! Both servers scan the same media, so an item is the same work on both when its library
//! and folder are the same. The exceptions are set up by `docker/init_server.py`.
#![cfg(feature = "integration")]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use common::{start, wait_for};

use abs_core::credentials::Credentials;
use abs_core::db::Db;
use abs_core::engine::{Command, Engine, Event};
use abs_core::works::MIRROR_PROGRESS_SETTING;

const SERVERS: [&str; 2] = ["http://localhost:13378", "http://localhost:13379"];
/// Different narrators on the two servers: two works.
const PER_SERVER: &[&str] = &["Matching/Fixture Author/Two Narrators"];
/// Same work, but 2.26.3 cuts the title short, so nothing can match it.
const KNOWN_MISSES: &[&str] = &["Audiobooks/Oscar Wilde/The Happy Prince and Other Tales"];

#[test]
fn overlapping_servers_give_one_work_each_without_wrong_merges() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let (engine, events) = start(&db_path, &Credentials::memory());
    add_both(&engine, &events);
    engine.shutdown();

    let works = works_by_truth(&db_path);
    let wrong: Vec<_> = works
        .iter()
        .filter(|(_, truths)| truths.len() > 1)
        .collect();
    assert!(wrong.is_empty(), "wrong merges: {wrong:#?}");

    let mut works_of_truth: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    for (work, truths) in &works {
        for truth in truths {
            works_of_truth
                .entry(truth.clone())
                .or_default()
                .insert(*work);
        }
    }
    let missed: Vec<&String> = works_of_truth
        .iter()
        .filter(|(_, works)| works.len() > 1)
        .map(|(truth, _)| truth)
        .collect();
    for truth in &missed {
        eprintln!("missed merge: {truth}");
    }
    let unexpected: Vec<_> = missed
        .iter()
        .filter(|t| !KNOWN_MISSES.contains(&t.as_str()))
        .collect();
    assert!(unexpected.is_empty(), "missed merges: {unexpected:?}");
}

#[test]
fn mirrored_progress_reaches_the_other_server() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let db = Db::open(&db_path).unwrap();
    db.store()
        .settings()
        .set(MIRROR_PROGRESS_SETTING, true)
        .unwrap();
    drop(db);
    let (engine, events) = start(&db_path, &Credentials::memory());
    let accounts = add_both(&engine, &events);
    let items = shared_narration(&db_path);
    let position = 100.0 + f64::from(now_secs() % 300);

    engine.send(Command::StartPlayback {
        account_id: accounts[0].clone(),
        item_id: items[&accounts[0]].clone(),
    });
    wait_for(&events, |e| {
        matches!(e, Event::PlaybackStarted(_)).then_some(())
    });
    engine.send(Command::ReportPlayback {
        item_id: items[&accounts[0]].clone(),
        position,
        playing: false,
        force_sync: true,
    });
    let other_item = items[&accounts[1]].clone();
    let mirrored = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(wait_for_progress(SERVERS[1], &other_item, position));
    engine.shutdown();
    assert_eq!(mirrored, Some(position));
}

/// Adds `listener` on both servers and waits until the works include both. Returns the
/// account ids in server order.
fn add_both(engine: &Engine, events: &Receiver<Event>) -> Vec<String> {
    let mut accounts = Vec::new();
    for url in SERVERS {
        engine.send(Command::AddAccount {
            url: url.into(),
            username: "listener".into(),
            password: "listener-pass".into(),
        });
        let account_id = wait_for(events, |e| match e {
            Event::AccountAdded(account) => Some(account.id.clone()),
            _ => None,
        });
        wait_for(events, |e| match e {
            Event::CatalogSynced { account_id: id, .. } => (id == &account_id).then_some(()),
            _ => None,
        });
        accounts.push(account_id);
    }
    wait_for(events, |e| match e {
        Event::WorksChanged(report) => (report.merged > 0).then_some(()),
        _ => None,
    });
    accounts
}

/// The "Shared Narration" item of each account.
fn shared_narration(db_path: &std::path::Path) -> BTreeMap<String, String> {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT account_id, remote_id FROM item
             WHERE json_extract(raw_json, '$.relPath') = 'Fixture Author/Shared Narration'",
        )
        .unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

/// The listener's position on the item once it equals `expected`, or the last seen after 10 s.
async fn wait_for_progress(server: &str, item_id: &str, expected: f64) -> Option<f64> {
    let client = abs_api::AbsClient::connect(abs_api::http_client().unwrap(), server)
        .await
        .unwrap();
    client.login("listener", "listener-pass").await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = client
            .item_progress(item_id)
            .await
            .unwrap()
            .and_then(|p| p.current_time);
        if current == Some(expected) || Instant::now() > deadline {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn now_secs() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    (secs % 100_000) as u32
}

/// Each work id with the true identities of its members.
fn works_by_truth(db_path: &std::path::Path) -> BTreeMap<i64, BTreeSet<String>> {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT m.work_id, l.name, json_extract(i.raw_json, '$.relPath'), a.server_id
             FROM work_member m
             JOIN item i ON i.account_id = m.account_id AND i.remote_id = m.item_id
             JOIN library l ON l.account_id = i.account_id AND l.remote_id = i.library_id
             JOIN account a ON a.id = m.account_id",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .unwrap();
    let mut works: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        let (work, library, rel_path, server) = row.unwrap();
        let mut truth = format!("{library}/{}", rel_path.replace('\\', "/"));
        if PER_SERVER.contains(&truth.as_str()) {
            truth = format!("{truth} on {server}");
        }
        works.entry(work).or_default().insert(truth);
    }
    assert!(works.len() > 500, "{} works", works.len());
    works
}
