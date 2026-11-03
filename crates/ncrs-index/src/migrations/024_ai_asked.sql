-- Whether the AI was asked for the fields of the document, so a document it
-- found nothing in is not asked again at every start.
ALTER TABLE document ADD COLUMN ai_asked INTEGER NOT NULL DEFAULT 0;
