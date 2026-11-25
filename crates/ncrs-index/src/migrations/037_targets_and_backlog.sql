-- Rules belong to a target. Every rule so far files into the one target, 'main'.
ALTER TABLE rule ADD COLUMN target TEXT NOT NULL DEFAULT 'main';

-- A suggestion no longer files on its own: it stays visible and is switched on by hand.
UPDATE rule SET enabled = 0 WHERE confirmed = 0;

-- Nothing waits in the background for a filing job any more, and no question
-- stays open: the plan is worked out again from the document.
UPDATE document SET status = 'new' WHERE status = 'planned';
UPDATE document SET status = 'new'
WHERE status <> 'filed'
  AND id IN (SELECT document_id FROM review_entry WHERE resolved_at IS NULL);
UPDATE review_entry
SET resolved_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE resolved_at IS NULL;

-- Discarded suggestions are remembered per target.
CREATE TABLE discarded_rule_suggestion_new (
    match_text TEXT NOT NULL,
    condition_kind TEXT NOT NULL,
    kind TEXT NOT NULL,
    filing_pattern TEXT NOT NULL,
    discarded_at INTEGER NOT NULL,
    target TEXT NOT NULL DEFAULT 'main',
    PRIMARY KEY (match_text, condition_kind, kind, filing_pattern, target)
);
INSERT INTO discarded_rule_suggestion_new
    (match_text, condition_kind, kind, filing_pattern, discarded_at)
SELECT match_text, condition_kind, kind, filing_pattern, discarded_at
FROM discarded_rule_suggestion;
DROP TABLE discarded_rule_suggestion;
ALTER TABLE discarded_rule_suggestion_new RENAME TO discarded_rule_suggestion;

-- The files a source already held when it was switched to watching: only what
-- is new or changed after that is filed. Size and modification time as in `document`.
CREATE TABLE source_backlog (
    source TEXT NOT NULL,
    path TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    file_mtime INTEGER NOT NULL,
    PRIMARY KEY (source, path)
);

-- A source whose files were taken stock of; one with an empty backlog is among them.
CREATE TABLE source_watch (
    source TEXT PRIMARY KEY
);

-- The batches of one run carry the id of its first batch. NULL is a run of its own.
ALTER TABLE execution_batch ADD COLUMN run_id INTEGER;

-- What was picked per folder in the chat; the table was made outside the migrations.
DROP TABLE IF EXISTS folder_pick;
