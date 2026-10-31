//! Folders on this computer, from adding one to playing and reading its books. Only the last
//! test needs a server. Run with `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use abs_api::models::EbookLocation;
use abs_core::credentials::Credentials;
use abs_core::db::{Db, ItemQuery, ItemSummary, ProgressRow};
use abs_core::engine::{Command, Engine, Event};
use abs_core::local::ScanReport;
use abs_core::playback::PlayStart;
use common::{start, wait_for};

const SERVER: &str = "http://localhost:13379";
const MULTI_FILE: &str = "The Happy Prince and Other Tales";
const M4B: &str = "The Happy Prince Marathon";

fn media(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/media")
        .join(rel)
}

/// Every file below a folder with its size and time, to show the app changed nothing.
fn listing(root: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(listing(&path));
        } else {
            let metadata = entry.metadata().unwrap();
            found.push((path, metadata.len(), metadata.modified().unwrap()));
        }
    }
    found.sort();
    found
}

struct Setup {
    _dir: tempfile::TempDir,
    db_path: PathBuf,
    credentials: Credentials,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    Setup {
        _dir: dir,
        db_path,
        credentials: Credentials::memory(),
    }
}

/// Adds a folder and waits for its first scan. Returns the account id and the report.
fn add_folder(engine: &Engine, events: &Receiver<Event>, path: &Path) -> (String, ScanReport) {
    engine.send(Command::AddLocalFolder {
        path: path.to_owned(),
    });
    let account_id = wait_for(events, |e| match e {
        Event::AccountAdded(account) => {
            assert!(account.local);
            Some(account.id.clone())
        }
        _ => None,
    });
    let report = wait_for(events, |e| match e {
        Event::LocalScanFinished { report, .. } => Some(*report),
        _ => None,
    });
    (account_id, report)
}

fn items(db_path: &Path, account_id: &str) -> Vec<ItemSummary> {
    let db = Db::open(db_path).unwrap();
    let catalog = db.store().catalog();
    let mut items = Vec::new();
    for library in catalog.libraries(account_id).unwrap() {
        let query = ItemQuery::new(&library.remote_id);
        items.extend(catalog.items_page(account_id, &query, 0, 50).unwrap());
    }
    items
}

fn titled<'a>(items: &'a [ItemSummary], title: &str) -> &'a ItemSummary {
    items.iter().find(|i| i.title == title).unwrap_or_else(|| {
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        panic!("no {title} in {titles:?}")
    })
}

fn play(engine: &Engine, events: &Receiver<Event>, item: &ItemSummary) -> PlayStart {
    engine.send(Command::StartPlayback {
        account_id: item.account_id.clone(),
        item_id: item.remote_id.clone(),
    });
    wait_for(events, |e| match e {
        Event::PlaybackStarted(start) => Some((**start).clone()),
        Event::PlaybackFailed { message } => panic!("{message}"),
        _ => None,
    })
}

fn report_position(engine: &Engine, events: &Receiver<Event>, item_id: &str, position: f64) {
    engine.send(Command::ReportPlayback {
        item_id: item_id.to_owned(),
        position,
        playing: true,
        force_sync: true,
    });
    wait_for(events, |e| {
        matches!(e, Event::ProgressChanged { .. }).then_some(())
    });
}

fn progress(db_path: &Path, account_id: &str, item_id: &str) -> ProgressRow {
    let db = Db::open(db_path).unwrap();
    let row = db.store().progress().get(account_id, item_id).unwrap();
    row.expect("progress stored")
}

