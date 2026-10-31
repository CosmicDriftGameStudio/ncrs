-- Tax contexts are replaced by tags: the context columns and tables go, the
-- field origin loses its condition form, and documents and rules gain the
-- columns the tags need. Tables with a REFERENCES column are rebuilt, because
-- DROP COLUMN refuses them.

CREATE TABLE document_new (
    id INTEGER PRIMARY KEY,
    hash BLOB NOT NULL UNIQUE CHECK (length(hash) = 32),
    path TEXT NOT NULL,
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
    CHECK ((amount_minor IS NULL) = (currency IS NULL))
);

INSERT INTO document_new (id, hash, path, source, received_at, status, sender_id, kind_id,
    document_date, document_number, amount_minor, currency, file_size, file_mtime, missing_since)
SELECT id, hash, path, source, received_at, status, sender_id, kind_id,
    document_date, document_number, amount_minor, currency, file_size, file_mtime, missing_since
FROM document;

DROP TABLE document;
ALTER TABLE document_new RENAME TO document;

CREATE INDEX document_by_path ON document (path);

CREATE TRIGGER document_text_follows_document AFTER DELETE ON document
BEGIN
    DELETE FROM document_text WHERE rowid = OLD.id;
END;

CREATE TABLE rule_new (
    id INTEGER PRIMARY KEY,
    combinator TEXT NOT NULL DEFAULT 'all' CHECK (combinator IN ('all', 'any')),
    extract_number TEXT,
    extract_date TEXT,
    extract_amount TEXT,
    kind_id INTEGER REFERENCES document_kind (id),
    filing_pattern TEXT NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    origin TEXT NOT NULL DEFAULT 'hand' CHECK (origin IN ('hand', 'suggestion', 'filing')),
    name TEXT
);

INSERT INTO rule_new (id, combinator, extract_number, extract_date, extract_amount, kind_id,
    filing_pattern, confirmed, position, enabled, origin)
SELECT id, combinator, extract_number, extract_date, extract_amount, kind_id,
    filing_pattern, confirmed, position, enabled, origin
FROM rule;

DROP TABLE rule;
ALTER TABLE rule_new RENAME TO rule;

CREATE TABLE saved_query_new (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    year INTEGER,
    kind TEXT,
    full_text TEXT,
    created_at INTEGER NOT NULL,
    sender TEXT
);

INSERT INTO saved_query_new (id, name, year, kind, full_text, created_at, sender)
SELECT id, name, year, kind, full_text, created_at, sender FROM saved_query;

DROP TABLE saved_query;
ALTER TABLE saved_query_new RENAME TO saved_query;

DROP TABLE tax_context_condition;
DROP TABLE tax_context;

-- Rows of the tax context field, or of the condition origin, have no successor.
CREATE TABLE document_field_origin_new (
    document_id INTEGER NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('sender', 'kind', 'recipient', 'date', 'number', 'amount')),
    origin TEXT NOT NULL CHECK (origin IN ('rule', 'ai', 'manual', 'address', 'heading')),
    rule_id INTEGER REFERENCES rule(id) ON DELETE SET NULL,
    confidence INTEGER CHECK (confidence BETWEEN 0 AND 100),
    PRIMARY KEY (document_id, field),
    CHECK (rule_id IS NULL OR origin = 'rule'),
    CHECK ((origin = 'ai') = (confidence IS NOT NULL))
);

INSERT INTO document_field_origin_new (document_id, field, origin, rule_id, confidence)
SELECT document_id, field, origin, rule_id, confidence FROM document_field_origin
WHERE field <> 'tax_context' AND origin <> 'condition';

DROP TABLE document_field_origin;
ALTER TABLE document_field_origin_new RENAME TO document_field_origin;

-- Reviews are read back whatever their state, so none may keep a reason or
-- field that no longer exists.
UPDATE review_entry SET reason = 'no_matching_rule', field = NULL
WHERE reason IN ('no_tax_context_matches', 'several_tax_contexts_match') OR field = 'tax_context';

-- Tags a person set themselves; the automatic ones are written from the fields.
ALTER TABLE document_tag ADD COLUMN automatic INTEGER NOT NULL DEFAULT 0
    CHECK (automatic IN (0, 1));

-- Filing patterns lose the placeholder together with the separator beside it.
UPDATE rule SET filing_pattern = replace(filing_pattern, '{tax_context}/', '') WHERE instr(filing_pattern, '{tax_context}/') > 0;
UPDATE rule SET filing_pattern = replace(filing_pattern, '/{tax_context}', '') WHERE instr(filing_pattern, '/{tax_context}') > 0;
UPDATE rule SET filing_pattern = replace(filing_pattern, '{tax_context}_', '') WHERE instr(filing_pattern, '{tax_context}_') > 0;
UPDATE rule SET filing_pattern = replace(filing_pattern, '_{tax_context}', '') WHERE instr(filing_pattern, '_{tax_context}') > 0;
UPDATE rule SET filing_pattern = replace(filing_pattern, '{tax_context}-', '') WHERE instr(filing_pattern, '{tax_context}-') > 0;
UPDATE rule SET filing_pattern = replace(filing_pattern, '-{tax_context}', '') WHERE instr(filing_pattern, '-{tax_context}') > 0;
UPDATE rule SET filing_pattern = replace(filing_pattern, '{tax_context}', '') WHERE instr(filing_pattern, '{tax_context}') > 0;
UPDATE rule SET filing_pattern = '{year}/{year}{month}{day}_{sender}_{number}' WHERE filing_pattern IN ('', '/');
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '{tax_context}/', '') WHERE instr(filing_pattern, '{tax_context}/') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '/{tax_context}', '') WHERE instr(filing_pattern, '/{tax_context}') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '{tax_context}_', '') WHERE instr(filing_pattern, '{tax_context}_') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '_{tax_context}', '') WHERE instr(filing_pattern, '_{tax_context}') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '{tax_context}-', '') WHERE instr(filing_pattern, '{tax_context}-') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '-{tax_context}', '') WHERE instr(filing_pattern, '-{tax_context}') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = replace(filing_pattern, '{tax_context}', '') WHERE instr(filing_pattern, '{tax_context}') > 0;
UPDATE OR REPLACE discarded_rule_suggestion SET filing_pattern = '{year}/{year}{month}{day}_{sender}_{number}' WHERE filing_pattern IN ('', '/');
UPDATE location_subtree SET pattern = replace(pattern, '{tax_context}/', '') WHERE instr(pattern, '{tax_context}/') > 0;
UPDATE location_subtree SET pattern = replace(pattern, '/{tax_context}', '') WHERE instr(pattern, '/{tax_context}') > 0;
UPDATE location_subtree SET pattern = replace(pattern, '{tax_context}_', '') WHERE instr(pattern, '{tax_context}_') > 0;
UPDATE location_subtree SET pattern = replace(pattern, '_{tax_context}', '') WHERE instr(pattern, '_{tax_context}') > 0;
UPDATE location_subtree SET pattern = replace(pattern, '{tax_context}-', '') WHERE instr(pattern, '{tax_context}-') > 0;
UPDATE location_subtree SET pattern = replace(pattern, '-{tax_context}', '') WHERE instr(pattern, '-{tax_context}') > 0;
UPDATE location_subtree SET pattern = replace(pattern, '{tax_context}', '') WHERE instr(pattern, '{tax_context}') > 0;
UPDATE location_subtree SET pattern = '{year}/{year}{month}{day}_{sender}_{number}' WHERE pattern IN ('', '/');
