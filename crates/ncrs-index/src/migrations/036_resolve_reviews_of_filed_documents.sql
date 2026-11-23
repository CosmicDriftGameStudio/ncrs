-- A document the user filed or moved into a write folder by hand kept its open
-- review, which then counted for good. A filed document has nothing to decide.
UPDATE review_entry
SET resolved_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE resolved_at IS NULL
  AND document_id IN (SELECT id FROM document WHERE status = 'filed');