#[test]
fn audiobooks_play_from_their_files_and_keep_the_position() {
    let setup = setup();
    let folder = media("audiobooks");
    let before = listing(&folder);
    let (engine, events) = start(&setup.db_path, &setup.credentials);
    let (account_id, report) = add_folder(&engine, &events, &folder);
    assert_eq!(report.added, 2);

    let books = items(&setup.db_path, &account_id);
    let multi = titled(&books, MULTI_FILE);
    assert_eq!(multi.author.as_deref(), Some("Oscar Wilde"));
    let started = play(&engine, &events, multi);
    assert!(started.on_this_computer && started.local);
    assert!(started.http_headers.is_empty());
    assert_eq!(started.tracks.len(), 5);
    for (index, track) in started.tracks.iter().enumerate() {
        assert!(Path::new(&track.url).is_file(), "{}", track.url);
        assert!(
            track.url.contains(&format!("_0{}_", index + 1)),
            "{}",
            track.url
        );
    }
    let offsets: f64 = started.tracks[..4].iter().map(|t| t.duration).sum();
    assert!((started.tracks[4].start_offset - offsets).abs() < 0.001);
    assert_eq!(started.chapters.len(), 5);
    assert!(started.duration > 6000.0, "{}", started.duration);

    let m4b = titled(&books, M4B);
    let whole = play(&engine, &events, m4b);
    assert_eq!(whole.tracks.len(), 1);
    assert_eq!(whole.chapters.len(), 80);

    report_position(&engine, &events, &m4b.remote_id, 1234.0);
    engine.send(Command::StopPlayback);
    engine.shutdown();

    let (engine, events) = start(&setup.db_path, &setup.credentials);
    let report = wait_for(&events, |e| match e {
        Event::LocalScanFinished { report, .. } => Some(*report),
        _ => None,
    });
    assert_eq!(report, ScanReport::default(), "nothing changed on disk");
    let row = progress(&setup.db_path, &account_id, &m4b.remote_id);
    assert_eq!(row.current_time, 1234.0);
    assert!(!row.is_finished);
    assert_eq!(play(&engine, &events, m4b).start_time, 1234.0);
    engine.send(Command::StopPlayback);
    engine.shutdown();

    let db = Db::open(&setup.db_path).unwrap();
    let sessions = db.store().local_sessions().list(&account_id).unwrap();
    assert!(sessions.is_empty(), "nothing could send {sessions:?}");
    assert_eq!(listing(&folder), before, "the folder stays as it was");
}

#[test]
fn a_book_counts_as_finished_at_its_end() {
    let setup = setup();
    let (engine, events) = start(&setup.db_path, &setup.credentials);
    let (account_id, _) = add_folder(&engine, &events, &media("audiobooks"));
    let books = items(&setup.db_path, &account_id);
    let book = titled(&books, M4B);
    let started = play(&engine, &events, book);
    report_position(&engine, &events, &book.remote_id, started.duration - 5.0);
    engine.shutdown();
    let row = progress(&setup.db_path, &account_id, &book.remote_id);
    assert!(row.is_finished);
}

#[test]
fn ebooks_open_and_keep_the_reading_position() {
    let setup = setup();
    let (engine, events) = start(&setup.db_path, &setup.credentials);
    let (account_id, report) = add_folder(&engine, &events, &media("ebooks"));
    assert_eq!(report.added, 3);

    for item in items(&setup.db_path, &account_id) {
        engine.send(Command::OpenEbook {
            account_id: account_id.clone(),
            item_id: item.remote_id.clone(),
        });
        let open = wait_for(&events, |e| match e {
            Event::EbookReady(open) => Some(open.clone()),
            Event::EbookFailed { message, .. } => panic!("{message}"),
            _ => None,
        });
        assert!(open.path.is_file());
        assert!(["epub", "pdf", "cbz"].contains(&open.format.as_str()));
        assert_eq!(open.title, item.title);
    }

    let item = items(&setup.db_path, &account_id).remove(0);
    engine.send(Command::SaveEbookProgress {
        account_id: account_id.clone(),
        item_id: item.remote_id.clone(),
        location: EbookLocation::Page(7),
        progress: 0.25,
    });
    wait_for(&events, |e| {
        matches!(e, Event::ProgressChanged { .. }).then_some(())
    });
    engine.shutdown();
    let row = progress(&setup.db_path, &account_id, &item.remote_id);
    assert_eq!(row.ebook_location.as_deref(), Some("7"));
    let db = Db::open(&setup.db_path).unwrap();
    let queued = db.store().queue().peek(&account_id, 10).unwrap();
    assert!(queued.is_empty(), "nothing could send it");
}

