CREATE TABLE rule_evidence (
    rule_id INTEGER PRIMARY KEY REFERENCES rule (id) ON DELETE CASCADE,
    matching_in_folder INTEGER NOT NULL,
    folder_documents INTEGER NOT NULL,
    matching_elsewhere INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
