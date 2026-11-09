-- Every document gets an origin, so a document that is not a file (a mail, an
-- attachment) fits in without a second rebuild of the table. For a file
-- `path` is the key of the origin; the columns for account and message id
-- come in 2.0.0 per ADD COLUMN.

CREATE TABLE document_new (
    id INTEGER PRIMARY KEY,
    hash BLOB NOT NULL UNIQUE CHECK (length(hash) = 32),
    path TEXT,
    source TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    status TEXT NOT NULL,
    sender_id INTEGER REFERENCES sender (id),
    kind_id INTEGER REFERENCES document_kind (id),
    document_date TEXT,
    document_number TEXT,
    amount_minor INTEGER,
    currency TEXT,
    file_size INTEGER,
    file_mtime INTEGER,
    missing_since INTEGER,
    recipient TEXT,
    fields_read_version INTEGER NOT NULL DEFAULT 0,
    ai_asked INTEGER NOT NULL DEFAULT 0,
    origin_kind TEXT NOT NULL DEFAULT 'file' CHECK (origin_kind IN ('file', 'mail', 'attachment')),
    CHECK ((amount_minor IS NULL) = (currency IS NULL)),
    CHECK (origin_kind <> 'file' OR path IS NOT NULL)
);

INSERT INTO document_new (id, hash, path, source, received_at, status, sender_id, kind_id,
    document_date, document_number, amount_minor, currency, file_size, file_mtime, missing_since,
    recipient, fields_read_version, ai_asked)
SELECT id, hash, path, source, received_at, status, sender_id, kind_id,
    document_date, document_number, amount_minor, currency, file_size, file_mtime, missing_since,
    recipient, fields_read_version, ai_asked
FROM document;

DROP TABLE document;
ALTER TABLE document_new RENAME TO document;

CREATE INDEX document_by_path ON document (path);

CREATE TRIGGER document_text_follows_document AFTER DELETE ON document
BEGIN
    DELETE FROM document_text WHERE rowid = OLD.id;
END;
