CREATE TABLE saved_query (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK (length(name) > 0),
    year INTEGER,
    tax_context TEXT CHECK (tax_context IS NULL OR tax_context IN ('company', 'private', 'none')),
    kind TEXT,
    full_text TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE saved_query_tag (
    saved_query_id INTEGER NOT NULL REFERENCES saved_query (id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag (id),
    PRIMARY KEY (saved_query_id, tag_id)
);
