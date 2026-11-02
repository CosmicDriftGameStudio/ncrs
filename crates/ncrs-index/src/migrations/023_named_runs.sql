-- A run can have a name and can copy or trash instead of move. The kind check
-- gains 'hermes', so the table is rebuilt (SQLite cannot alter a CHECK).
CREATE TABLE execution_batch_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('reorganisation', 'filing', 'hermes')),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    undone_at INTEGER,
    name TEXT,
    transfer TEXT NOT NULL DEFAULT 'move' CHECK (transfer IN ('move', 'copy', 'trash')),
    -- The folder the run took its files from, as given and as the system
    -- resolved it then; an undo checks the way back against them.
    source_root TEXT,
    source_resolved TEXT,
    -- The folder the run put its files in, recorded the same way.
    target_root TEXT,
    target_resolved TEXT
);
INSERT INTO execution_batch_new (id, kind, started_at, finished_at, undone_at)
SELECT id, kind, started_at, finished_at, undone_at FROM execution_batch;
DROP TABLE execution_batch;
ALTER TABLE execution_batch_new RENAME TO execution_batch;
