-- Whether a document has a non-blank full text, kept beside the text so the
-- counts of the automatic filing need not read every text of the archive.
-- `set_document_text` and `replace_document_content` keep it in step; the FTS
-- table cannot carry a trigger.
ALTER TABLE document ADD COLUMN has_text INTEGER NOT NULL DEFAULT 0 CHECK (has_text IN (0, 1));

UPDATE document SET has_text = 1
WHERE id IN (SELECT rowid FROM document_text WHERE trim(body) <> '');
