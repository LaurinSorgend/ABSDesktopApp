//! Scans one chosen folder and brings its library up to date. The app only reads the folder.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use uuid::Uuid;

use super::book::{BookIds, BuiltBook, FoundFile, build_book};
use super::group::{BookGroup, FileKind, group_files, kind_of};
use super::probe::album_of;
use crate::account::lock;
use crate::db::{Db, DbResult, LocalBook, LocalFolder, now_ms};

/// Folders that are never books.
const SKIPPED_FOLDERS: [&str; 3] = ["@eaDir", "$RECYCLE.BIN", "System Volume Information"];
/// A folder with this file in it is left out, as in ABS.
const IGNORE_FILE: &str = ".ignore";

/// What a scan did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    /// The folder could not be reached; nothing was changed.
    pub unavailable: bool,
}

/// The name a folder is shown under.
pub fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Scans a folder. `progress` gets books done and books in total.
pub fn scan_folder(
    db: &Mutex<Db>,
    folder: &LocalFolder,
    progress: &dyn Fn(usize, usize),
) -> DbResult<ScanReport> {
    let (account_id, library_id) = (&folder.account_id, &folder.library_id);
    let known: Vec<LocalBook> = lock(db).store().local().books(account_id, library_id)?;
    let listing = list_files(&folder.path);
    let Ok(listing) = listing else {
        return unavailable(db, folder);
    };
    let has_media = listing
        .files
        .iter()
        .any(|f| kind_of(&f.rel_path) != FileKind::Other);
    if !has_media && !known.is_empty() {
        return unavailable(db, folder);
    }
    let groups = group(&folder.path, &listing.files);
    let known_by_id: HashMap<&str, &LocalBook> =
        known.iter().map(|b| (b.item_id.as_str(), b)).collect();
    let mut report = ScanReport::default();
    let mut found = HashSet::new();
    let mut new_by_signature: HashMap<String, String> = HashMap::new();
    for (done, group) in groups.iter().enumerate() {
        progress(done, groups.len());
        let files = files_of(group, &listing.files);
        let item_id = item_id(library_id, group);
        let fingerprint = fingerprint(&files);
        let existing = known_by_id.get(item_id.as_str());
        if existing.is_some_and(|b| b.fingerprint == fingerprint) {
            found.insert(item_id);
            continue;
        }
        let signature = signature(&files);
        let ids = BookIds {
            item_id: &item_id,
            library_id,
            folder_name: &folder_name(&folder.path),
            signature: &signature,
            fingerprint: &fingerprint,
            added_at: added_at(db, account_id, &item_id),
        };
        let Some(built) = build_book(&folder.path, group, &listing.files, &ids) else {
            if existing.is_some() {
                found.insert(item_id);
            }
            continue;
        };
        store_book(db, account_id, &built)?;
        if existing.is_some() {
            report.updated += 1;
        } else {
            report.added += 1;
            new_by_signature.insert(signature, item_id.clone());
        }
        found.insert(item_id);
    }
    progress(groups.len(), groups.len());
    if listing.complete {
        report.removed = remove_gone(db, account_id, &known, &found, &new_by_signature)?;
    }
    lock(db)
        .store()
        .local()
        .set_scanned(account_id, library_id, true)?;
    Ok(report)
}

fn unavailable(db: &Mutex<Db>, folder: &LocalFolder) -> DbResult<ScanReport> {
    lock(db)
        .store()
        .local()
        .set_scanned(&folder.account_id, &folder.library_id, false)?;
    Ok(ScanReport {
        unavailable: true,
        ..ScanReport::default()
    })
}

/// Files of a folder and whether every subfolder could be read.
struct Listing {
    files: Vec<FoundFile>,
    complete: bool,
}

/// Lists a folder's files. Fails only when the folder itself cannot be read.
fn list_files(root: &Path) -> std::io::Result<Listing> {
    let mut listing = Listing {
        files: Vec::new(),
        complete: true,
    };
    let entries = std::fs::read_dir(root)?;
    walk(entries, root, "", &mut listing);
    Ok(listing)
}

