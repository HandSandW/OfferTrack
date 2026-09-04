-- Give upgraded records that never left their creation event the new empty
-- auxiliary-state default without erasing a state explicitly chosen later.
UPDATE applications
SET current_stage_state = ''
WHERE current_stage_state = 'pending'
  AND (SELECT COUNT(*) FROM workflow_events e WHERE e.application_id = applications.id) = 1
  AND EXISTS (
      SELECT 1 FROM workflow_events e
      WHERE e.application_id = applications.id
        AND e.previous_state IS NULL
        AND e.next_state = 'pending'
  );

UPDATE workflow_events
SET next_state = '',
    next_state_name_snapshot = '',
    next_state_kind_snapshot = NULL
WHERE previous_state IS NULL
  AND next_state = 'pending'
  AND (SELECT COUNT(*) FROM workflow_events other WHERE other.application_id = workflow_events.application_id) = 1;

ALTER TABLE views ADD COLUMN name_key TEXT NOT NULL DEFAULT '';
UPDATE views SET name_key = lower(trim(name));

-- Legacy duplicate names are retained but made unambiguous before the index is
-- installed. The stable ID suffix makes the migration deterministic and avoids
-- collisions with user-authored numbered copies.
UPDATE views
SET name = name || '（' || substr(id, 1, 8) || '）',
    name_key = name_key || '（' || id || '）'
WHERE rowid NOT IN (
    SELECT min(rowid) FROM views GROUP BY view_kind, name_key
);

CREATE UNIQUE INDEX idx_views_kind_name_key ON views(view_kind, name_key);
