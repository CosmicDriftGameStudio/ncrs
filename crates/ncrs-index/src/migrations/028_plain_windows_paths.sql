-- Windows paths were stored the way canonicalize returns them, with the
-- verbatim prefix: \\?\C:\x and \\?\UNC\server\share\x. The app now stores and
-- compares C:\x and \\server\share\x. SQLite string literals have no escapes,
-- so '\\?\' is the four characters \ \ ? \.
--
-- Drive form:  \\?\X:...    -> X:...          (X an ASCII letter)
-- Share form:  \\?\UNC\s... -> \\s...
-- Anything else (\\?\Volume{...}\, plain, Unix) stays as it is.

-- A document whose plain path another document already holds keeps its
-- prefixed path: the two rows are different files, and neither may be merged
-- or removed by a path rewrite.
UPDATE document SET path = CASE
    WHEN substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(path, 5)
    ELSE '\' || substr(path, 8)
END
WHERE (
        (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
            AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
        OR substr(path, 1, 8) = '\\?\UNC\'
    )
    AND NOT EXISTS (
        SELECT 1 FROM document AS other
        WHERE other.path = CASE
            WHEN substr(document.path, 1, 4) = '\\?\' AND substr(document.path, 6, 1) = ':'
                AND upper(substr(document.path, 5, 1)) BETWEEN 'A' AND 'Z'
                THEN substr(document.path, 5)
            ELSE '\' || substr(document.path, 8)
        END
    );

UPDATE OR IGNORE duplicate_path SET path = CASE
    WHEN substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(path, 5)
    ELSE '\' || substr(path, 8)
END
WHERE (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
    OR substr(path, 1, 8) = '\\?\UNC\';

DELETE FROM duplicate_path
WHERE (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
    OR substr(path, 1, 8) = '\\?\UNC\';

UPDATE OR IGNORE excluded_path SET path = CASE
    WHEN substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(path, 5)
    ELSE '\' || substr(path, 8)
END
WHERE (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
    OR substr(path, 1, 8) = '\\?\UNC\';

DELETE FROM excluded_path
WHERE (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
    OR substr(path, 1, 8) = '\\?\UNC\';

UPDATE OR IGNORE execution_created_dir SET path = CASE
    WHEN substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(path, 5)
    ELSE '\' || substr(path, 8)
END
WHERE (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
    OR substr(path, 1, 8) = '\\?\UNC\';

DELETE FROM execution_created_dir
WHERE (substr(path, 1, 4) = '\\?\' AND substr(path, 6, 1) = ':'
        AND upper(substr(path, 5, 1)) BETWEEN 'A' AND 'Z')
    OR substr(path, 1, 8) = '\\?\UNC\';

UPDATE execution_item SET
    from_path = CASE
        WHEN substr(from_path, 1, 4) = '\\?\' AND substr(from_path, 6, 1) = ':'
            AND upper(substr(from_path, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(from_path, 5)
        WHEN substr(from_path, 1, 8) = '\\?\UNC\' THEN '\' || substr(from_path, 8)
        ELSE from_path
    END,
    to_path = CASE
        WHEN substr(to_path, 1, 4) = '\\?\' AND substr(to_path, 6, 1) = ':'
            AND upper(substr(to_path, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(to_path, 5)
        WHEN substr(to_path, 1, 8) = '\\?\UNC\' THEN '\' || substr(to_path, 8)
        ELSE to_path
    END;

UPDATE execution_batch SET
    source_root = CASE
        WHEN substr(source_root, 1, 4) = '\\?\' AND substr(source_root, 6, 1) = ':'
            AND upper(substr(source_root, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(source_root, 5)
        WHEN substr(source_root, 1, 8) = '\\?\UNC\' THEN '\' || substr(source_root, 8)
        ELSE source_root
    END,
    source_resolved = CASE
        WHEN substr(source_resolved, 1, 4) = '\\?\' AND substr(source_resolved, 6, 1) = ':'
            AND upper(substr(source_resolved, 5, 1)) BETWEEN 'A' AND 'Z'
            THEN substr(source_resolved, 5)
        WHEN substr(source_resolved, 1, 8) = '\\?\UNC\' THEN '\' || substr(source_resolved, 8)
        ELSE source_resolved
    END,
    target_root = CASE
        WHEN substr(target_root, 1, 4) = '\\?\' AND substr(target_root, 6, 1) = ':'
            AND upper(substr(target_root, 5, 1)) BETWEEN 'A' AND 'Z' THEN substr(target_root, 5)
        WHEN substr(target_root, 1, 8) = '\\?\UNC\' THEN '\' || substr(target_root, 8)
        ELSE target_root
    END,
    target_resolved = CASE
        WHEN substr(target_resolved, 1, 4) = '\\?\' AND substr(target_resolved, 6, 1) = ':'
            AND upper(substr(target_resolved, 5, 1)) BETWEEN 'A' AND 'Z'
            THEN substr(target_resolved, 5)
        WHEN substr(target_resolved, 1, 8) = '\\?\UNC\' THEN '\' || substr(target_resolved, 8)
        ELSE target_resolved
    END;
