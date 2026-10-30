CREATE TABLE tax_context (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    short_name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

-- id 0 stands for "no context": never listed, renamed or deleted.
INSERT INTO tax_context (id, name, short_name) VALUES
    (0, 'none', 'none'),
    (1, 'Firma', 'company'),
    (2, 'Privat', 'private');

ALTER TABLE document ADD COLUMN tax_context_id INTEGER REFERENCES tax_context (id);
UPDATE document SET tax_context_id = CASE tax_context
    WHEN 'company' THEN 1 WHEN 'private' THEN 2 WHEN 'none' THEN 0 END;
ALTER TABLE document DROP COLUMN tax_context;

ALTER TABLE rule ADD COLUMN tax_context_id INTEGER REFERENCES tax_context (id);
UPDATE rule SET tax_context_id = CASE tax_context
    WHEN 'company' THEN 1 WHEN 'private' THEN 2 WHEN 'none' THEN 0 END;
ALTER TABLE rule DROP COLUMN tax_context;

ALTER TABLE saved_query ADD COLUMN tax_context_id INTEGER REFERENCES tax_context (id);
UPDATE saved_query SET tax_context_id = CASE tax_context
    WHEN 'company' THEN 1 WHEN 'private' THEN 2 WHEN 'none' THEN 0 END;
ALTER TABLE saved_query DROP COLUMN tax_context;
