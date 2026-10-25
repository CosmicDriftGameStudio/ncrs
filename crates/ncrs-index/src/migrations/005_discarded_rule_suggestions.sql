CREATE TABLE discarded_rule_suggestion (
    match_text TEXT NOT NULL,
    kind TEXT NOT NULL,
    filing_pattern TEXT NOT NULL,
    discarded_at INTEGER NOT NULL,
    PRIMARY KEY (match_text, kind, filing_pattern)
);
