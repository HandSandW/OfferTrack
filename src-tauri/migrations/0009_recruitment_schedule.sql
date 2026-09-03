ALTER TABLE recruitment_events ADD COLUMN revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0);
ALTER TABLE recruitment_events ADD COLUMN deadline_at_utc TEXT;
ALTER TABLE recruitment_events ADD COLUMN interview_round_id TEXT REFERENCES interview_rounds(id) ON DELETE RESTRICT;
ALTER TABLE recruitment_events ADD COLUMN location TEXT NOT NULL DEFAULT '';
ALTER TABLE recruitment_events ADD COLUMN meeting_url TEXT;
ALTER TABLE recruitment_events ADD COLUMN result TEXT NOT NULL DEFAULT '';
CREATE UNIQUE INDEX idx_event_interview ON recruitment_events(interview_round_id) WHERE interview_round_id IS NOT NULL;
CREATE TRIGGER event_round_owner_insert BEFORE INSERT ON recruitment_events
WHEN NEW.interview_round_id IS NOT NULL AND (NEW.event_type <> 'interview' OR NOT EXISTS (
    SELECT 1 FROM interview_rounds WHERE id=NEW.interview_round_id AND application_id=NEW.application_id
)) BEGIN SELECT RAISE(ABORT, 'Invalid event round owner'); END;
CREATE TRIGGER event_round_owner_update BEFORE UPDATE OF interview_round_id,application_id,event_type ON recruitment_events
WHEN NEW.interview_round_id IS NOT NULL AND (NEW.event_type <> 'interview' OR NOT EXISTS (
    SELECT 1 FROM interview_rounds WHERE id=NEW.interview_round_id AND application_id=NEW.application_id
)) BEGIN SELECT RAISE(ABORT, 'Invalid event round owner'); END;
