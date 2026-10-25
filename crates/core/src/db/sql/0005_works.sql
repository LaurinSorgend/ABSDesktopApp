CREATE TABLE work (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    author TEXT,
    cover_account_id TEXT,
    cover_item_id TEXT,
    preferred_account_id TEXT REFERENCES account(id) ON DELETE SET NULL
);

CREATE TABLE work_member (
    work_id INTEGER NOT NULL REFERENCES work(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    match_method TEXT NOT NULL,
    confidence REAL NOT NULL,
    PRIMARY KEY (account_id, item_id),
    FOREIGN KEY (account_id, item_id)
        REFERENCES item(account_id, remote_id) ON DELETE CASCADE
);

CREATE INDEX work_member_work ON work_member(work_id);

CREATE TABLE work_override (
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    action TEXT NOT NULL,
    target_account_id TEXT REFERENCES account(id) ON DELETE CASCADE,
    target_item_id TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, item_id)
);
