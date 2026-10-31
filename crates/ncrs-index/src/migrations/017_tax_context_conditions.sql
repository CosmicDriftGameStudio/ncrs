-- Same shape as rule_condition; the reader validates kind.
CREATE TABLE tax_context_condition (
    tax_context_id INTEGER NOT NULL REFERENCES tax_context (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    negated INTEGER NOT NULL DEFAULT 0 CHECK (negated IN (0, 1)),
    PRIMARY KEY (tax_context_id, position)
);

-- Any of the identifiers of a company is enough, so "any" is the default.
ALTER TABLE tax_context ADD COLUMN combinator TEXT NOT NULL DEFAULT 'any'
    CHECK (combinator IN ('all', 'any'));

-- A NULL tax context of a rule now means "open": the conditions of the contexts
-- decide. Migrated rules all carry a value, so none of them turns open.

CREATE TABLE document_field_origin_new (
    document_id INTEGER NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('sender', 'kind', 'tax_context', 'date', 'number', 'amount')),
    origin TEXT NOT NULL CHECK (origin IN ('rule', 'ai', 'manual', 'condition')),
    rule_id INTEGER REFERENCES rule(id) ON DELETE SET NULL,
    confidence INTEGER CHECK (confidence BETWEEN 0 AND 100),
    condition_kind TEXT,
    condition_value TEXT,
    PRIMARY KEY (document_id, field),
    CHECK (rule_id IS NULL OR origin = 'rule'),
    CHECK ((origin = 'ai') = (confidence IS NOT NULL)),
    CHECK ((origin = 'condition') = (condition_kind IS NOT NULL AND condition_value IS NOT NULL))
);

INSERT INTO document_field_origin_new (document_id, field, origin, rule_id, confidence)
SELECT document_id, field, origin, rule_id, confidence FROM document_field_origin;

DROP TABLE document_field_origin;
ALTER TABLE document_field_origin_new RENAME TO document_field_origin;
