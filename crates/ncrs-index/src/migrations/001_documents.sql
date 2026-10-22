CREATE TABLE sender (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE
);

CREATE TABLE document_kind (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE
);

CREATE TABLE tag (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE
);

CREATE TABLE document (
    id INTEGER PRIMARY KEY,
    hash BLOB NOT NULL UNIQUE CHECK (length(hash) = 32),
    path TEXT NOT NULL,
    source TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    status TEXT NOT NULL,
    sender_id INTEGER REFERENCES sender (id),
    kind_id INTEGER REFERENCES document_kind (id),
    tax_context TEXT CHECK (tax_context IN ('company', 'private', 'none')),
    document_date TEXT,
    document_number TEXT,
    amount_minor INTEGER,
    currency TEXT,
    CHECK ((amount_minor IS NULL) = (currency IS NULL))
);

CREATE TABLE document_tag (
    document_id INTEGER NOT NULL REFERENCES document (id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag (id),
    PRIMARY KEY (document_id, tag_id)
);

CREATE VIRTUAL TABLE document_text USING fts5(body, tokenize = 'unicode61 remove_diacritics 2');

CREATE TRIGGER document_text_follows_document AFTER DELETE ON document
BEGIN
    DELETE FROM document_text WHERE rowid = OLD.id;
END;

CREATE TABLE rule (
    id INTEGER PRIMARY KEY,
    match_text TEXT,
    match_text_is_regex INTEGER NOT NULL DEFAULT 0 CHECK (match_text_is_regex IN (0, 1)),
    match_sender_id INTEGER REFERENCES sender (id),
    extract_number TEXT,
    extract_date TEXT,
    extract_amount TEXT,
    kind_id INTEGER NOT NULL REFERENCES document_kind (id),
    tax_context TEXT NOT NULL CHECK (tax_context IN ('company', 'private', 'none')),
    filing_pattern TEXT NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1)),
    CHECK (match_text IS NOT NULL OR match_sender_id IS NOT NULL)
);

CREATE TABLE rule_tag (
    rule_id INTEGER NOT NULL REFERENCES rule (id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag (id),
    PRIMARY KEY (rule_id, tag_id)
);

CREATE TABLE review_entry (
    id INTEGER PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES document (id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    field TEXT,
    created_at INTEGER NOT NULL,
    resolved_at INTEGER
);

CREATE UNIQUE INDEX one_open_review_per_document ON review_entry (document_id) WHERE resolved_at IS NULL;
