CREATE TABLE location_profile (
    location TEXT PRIMARY KEY,
    learned_at INTEGER NOT NULL
);

CREATE TABLE location_subtree (
    location TEXT NOT NULL REFERENCES location_profile (location) ON DELETE CASCADE,
    subtree TEXT NOT NULL,
    pattern TEXT,
    documents INTEGER NOT NULL,
    total INTEGER NOT NULL,
    PRIMARY KEY (location, subtree)
);
