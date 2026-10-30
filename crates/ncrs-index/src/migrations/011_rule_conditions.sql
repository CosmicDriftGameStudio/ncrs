-- kind has no CHECK: further kinds join without rebuilding the table, the reader validates it.
CREATE TABLE rule_condition (
    rule_id INTEGER NOT NULL REFERENCES rule (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    negated INTEGER NOT NULL DEFAULT 0 CHECK (negated IN (0, 1)),
    PRIMARY KEY (rule_id, position)
);

INSERT INTO rule_condition (rule_id, position, kind, value)
SELECT id, 0, CASE match_text_is_regex WHEN 1 THEN 'regex' ELSE 'substring' END, match_text
FROM rule WHERE match_text IS NOT NULL;

INSERT INTO rule_condition (rule_id, position, kind, value)
SELECT r.id, 1, 'sender', s.name
FROM rule r JOIN sender s ON s.id = r.match_sender_id;

CREATE TABLE rule_new (
    id INTEGER PRIMARY KEY,
    combinator TEXT NOT NULL DEFAULT 'all' CHECK (combinator IN ('all', 'any')),
    extract_number TEXT,
    extract_date TEXT,
    extract_amount TEXT,
    kind_id INTEGER NOT NULL REFERENCES document_kind (id),
    tax_context TEXT NOT NULL CHECK (tax_context IN ('company', 'private', 'none')),
    filing_pattern TEXT NOT NULL,
    confirmed INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1))
);

INSERT INTO rule_new (id, extract_number, extract_date, extract_amount, kind_id, tax_context,
    filing_pattern, confirmed)
SELECT id, extract_number, extract_date, extract_amount, kind_id, tax_context,
    filing_pattern, confirmed
FROM rule;

DROP TABLE rule;
ALTER TABLE rule_new RENAME TO rule;
