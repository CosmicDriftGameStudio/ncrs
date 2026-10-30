CREATE TABLE sender_alias_new (
    id INTEGER PRIMARY KEY,
    sender_id INTEGER NOT NULL REFERENCES sender(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('name', 'domain', 'iban', 'tax_id')),
    value TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('offered', 'confirmed', 'discarded')),
    UNIQUE (sender_id, kind, value)
);
INSERT INTO sender_alias_new (id, sender_id, kind, value, state)
SELECT id, sender_id, kind, value, state FROM sender_alias;
DROP TABLE sender_alias;
ALTER TABLE sender_alias_new RENAME TO sender_alias;
CREATE INDEX sender_alias_by_sender ON sender_alias (sender_id);
CREATE UNIQUE INDEX sender_alias_value_in_use ON sender_alias (kind, value) WHERE state <> 'discarded';
