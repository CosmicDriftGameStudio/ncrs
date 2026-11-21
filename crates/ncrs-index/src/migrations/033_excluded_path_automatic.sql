-- An exclusion Hermes made itself ("not a document") remembers the size and
-- modification time of the file it judged, so a different file at the same
-- path is read again. NULL: the user excluded it, or it predates this.
ALTER TABLE excluded_path ADD COLUMN auto_size INTEGER;
ALTER TABLE excluded_path ADD COLUMN auto_mtime INTEGER;
