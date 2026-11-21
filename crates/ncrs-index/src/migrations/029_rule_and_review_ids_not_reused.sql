-- Same reason as for documents: a deleted rule or review entry must not give
-- its id to the next one, or a stale handle would act on a stranger.
CREATE TABLE rule_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    combinator TEXT NOT NULL DEFAULT 'all' CHECK (combinator IN ('all', 'any')),
    extract_number TEXT,
    extract_date TEXT,
    extract_amount TEXT,
    kind_id INTEGER REFERENCES document_kind (id),
    filing_pattern TEXT NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    origin TEXT NOT NULL DEFAULT 'hand' CHECK (origin IN ('hand', 'suggestion', 'filing', 'chat')),
    name TEXT
);

INSERT INTO rule_new (id, combinator, extract_number, extract_date, extract_amount, kind_id,
    filing_pattern, confirmed, position, enabled, origin, name)
SELECT id, combinator, extract_number, extract_date, extract_amount, kind_id,
    filing_pattern, confirmed, position, enabled, origin, name
FROM rule;

DROP TABLE rule;
ALTER TABLE rule_new RENAME TO rule;

CREATE TABLE review_entry_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    document_id INTEGER NOT NULL REFERENCES document (id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    field TEXT,
    created_at INTEGER NOT NULL,
    resolved_at INTEGER
);

INSERT INTO review_entry_new (id, document_id, reason, field, created_at, resolved_at)
SELECT id, document_id, reason, field, created_at, resolved_at
FROM review_entry;

DROP TABLE review_entry;
ALTER TABLE review_entry_new RENAME TO review_entry;

CREATE UNIQUE INDEX one_open_review_per_document ON review_entry (document_id) WHERE resolved_at IS NULL;
