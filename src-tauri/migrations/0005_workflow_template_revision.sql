-- Template edits use the same optimistic version contract as applications.
ALTER TABLE workflow_templates ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);
CREATE UNIQUE INDEX idx_workflow_templates_single_default
    ON workflow_templates(is_default) WHERE is_default = 1;
