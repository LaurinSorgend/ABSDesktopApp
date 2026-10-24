CREATE TABLE download (
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    title TEXT NOT NULL,
    dir TEXT NOT NULL,
    state TEXT NOT NULL,
    error TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, item_id)
);

CREATE TABLE download_file (
    account_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    ino TEXT NOT NULL,
    position INTEGER NOT NULL,
    file_name TEXT NOT NULL,
    size INTEGER NOT NULL,
    bytes_done INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, item_id, ino),
    FOREIGN KEY (account_id, item_id)
        REFERENCES download(account_id, item_id) ON DELETE CASCADE
);
