CREATE TABLE document_field_origin (
    document_id INTEGER NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('sender', 'kind', 'tax_context', 'date', 'number', 'amount')),
    origin TEXT NOT NULL CHECK (origin IN ('rule', 'ai', 'manual')),
    rule_id INTEGER REFERENCES rule(id) ON DELETE SET NULL,
    confidence INTEGER CHECK (confidence BETWEEN 0 AND 100),
    PRIMARY KEY (document_id, field),
    CHECK (rule_id IS NULL OR origin = 'rule'),
    CHECK ((origin = 'ai') = (confidence IS NOT NULL))
);
