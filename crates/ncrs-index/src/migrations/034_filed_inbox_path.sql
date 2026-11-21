-- An inbox file whose content is already filed stays where it is and is not a
-- copy. The stamp it had when that was found lets the next scan skip hashing
-- it while the file is unchanged.
CREATE TABLE filed_inbox_path (
    path TEXT PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES document (id) ON DELETE CASCADE,
    file_size INTEGER NOT NULL,
    file_mtime INTEGER NOT NULL
);

CREATE INDEX filed_inbox_path_by_document ON filed_inbox_path (document_id);
