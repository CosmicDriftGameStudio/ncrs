-- A tag name can be a sender and an own tag at once; an exception belongs to
-- one group. The exceptions kept so far applied to the name in every group, so
-- each is copied to all of them and nothing the user saw changes.
CREATE TABLE file_list_tag_new (
    grp TEXT NOT NULL CHECK (grp IN ('kind', 'recipient', 'sender', 'year', 'own')),
    tag TEXT NOT NULL,
    shown INTEGER NOT NULL CHECK (shown IN (0, 1)),
    PRIMARY KEY (grp, tag)
);

INSERT INTO file_list_tag_new (grp, tag, shown)
SELECT groups.grp, file_list_tag.tag, file_list_tag.shown
FROM file_list_tag
CROSS JOIN (
    SELECT 'kind' AS grp UNION ALL SELECT 'recipient' UNION ALL SELECT 'sender'
    UNION ALL SELECT 'year' UNION ALL SELECT 'own'
) AS groups;

DROP TABLE file_list_tag;
ALTER TABLE file_list_tag_new RENAME TO file_list_tag;