fn walk(entries: std::fs::ReadDir, dir: &Path, rel: &str, listing: &mut Listing) {
    if dir.join(IGNORE_FILE).exists() {
        return;
    }
    for entry in entries {
        let Ok(entry) = entry else {
            listing.complete = false;
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            listing.complete = false;
            continue;
        };
        if name.starts_with('.') || kind.is_symlink() {
            continue;
        }
        let rel_path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if kind.is_dir() {
            if SKIPPED_FOLDERS.contains(&name.as_str()) {
                continue;
            }
            match std::fs::read_dir(entry.path()) {
                Ok(inner) => walk(inner, &entry.path(), &rel_path, listing),
                Err(_) => listing.complete = false,
            }
        } else if let Ok(metadata) = entry.metadata() {
            listing.files.push(FoundFile {
                rel_path,
                size: metadata.len(),
                modified: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |time| time.as_millis() as i64),
            });
        } else {
            listing.complete = false;
        }
    }
}

fn group(root: &Path, files: &[FoundFile]) -> Vec<BookGroup> {
    let albums: RefCell<HashMap<String, Option<String>>> = RefCell::default();
    let album = |rel: &str| {
        albums
            .borrow_mut()
            .entry(rel.to_owned())
            .or_insert_with(|| album_of(&root.join(rel)))
            .clone()
    };
    let paths: Vec<String> = files.iter().map(|f| f.rel_path.clone()).collect();
    group_files(&paths, &album)
}

fn files_of(group: &BookGroup, files: &[FoundFile]) -> Vec<FoundFile> {
    let mut found: Vec<FoundFile> = files
        .iter()
        .filter(|f| group.files.contains(&f.rel_path))
        .cloned()
        .collect();
    found.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    found
}

/// Stable for the same book in the same place.
fn item_id(library_id: &str, group: &BookGroup) -> String {
    stable_id(&format!("{library_id}/{}", group.rel_path()))
}

/// Changes when any file of the book changes.
fn fingerprint(files: &[FoundFile]) -> String {
    let text: Vec<String> = files
        .iter()
        .map(|f| format!("{}|{}|{}", f.rel_path, f.size, f.modified))
        .collect();
    stable_id(&text.join("\n"))
}

/// The same for a book that moved: names and sizes of its media files, without their folders.
fn signature(files: &[FoundFile]) -> String {
    let mut text: Vec<String> = files
        .iter()
        .filter(|f| kind_of(&f.rel_path) != FileKind::Other)
        .map(|f| {
            let name = f.rel_path.rsplit('/').next().unwrap_or(&f.rel_path);
            format!("{name}|{}", f.size)
        })
        .collect();
    text.sort();
    stable_id(&text.join("\n"))
}

fn stable_id(text: &str) -> String {
    Uuid::new_v5(&Uuid::NAMESPACE_OID, text.as_bytes()).to_string()
}

/// When the book was first seen; a changed book keeps its place among the newest.
fn added_at(db: &Mutex<Db>, account_id: &str, item_id: &str) -> i64 {
    let json = lock(db).store().catalog().item_json(account_id, item_id);
    json.ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value["addedAt"].as_i64())
        .unwrap_or_else(now_ms)
}

fn store_book(db: &Mutex<Db>, account_id: &str, built: &BuiltBook) -> DbResult<()> {
    lock(db).write(|store| {
        store
            .catalog()
            .upsert_items(account_id, std::slice::from_ref(&built.item))?;
        store
            .local()
            .save_book(account_id, &built.book, &built.tracks)
    })
}

