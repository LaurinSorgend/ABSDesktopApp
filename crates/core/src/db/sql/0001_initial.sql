CREATE TABLE server (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE server_url (
    server_id TEXT NOT NULL REFERENCES server(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, url)
);

CREATE TABLE account (
    id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL REFERENCES server(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    username TEXT NOT NULL,
    device_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (server_id, user_id)
);

CREATE TABLE library (
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    remote_id TEXT NOT NULL,
    name TEXT NOT NULL,
    media_type TEXT NOT NULL,
    display_order INTEGER NOT NULL DEFAULT 0,
    raw_json TEXT NOT NULL,
    PRIMARY KEY (account_id, remote_id)
);

CREATE TABLE item (
    account_id TEXT NOT NULL,
    remote_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    title TEXT NOT NULL,
    title_sort TEXT NOT NULL,
    author TEXT,
    narrator TEXT,
    series TEXT,
    asin TEXT,
    isbn TEXT,
    duration REAL,
    cover_etag TEXT,
    added_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    raw_json TEXT NOT NULL,
    PRIMARY KEY (account_id, remote_id),
    FOREIGN KEY (account_id, library_id)
        REFERENCES library(account_id, remote_id) ON DELETE CASCADE
);

CREATE INDEX item_library_title ON item(account_id, library_id, title_sort);
CREATE INDEX item_library_added ON item(account_id, library_id, added_at);

CREATE TABLE progress (
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    position REAL NOT NULL DEFAULT 0,
    progress REAL NOT NULL DEFAULT 0,
    is_finished INTEGER NOT NULL DEFAULT 0,
    ebook_location TEXT,
    ebook_progress REAL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, item_id)
);

CREATE TABLE bookmark (
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    time REAL NOT NULL,
    title TEXT NOT NULL,
    PRIMARY KEY (account_id, item_id, time)
);

CREATE TABLE pending_sync (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE setting (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE VIRTUAL TABLE item_fts USING fts5(
    title, author, narrator, series,
    content = 'item',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER item_fts_insert AFTER INSERT ON item BEGIN
    INSERT INTO item_fts(rowid, title, author, narrator, series)
    VALUES (new.rowid, new.title, new.author, new.narrator, new.series);
END;

CREATE TRIGGER item_fts_delete AFTER DELETE ON item BEGIN
    INSERT INTO item_fts(item_fts, rowid, title, author, narrator, series)
    VALUES ('delete', old.rowid, old.title, old.author, old.narrator, old.series);
END;

CREATE TRIGGER item_fts_update AFTER UPDATE ON item BEGIN
    INSERT INTO item_fts(item_fts, rowid, title, author, narrator, series)
    VALUES ('delete', old.rowid, old.title, old.author, old.narrator, old.series);
    INSERT INTO item_fts(rowid, title, author, narrator, series)
    VALUES (new.rowid, new.title, new.author, new.narrator, new.series);
END;
