use crate::{
    error::CoreError,
    overview::Interview,
    recruitment::Event,
    tasks::{Task, timestamp},
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub source_kind: String,
    pub source_id: String,
    pub application_id: Option<String>,
    pub label: String,
    pub at_utc: Option<String>,
    pub starts_at_utc: Option<String>,
    pub finished: bool,
    pub high_priority: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DueBucket {
    pub label: String,
    pub keys: Vec<String>,
}

pub fn entries(tasks: &[Task], events: &[Event], interviews: &[Interview]) -> Vec<Entry> {
    let mut entries: Vec<_> = tasks
        .iter()
        .filter(|t| !t.application_archived)
        .map(|t| Entry {
            key: format!("task:{}", t.id),
            source_kind: "task".into(),
            source_id: t.id.clone(),
            application_id: t.application_id.clone(),
            label: t.title.clone(),
            at_utc: t.due_at_utc.clone(),
            starts_at_utc: None,
            finished: t.completed_at_utc.is_some(),
            high_priority: t.priority == "high",
        })
        .collect();
    entries.extend(
        events
            .iter()
            .filter(|e| !e.application_archived && !e.application_terminal)
            .map(|e| Entry {
                key: format!("event:{}", e.id),
                source_kind: "event".into(),
                source_id: e.id.clone(),
                application_id: e.application_id.clone(),
                label: e.title.clone(),
                at_utc: e.deadline_at_utc.clone().or(e.starts_at_utc.clone()),
                starts_at_utc: e.starts_at_utc.clone(),
                finished: e.finished,
                high_priority: false,
            }),
    );
    entries.extend(interviews.iter().map(|i| Entry {
        key: format!("interview:{}", i.id),
        source_kind: "interview".into(),
        source_id: i.id.clone(),
        application_id: Some(i.application_id.clone()),
        label: i.label.clone(),
        at_utc: Some(i.scheduled_at_utc.clone()),
        starts_at_utc: Some(i.scheduled_at_utc.clone()),
        finished: false,
        high_priority: false,
    }));
    entries
}

pub fn due_buckets(
    entries: &[Entry],
    now: DateTime<Utc>,
    days: i64,
) -> Result<Vec<DueBucket>, CoreError> {
    let mut overdue = Vec::new();
    let mut soon = Vec::new();
    let mut priority = Vec::new();
    for e in entries.iter().filter(|e| !e.finished) {
        if e.high_priority {
            priority.push(e.key.clone());
        }
        if let Some(at) = &e.at_utc {
            let at = timestamp(at).map_err(|_| CoreError::DatabaseInvalid)?;
            if at < now {
                overdue.push(e.key.clone());
            } else if at <= now + Duration::days(days) {
                soon.push(e.key.clone());
            }
        }
    }
    Ok(vec![
        DueBucket {
            label: "已逾期事项".into(),
            keys: overdue,
        },
        DueBucket {
            label: format!("未来 {days} 天到期事项"),
            keys: soon,
        },
        DueBucket {
            label: "高优先级待办".into(),
            keys: priority,
        },
    ])
}