/// Removes books no longer found. A new book with the same files takes over the listening
/// state of a removed one.
fn remove_gone(
    db: &Mutex<Db>,
    account_id: &str,
    known: &[LocalBook],
    found: &HashSet<String>,
    new_by_signature: &HashMap<String, String>,
) -> DbResult<usize> {
    let gone: Vec<&LocalBook> = known
        .iter()
        .filter(|b| !found.contains(&b.item_id))
        .collect();
    lock(db).write(|store| {
        for book in &gone {
            match new_by_signature.get(&book.signature) {
                Some(new_id) => {
                    store
                        .local()
                        .move_listening_state(account_id, &book.item_id, new_id)?
                }
                None => store
                    .local()
                    .remove_listening_state(account_id, &book.item_id)?,
            }
            store.catalog().remove_item(account_id, &book.item_id)?;
        }
        Ok(gone.len())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail::ItemDetail;
    use std::path::PathBuf;

    const EPUB_OPF: &str = r#"<package><metadata xmlns:dc="d"><dc:title>From Epub</dc:title>
        <dc:creator>Epub Author</dc:creator></metadata><manifest/></package>"#;

    fn write_epub(path: &Path) {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("META-INF/container.xml", options).unwrap();
        zip.write_all(
            br#"<container><rootfiles><rootfile full-path="a/b.opf"/></rootfiles></container>"#,
        )
        .unwrap();
        zip.start_file("a/b.opf", options).unwrap();
        zip.write_all(EPUB_OPF.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    struct Setup {
        dir: tempfile::TempDir,
        db: Mutex<Db>,
        folder: LocalFolder,
    }

    fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let account = db.store().accounts().local_account().unwrap();
        let root: PathBuf = dir.path().join("books");
        std::fs::create_dir(&root).unwrap();
        db.store()
            .local()
            .add_folder(&account.id, "lib", "books", &root)
            .unwrap();
        let folder = db
            .store()
            .local()
            .folder(&account.id, "lib")
            .unwrap()
            .unwrap();
        Setup {
            dir,
            db: Mutex::new(db),
            folder,
        }
    }

    impl Setup {
        fn root(&self) -> PathBuf {
            self.dir.path().join("books")
        }

        fn scan(&self) -> ScanReport {
            scan_folder(&self.db, &self.folder, &|_, _| {}).unwrap()
        }

        fn add_book(&self, rel: &str) {
            let path = self.root().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            write_epub(&path);
        }

        fn items(&self) -> Vec<crate::db::ItemSummary> {
            let query = crate::db::ItemQuery::new("lib");
            let db = lock(&self.db);
            let items = db
                .store()
                .catalog()
                .items_page(&self.folder.account_id, &query, 0, 50);
            items.unwrap()
        }

        fn set_position(&self, item_id: &str) {
            lock(&self.db)
                .store()
                .progress()
                .set_position(&self.folder.account_id, item_id, 5.0, 0.5, 1)
                .unwrap();
        }

        fn position(&self, item_id: &str) -> Option<f64> {
            let db = lock(&self.db);
            let row = db
                .store()
                .progress()
                .get(&self.folder.account_id, item_id)
                .unwrap();
            row.map(|p| p.current_time)
        }
    }

    #[test]
    fn an_ebook_becomes_a_book_with_its_metadata() {
        let setup = setup();
        setup.add_book("Folder Author/Folder Title/book.epub");
        assert_eq!(setup.scan().added, 1);
        let items = setup.items();
        assert_eq!(items[0].title, "From Epub");
        assert_eq!(items[0].author.as_deref(), Some("Epub Author"));

        let db = lock(&setup.db);
        let store = db.store();
        let account = &setup.folder.account_id;
        let json = store
            .catalog()
            .item_json(account, &items[0].remote_id)
            .unwrap()
            .unwrap();
        let detail = ItemDetail::from_json(&json).unwrap();
        assert_eq!(detail.title, "From Epub");
        assert!(!detail.has_audio);
        assert_eq!(detail.ebook_format.as_deref(), Some("epub"));
        let book = store
            .local()
            .book(account, &items[0].remote_id)
            .unwrap()
            .unwrap();
        assert!(book.ebook_path.unwrap().ends_with("book.epub"));
    }

    #[test]
    fn an_unchanged_book_is_left_alone_and_a_changed_one_updated() {
        let setup = setup();
        setup.add_book("Author/Title/book.epub");
        setup.scan();
        assert_eq!(setup.scan(), ScanReport::default());
        std::fs::write(setup.root().join("Author/Title/desc.txt"), "Now described.").unwrap();
        assert_eq!(setup.scan().updated, 1);
        let db = lock(&setup.db);
        let id = db
            .store()
            .local()
            .books(&setup.folder.account_id, "lib")
            .unwrap()
            .remove(0)
            .item_id;
        let json = db
            .store()
            .catalog()
            .item_json(&setup.folder.account_id, &id)
            .unwrap()
            .unwrap();
        let detail = ItemDetail::from_json(&json).unwrap();
        assert_eq!(detail.description.as_deref(), Some("Now described."));
    }

    #[test]
    fn a_moved_book_keeps_its_position() {
        let setup = setup();
        setup.add_book("Author/Title/book.epub");
        setup.scan();
        let old = setup.items().remove(0).remote_id;
        setup.set_position(&old);

        std::fs::create_dir_all(setup.root().join("Other")).unwrap();
        std::fs::rename(
            setup.root().join("Author/Title"),
            setup.root().join("Other/Title"),
        )
        .unwrap();
        let report = setup.scan();
        assert_eq!((report.added, report.removed), (1, 1));
        let new = setup.items().remove(0).remote_id;
        assert_ne!(new, old);
        assert_eq!(setup.position(&new), Some(5.0));
        assert_eq!(setup.position(&old), None);
    }

    #[test]
    fn a_removed_book_goes_with_its_position() {
        let setup = setup();
        setup.add_book("Author/One/book.epub");
        setup.add_book("Author/Two/other.pdf");
        setup.scan();
        let gone = setup
            .items()
            .into_iter()
            .find(|i| i.title == "From Epub")
            .unwrap()
            .remote_id;
        setup.set_position(&gone);
        std::fs::remove_dir_all(setup.root().join("Author/One")).unwrap();
        assert_eq!(setup.scan().removed, 1);
        assert_eq!(setup.items().len(), 1);
        assert_eq!(setup.position(&gone), None);
    }

    #[test]
    fn an_unreachable_folder_keeps_its_books() {
        let setup = setup();
        setup.add_book("Author/Title/book.epub");
        setup.scan();
        let moved_away = setup.dir.path().join("elsewhere");
        std::fs::rename(setup.root(), &moved_away).unwrap();
        assert!(setup.scan().unavailable);
        assert_eq!(setup.items().len(), 1);
        let account = &setup.folder.account_id;
        let stored = lock(&setup.db)
            .store()
            .local()
            .folder(account, "lib")
            .unwrap()
            .unwrap();
        assert!(!stored.available);

        std::fs::rename(&moved_away, setup.root()).unwrap();
        assert_eq!(setup.scan(), ScanReport::default());
        let stored = lock(&setup.db)
            .store()
            .local()
            .folder(account, "lib")
            .unwrap()
            .unwrap();
        assert!(stored.available);
    }

    #[test]
    fn an_empty_folder_that_had_books_counts_as_unreachable() {
        let setup = setup();
        setup.add_book("Author/Title/book.epub");
        setup.scan();
        std::fs::remove_dir_all(setup.root().join("Author")).unwrap();
        assert!(setup.scan().unavailable);
        assert_eq!(setup.items().len(), 1);
    }

    #[test]
    fn hidden_and_ignored_folders_are_skipped() {
        let setup = setup();
        setup.add_book(".hidden/Title/book.epub");
        setup.add_book("Skip/Title/book.epub");
        std::fs::write(setup.root().join("Skip/.ignore"), "").unwrap();
        setup.scan();
        assert!(setup.items().is_empty());
    }

    #[test]
    fn a_damaged_audio_file_is_left_out() {
        let setup = setup();
        let dir = setup.root().join("Author/Title");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.mp3"), b"no audio").unwrap();
        assert_eq!(setup.scan().added, 0);
        write_epub(&dir.join("book.epub"));
        assert_eq!(setup.scan().added, 1);
        let db = lock(&setup.db);
        let account = &setup.folder.account_id;
        let id = db
            .store()
            .local()
            .books(account, "lib")
            .unwrap()
            .remove(0)
            .item_id;
        assert!(db.store().local().tracks(account, &id).unwrap().is_empty());
    }

    #[test]
    fn a_book_id_is_stable() {
        let group = BookGroup {
            dir: "A/B".into(),
            name: None,
            files: Vec::new(),
        };
        assert_eq!(item_id("lib", &group), item_id("lib", &group));
        assert_ne!(item_id("lib", &group), item_id("other", &group));
    }
}
