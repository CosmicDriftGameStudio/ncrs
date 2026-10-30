CREATE TABLE discarded_rule_suggestion_new (
    match_text TEXT NOT NULL,
    condition_kind TEXT NOT NULL,
    kind TEXT NOT NULL,
    filing_pattern TEXT NOT NULL,
    discarded_at INTEGER NOT NULL,
    PRIMARY KEY (match_text, condition_kind, kind, filing_pattern)
);
INSERT INTO discarded_rule_suggestion_new
    (match_text, condition_kind, kind, filing_pattern, discarded_at)
SELECT match_text, 'word', kind, filing_pattern, discarded_at FROM discarded_rule_suggestion;
DROP TABLE discarded_rule_suggestion;
ALTER TABLE discarded_rule_suggestion_new RENAME TO discarded_rule_suggestion;
ALTER TABLE rule_evidence ADD COLUMN subtree TEXT;
