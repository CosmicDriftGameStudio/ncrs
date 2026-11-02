-- Which version of the field detection last read the document, so an archive
-- is read again when the detection learns something new.
ALTER TABLE document ADD COLUMN fields_read_version INTEGER NOT NULL DEFAULT 0;
