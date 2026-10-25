CREATE TABLE execution_batch (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('reorganisation', 'filing')),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    undone_at INTEGER
);

CREATE TABLE execution_item (
    batch_id INTEGER NOT NULL REFERENCES execution_batch (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    from_path TEXT NOT NULL,
    to_path TEXT NOT NULL,
    hash BLOB NOT NULL,
    document_id INTEGER REFERENCES document (id) ON DELETE SET NULL,
    duplicate INTEGER NOT NULL CHECK (duplicate IN (0, 1)),
    state TEXT NOT NULL
        CHECK (state IN ('pending', 'moved', 'skipped', 'undone', 'undo_skipped')),
    reason TEXT,
    PRIMARY KEY (batch_id, seq)
);

CREATE TABLE execution_created_dir (
    batch_id INTEGER NOT NULL REFERENCES execution_batch (id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    PRIMARY KEY (batch_id, path)
);
