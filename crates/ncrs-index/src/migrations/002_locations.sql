ALTER TABLE document ADD COLUMN file_size INTEGER;
ALTER TABLE document ADD COLUMN file_mtime INTEGER;
ALTER TABLE document ADD COLUMN missing_since INTEGER;

CREATE INDEX document_by_path ON document (path);

CREATE TABLE duplicate_path (
    id INTEGER PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES document (id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE,
    seen_at INTEGER NOT NULL,
    file_size INTEGER,
    file_mtime INTEGER
);

CREATE INDEX duplicate_path_by_document ON duplicate_path (document_id);
