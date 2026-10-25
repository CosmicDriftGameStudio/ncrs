CREATE TABLE reorganisation_plan (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    location TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    executed_at INTEGER
);

CREATE INDEX reorganisation_plan_location ON reorganisation_plan (location);

CREATE TABLE reorganisation_item (
    plan_id INTEGER NOT NULL REFERENCES reorganisation_plan (id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    hash BLOB NOT NULL,
    outcome TEXT NOT NULL,
    target TEXT,
    detail TEXT,
    PRIMARY KEY (plan_id, source)
);

CREATE TABLE reorganisation_finding (
    plan_id INTEGER NOT NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    detail TEXT NOT NULL,
    PRIMARY KEY (plan_id, source, kind),
    FOREIGN KEY (plan_id, source) REFERENCES reorganisation_item (plan_id, source)
        ON DELETE CASCADE
);
