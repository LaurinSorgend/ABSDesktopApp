CREATE TABLE local_session (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    title TEXT NOT NULL,
    author TEXT NOT NULL,
    duration REAL NOT NULL,
    start_position REAL NOT NULL,
    position REAL NOT NULL,
    time_listening REAL NOT NULL,
    started_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    decision TEXT
);
