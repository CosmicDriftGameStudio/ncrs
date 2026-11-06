-- A rule made in the Hermes chat has its own origin; the check of the column has to learn it.
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
