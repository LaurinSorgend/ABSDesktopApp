//! Work matching over both Docker servers. Run with `cargo test -p abs-core --features integration`.
//!
//! Both servers scan the same media, so an item is the same work on both when its library
//! and folder are the same. The exceptions are set up by `docker/init_server.py`.
#![cfg(feature = "integration")]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{start, wait_for};

use abs_core::credentials::Credentials;
use abs_core::engine::{Command, Event};

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
    for (n, url) in SERVERS.iter().enumerate() {
        engine.send(Command::AddAccount {
            url: (*url).into(),
            username: "listener".into(),
            password: "listener-pass".into(),
        });
        wait_for(&events, |e| {
            matches!(e, Event::CatalogSynced { .. }).then_some(())
        });
        if n + 1 == SERVERS.len() {
            wait_for(&events, |e| {
                matches!(e, Event::WorksChanged(_)).then_some(())
            });
        }
    }
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