#[test]
fn a_missing_file_gives_a_clear_error() {
    let setup = setup();
    let folder = tempfile::tempdir().unwrap();
    let book = folder.path().join("Author/Title");
    std::fs::create_dir_all(&book).unwrap();
    let audio = media(&format!(
        "audiobooks/Oscar Wilde/{MULTI_FILE}/thehappyprinceandothertales_01_wilde_64kb.mp3"
    ));
    std::fs::copy(audio, book.join("01.mp3")).unwrap();
    let comic = media("ebooks/Fixture Studio/Test Pattern Comic/Test Pattern Comic.cbz");
    std::fs::copy(comic, book.join("comic.cbz")).unwrap();

    let (engine, events) = start(&setup.db_path, &setup.credentials);
    let (account_id, _) = add_folder(&engine, &events, folder.path());
    let item = items(&setup.db_path, &account_id).remove(0);
    std::fs::remove_file(book.join("01.mp3")).unwrap();
    std::fs::remove_file(book.join("comic.cbz")).unwrap();

    engine.send(Command::StartPlayback {
        account_id: account_id.clone(),
        item_id: item.remote_id.clone(),
    });
    let message = wait_for(&events, |e| match e {
        Event::PlaybackFailed { message } => Some(message.clone()),
        _ => None,
    });
    assert!(message.contains("cannot be reached"), "{message}");
    engine.send(Command::OpenEbook {
        account_id,
        item_id: item.remote_id,
    });
    let message = wait_for(&events, |e| match e {
        Event::EbookFailed { message, .. } => Some(message.clone()),
        _ => None,
    });
    assert!(message.contains("cannot be reached"), "{message}");
}

#[test]
fn folders_come_and_go_and_their_files_stay() {
    let setup = setup();
    let folder = media("ebooks");
    let before = listing(&folder);
    let (engine, events) = start(&setup.db_path, &setup.credentials);
    let (account_id, _) = add_folder(&engine, &events, &folder);

    engine.send(Command::AddLocalFolder {
        path: folder.clone(),
    });
    let message = wait_for(&events, |e| match e {
        Event::RequestFailed { message, .. } => Some(message.clone()),
        _ => None,
    });
    assert!(message.contains("already"), "{message}");

    let library_id = {
        let db = Db::open(&setup.db_path).unwrap();
        db.store().local().folders().unwrap().remove(0).library_id
    };
    engine.send(Command::RemoveLocalFolder { library_id });
    wait_for(&events, |e| {
        matches!(e, Event::LocalFoldersChanged).then_some(())
    });
    assert!(items(&setup.db_path, &account_id).is_empty());
    assert_eq!(listing(&folder), before);

    engine.send(Command::RemoveAccount {
        account_id: account_id.clone(),
    });
    wait_for(&events, |e| {
        matches!(e, Event::AccountRemoved { .. }).then_some(())
    });
    let db = Db::open(&setup.db_path).unwrap();
    assert!(db.store().accounts().list().unwrap().is_empty());
}

/// A local book and the server's copy of it are one work.
#[test]
fn a_local_book_joins_the_servers_copy() {
    let setup = setup();
    let (engine, events) = start(&setup.db_path, &setup.credentials);
    engine.send(Command::AddAccount {
        url: SERVER.into(),
        username: "listener".into(),
        password: "listener-pass".into(),
    });
    let server_account = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(&events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    let (account_id, _) = add_folder(&engine, &events, &media("audiobooks"));
    wait_for(&events, |e| {
        matches!(e, Event::WorksChanged(_)).then_some(())
    });
    let books = items(&setup.db_path, &account_id);
    let book = titled(&books, M4B);
    let db = Db::open(&setup.db_path).unwrap();
    let works = db.store().works();
    let mut members = Vec::new();
    for _ in 0..50 {
        if let Some(work) = works.work_of(&account_id, &book.remote_id).unwrap() {
            members = works.members(work).unwrap();
        }
        if members.len() > 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        members.iter().any(|m| m.account_id == server_account),
        "{members:?}"
    );
    let source = abs_core::works::playback_source(&db.store(), members[0].work_id, |_| false);
    assert_eq!(
        source.unwrap().unwrap().0,
        account_id,
        "it plays without a connection"
    );
    engine.send(Command::RemoveAccount {
        account_id: server_account,
    });
    wait_for(&events, |e| {
        matches!(e, Event::AccountRemoved { .. }).then_some(())
    });
}
