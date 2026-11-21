-- The rules step's own reading of a letter (`detected`) is told apart from a
-- value of a rule that was deleted since (`rule` without rule_id). Rows
-- written before stay as they are: they may hold a value somebody set.
CREATE TABLE document_field_origin_new (
    document_id INTEGER NOT NULL REFERENCES document(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('sender', 'kind', 'recipient', 'date', 'number', 'amount')),
    origin TEXT NOT NULL CHECK (origin IN ('rule', 'ai', 'manual', 'address', 'heading', 'detected')),
    rule_id INTEGER REFERENCES rule(id) ON DELETE SET NULL,
    confidence INTEGER CHECK (confidence BETWEEN 0 AND 100),
    PRIMARY KEY (document_id, field),
    CHECK (rule_id IS NULL OR origin = 'rule'),
    CHECK ((origin = 'ai') = (confidence IS NOT NULL))
);

INSERT INTO document_field_origin_new (document_id, field, origin, rule_id, confidence)
SELECT document_id, field, origin, rule_id, confidence FROM document_field_origin;

DROP TABLE document_field_origin;
ALTER TABLE document_field_origin_new RENAME TO document_field_origin;
