ALTER TABLE account ADD COLUMN local INTEGER NOT NULL DEFAULT 0;

CREATE TABLE local_folder (
    account_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    last_scan_at INTEGER,
    available INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (account_id, library_id),
    FOREIGN KEY (account_id, library_id)
        REFERENCES library(account_id, remote_id) ON DELETE CASCADE
);

CREATE TABLE local_book (
    account_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    dir TEXT NOT NULL,
    ebook_path TEXT,
    cover_path TEXT,
    signature TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (account_id, item_id),
    FOREIGN KEY (account_id, item_id)
        REFERENCES item(account_id, remote_id) ON DELETE CASCADE
);

CREATE TABLE local_track (
    account_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    duration REAL NOT NULL,
    start_offset REAL NOT NULL,
    PRIMARY KEY (account_id, item_id, position),
    FOREIGN KEY (account_id, item_id)
        REFERENCES local_book(account_id, item_id) ON DELETE CASCADE
);
