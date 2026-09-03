ALTER TABLE views ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);
ALTER TABLE field_definitions ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);
CREATE UNIQUE INDEX idx_views_default_per_kind ON views(view_kind) WHERE is_default = 1;
