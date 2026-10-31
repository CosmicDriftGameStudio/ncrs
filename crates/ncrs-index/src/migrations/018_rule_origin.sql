-- Before this step no rule could be made by hand: a rule with evidence came from
-- a suggestion, every other one from filing a document.
ALTER TABLE rule ADD COLUMN origin TEXT NOT NULL DEFAULT 'hand'
    CHECK (origin IN ('hand', 'suggestion', 'filing'));

UPDATE rule SET origin = CASE
    WHEN EXISTS (SELECT 1 FROM rule_evidence e WHERE e.rule_id = rule.id) THEN 'suggestion'
    ELSE 'filing'
END;
