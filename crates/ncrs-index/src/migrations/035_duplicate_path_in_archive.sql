-- Whether a recorded copy lies in an archive location (1) or an inbox (0).
-- NULL: not known, as for every copy recorded before this column existed.
ALTER TABLE duplicate_path ADD COLUMN in_archive INTEGER;
