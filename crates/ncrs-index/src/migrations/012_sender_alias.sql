CREATE TABLE sender_alias (
    id INTEGER PRIMARY KEY,
    sender_id INTEGER NOT NULL REFERENCES sender(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('name', 'domain', 'iban', 'tax_id')),
    value TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('offered', 'confirmed', 'discarded')),
    UNIQUE (kind, value)
);
CREATE INDEX sender_alias_by_sender ON sender_alias (sender_id);
