-- A tag the user shows in or hides from the file list against the group
-- default, which lives in the settings.
CREATE TABLE file_list_tag (
    tag TEXT PRIMARY KEY,
    shown INTEGER NOT NULL CHECK (shown IN (0, 1))
);
