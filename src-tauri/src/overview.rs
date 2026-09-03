//! Deterministic, read-only projections. Counts carry their source IDs for drill-down.
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};
use rusqlite::{Connection, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    error::CoreError,
    recruitment, schedule,
    tasks::{self, ReminderRule, Task},
    warehouse::WarehouseSession,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub label: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub label: String,
    pub created_at_utc: String,
    pub application_date: Option<String>,
    pub stage_key: String,
    pub stage_name: String,
    pub state_kind: String,
    pub terminal: bool,
    pub status_updated_at_utc: String,
    pub updated_at_utc: String,
    pub company_type: String,
    pub industry: String,
    pub work_location: String,
    pub resume_count: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interview {
    pub id: String,
    pub application_id: String,
    pub label: String,
    pub scheduled_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub key: String,
    pub fingerprint: String,
    pub rule_key: String,
    pub source_kind: String,
    pub source_id: String,
    pub application_id: Option<String>,
    pub label: String,
    pub reason: String,
    pub severity: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub generated_at_utc: String,
    pub records: Vec<Record>,
    pub metrics: Vec<Bucket>,
    pub stages: Vec<Bucket>,
    pub industries: Vec<Bucket>,
    pub locations: Vec<Bucket>,
    pub company_types: Vec<Bucket>,
    pub funnel: Vec<Bucket>,
    pub trend: Vec<TrendDay>,
    pub tasks: Vec<Task>,
    pub interviews: Vec<Interview>,
    pub reminders: Vec<Reminder>,
    pub events: Vec<recruitment::Event>,
    pub schedule: Vec<schedule::Entry>,
    pub due_metrics: Vec<schedule::DueBucket>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendDay {
    pub date: String,
    pub created_ids: Vec<String>,
    pub applied_ids: Vec<String>,
}

fn records(connection: &Connection) -> Result<Vec<Record>, CoreError> {
    let mut query = connection.prepare("SELECT a.id,a.company_name || ' · ' || a.position_name,
        a.created_at_utc,a.application_date,COALESCE(s.stable_key,''),COALESCE(s.display_name,'未分类'),
        COALESCE(w.semantic_kind,''),(COALESCE(s.is_terminal,0)=1 OR a.current_stage_state='failed'),
        a.status_updated_at_utc,a.updated_at_utc,a.company_type,a.industry,a.work_location,
        (SELECT COUNT(*) FROM documents d WHERE d.application_id=a.id AND d.missing_at_utc IS NULL
          AND (lower(d.display_name) LIKE '%.pdf' OR lower(d.display_name) LIKE '%.docx' OR lower(d.display_name) LIKE '%.doc'))
        FROM applications a LEFT JOIN workflow_stages s ON s.id=a.current_stage_id
        LEFT JOIN workflow_states w ON w.application_id=a.id AND w.stable_key=a.current_stage_state
        WHERE a.deleted_at_utc IS NULL AND a.archived_at_utc IS NULL ORDER BY a.updated_at_utc DESC,a.id").map_err(|_| CoreError::DatabaseInvalid)?;
    query
        .query_map([], |r| {
            Ok(Record {
                id: r.get(0)?,
                label: r.get(1)?,
                created_at_utc: r.get(2)?,
                application_date: r.get(3)?,
                stage_key: r.get(4)?,
                stage_name: r.get(5)?,
                state_kind: r.get(6)?,
                terminal: r.get(7)?,
                status_updated_at_utc: r.get(8)?,
                updated_at_utc: r.get(9)?,
                company_type: r.get(10)?,
                industry: r.get(11)?,
                work_location: r.get(12)?,
                resume_count: r.get(13)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn interviews(connection: &Connection) -> Result<Vec<Interview>, CoreError> {
    let mut query = connection.prepare("SELECT i.id,i.application_id,a.company_name || ' · ' || i.display_name,i.scheduled_at_utc,i.updated_at_utc
      FROM interview_rounds i JOIN applications a ON a.id=i.application_id
      LEFT JOIN workflow_stages s ON s.id=a.current_stage_id
      LEFT JOIN workflow_states w ON w.application_id=a.id AND w.stable_key=i.state
      WHERE a.deleted_at_utc IS NULL AND a.archived_at_utc IS NULL AND COALESCE(s.is_terminal,0)=0 AND a.current_stage_state<>'failed'
      AND i.completed_at_utc IS NULL AND i.scheduled_at_utc IS NOT NULL AND COALESCE(w.semantic_kind,'') NOT IN ('completed','failed')
      AND NOT EXISTS (SELECT 1 FROM recruitment_events e WHERE e.interview_round_id=i.id)
      ORDER BY julianday(i.scheduled_at_utc),i.id").map_err(|_| CoreError::DatabaseInvalid)?;
    query
        .query_map([], |r| {
            Ok(Interview {
                id: r.get(0)?,
                application_id: r.get(1)?,
                label: r.get(2)?,
                scheduled_at_utc: r.get(3)?,
                updated_at_utc: r.get(4)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn group(records: &[Record], key: impl Fn(&Record) -> String) -> Vec<Bucket> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for record in records {
        let name = key(record);
        groups
            .entry(if name.trim().is_empty() {
                "未分类".into()
            } else {
                name
            })
            .or_default()
            .push(record.id.clone());
    }
    groups
        .into_iter()
        .map(|(label, ids)| Bucket { label, ids })
        .collect()
}

fn rule<'a>(rules: &'a [ReminderRule], key: &str) -> &'a ReminderRule {
    // The fixed seven-rule contract is validated by tasks::rules before calculation.
    rules
        .iter()
        .find(|r| r.key == key)
        .expect("validated reminder rules")
}

fn instant(value: &str) -> Result<DateTime<Utc>, CoreError> {
    tasks::timestamp(value).map_err(|_| CoreError::DatabaseInvalid)
}

fn deadline(
    rules: &[ReminderRule],
    now: DateTime<Utc>,
    due: DateTime<Utc>,
) -> Option<(&ReminderRule, &'static str)> {
    let remaining = due - now;
    if remaining < Duration::zero() {
        return rule(rules, "overdue")
            .enabled
            .then_some((rule(rules, "overdue"), "overdue"));
    }
    let urgent = rule(rules, "due_urgent");
    if urgent.enabled && remaining <= Duration::hours(urgent.value) {
        return Some((urgent, "urgent"));
    }
    let soon = rule(rules, "due_soon");
    (soon.enabled && remaining <= Duration::days(soon.value)).then_some((soon, "normal"))
}

fn reminder(
    source: (&str, &str, Option<&str>, &str, &str),
    rule: (&str, i64, &str),
    severity: &str,
) -> Reminder {
    let (kind, id, application, label, basis) = source;
    let (key, revision, reason) = rule;
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(format!("{kind}:{id}:{basis}:{key}:{revision}:{severity}"))
    );
    Reminder {
        key: format!("{kind}:{id}:{key}"),
        fingerprint,
        rule_key: key.into(),
        source_kind: kind.into(),
        source_id: id.into(),
        application_id: application.map(str::to_owned),
        label: label.into(),
        reason: reason.into(),
        severity: severity.into(),
    }
}

fn calculate(connection: &Connection, now: DateTime<FixedOffset>) -> Result<Overview, CoreError> {
    let records = records(connection)?;
    let tasks = tasks::list(connection)?;
    let interviews = interviews(connection)?;
    let events = recruitment::list(connection)?;
    let rules = tasks::rules(connection)?;
    let utc = now.with_timezone(&Utc);
    let today = now.date_naive();
    let mut trend: Vec<_> = (0..30)
        .rev()
        .map(|days| TrendDay {
            date: (today - Duration::days(days)).to_string(),
            created_ids: Vec::new(),
            applied_ids: Vec::new(),
        })
        .collect();
    let mut reminders = Vec::new();
    for record in &records {
        let created = instant(&record.created_at_utc)?;
        let updated = instant(&record.updated_at_utc)?;
        let status = instant(&record.status_updated_at_utc)?;
        let created_date = created.with_timezone(now.offset()).date_naive().to_string();
        if created <= utc
            && let Some(day) = trend.iter_mut().find(|d| d.date == created_date)
        {
            day.created_ids.push(record.id.clone());
        }
        if let Some(date) = &record.application_date {
            NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| CoreError::DatabaseInvalid)?;
            if let Some(day) = trend.iter_mut().find(|d| &d.date == date) {
                day.applied_ids.push(record.id.clone());
            }
        }
        if record.terminal {
            continue;
        }
        let checks = [
            (
                "missing_resume",
                record.resume_count == 0,
                created,
                "normal",
            ),
            (
                "preparing_idle",
                record.stage_key == "preparing",
                updated,
                "normal",
            ),
            (
                "stage_idle",
                record.stage_key != "preparing",
                status,
                "normal",
            ),
            (
                "result_idle",
                record.state_kind == "awaitingResult",
                status,
                "urgent",
            ),
        ];
        let important = rule(&rules, "result_idle");
        for (key, condition, since, severity) in checks {
            let rule = rule(&rules, key);
            // The more specific important result reminder supersedes ordinary follow-up.
            if key == "stage_idle"
                && record.state_kind == "awaitingResult"
                && important.enabled
                && utc - status >= Duration::days(important.value)
            {
                continue;
            }
            if condition && rule.enabled && utc - since >= Duration::days(rule.value) {
                reminders.push(reminder(
                    (
                        "application",
                        &record.id,
                        Some(&record.id),
                        &record.label,
                        &record.updated_at_utc,
                    ),
                    (&rule.key, rule.revision, &rule.label),
                    severity,
                ));
            }
        }
    }
    for task in tasks
        .iter()
        .filter(|t| t.completed_at_utc.is_none() && !t.application_archived)
    {
        let basis = task.revision.to_string();
        let source = (
            "task",
            task.id.as_str(),
            task.application_id.as_deref(),
            task.title.as_str(),
            basis.as_str(),
        );
        if let Some(due) = &task.due_at_utc
            && let Some((rule, severity)) = deadline(&rules, utc, instant(due)?)
        {
            reminders.push(reminder(
                source,
                (&rule.key, rule.revision, &rule.label),
                severity,
            ));
        }
        if let Some(at) = &task.remind_at_utc
            && instant(at)? <= utc
        {
            reminders.push(reminder(
                source,
                ("manual", 0, "已到设定提醒时间"),
                "normal",
            ));
        }
        if task.priority == "high" {
            reminders.push(reminder(source, ("priority", 0, "高优先级待办"), "normal"));
        }
    }
    for interview in &interviews {
        if let Some((rule, severity)) = deadline(&rules, utc, instant(&interview.scheduled_at_utc)?)
        {
            reminders.push(reminder(
                (
                    "interview",
                    &interview.id,
                    Some(&interview.application_id),
                    &interview.label,
                    &interview.updated_at_utc,
                ),
                (&rule.key, rule.revision, &rule.label),
                severity,
            ));
        }
    }
    for event in events
        .iter()
        .filter(|e| !e.finished && !e.application_archived && !e.application_terminal)
    {
        for (checkpoint, at) in [
            ("start", &event.starts_at_utc),
            ("deadline", &event.deadline_at_utc),
        ] {
            if checkpoint == "deadline"
                && event.starts_at_utc.as_deref().map(instant).transpose()?
                    == at.as_deref().map(instant).transpose()?
            {
                continue;
            }
            if let Some(at) = at
                && let Some((rule, severity)) = deadline(&rules, utc, instant(at)?)
            {
                let reason = format!(
                    "{} · {}",
                    if checkpoint == "start" {
                        "计划时间"
                    } else {
                        "截止时间"
                    },
                    rule.label
                );
                let mut notice = reminder(
                    (
                        "event",
                        &event.id,
                        event.application_id.as_deref(),
                        &event.title,
                        &event.source_version,
                    ),
                    (
                        &format!("{checkpoint}:{}", rule.key),
                        rule.revision,
                        &reason,
                    ),
                    severity,
                );
                notice.rule_key = rule.key.clone();
                reminders.push(notice);
            }
        }
    }
    let schedule = schedule::entries(&tasks, &events, &interviews);
    let due_metrics = schedule::due_buckets(&schedule, utc, rule(&rules, "due_soon").value)?;
    let mut action_query = connection
        .prepare("SELECT reminder_key,fingerprint,until_utc FROM reminder_actions")
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let actions = action_query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for (key, fingerprint, until) in actions {
        let hide = until
            .as_deref()
            .map(instant)
            .transpose()?
            .is_none_or(|at| at > utc);
        if hide {
            reminders.retain(|r| r.key != key || r.fingerprint != fingerprint);
        }
    }
    reminders.sort_by_key(|r| {
        (
            match r.severity.as_str() {
                "overdue" => 0,
                "urgent" => 1,
                _ => 2,
            },
            r.key.clone(),
        )
    });
    type MetricPredicate = (&'static str, fn(&Record) -> bool);
    let predicates: [MetricPredicate; 6] = [
        ("活跃投递", |_| true),
        ("准备投递", |r| {
            !r.terminal && r.stage_key == "preparing"
        }),
        ("进行中", |r| !r.terminal && r.stage_key != "preparing"),
        ("待结果", |r| {
            !r.terminal && r.state_kind == "awaitingResult"
        }),
        ("Offer", |r| r.terminal && r.stage_key == "offer"),
        ("已挂", |r| r.terminal && r.stage_key != "offer"),
    ];
    let mut metrics: Vec<_> = predicates
        .into_iter()
        .map(|(label, filter)| Bucket {
            label: label.into(),
            ids: records
                .iter()
                .filter(|r| filter(r))
                .map(|r| r.id.clone())
                .collect(),
        })
        .collect();
    for days in [7, 30] {
        let recent = &trend[(30 - days)..];
        metrics.push(Bucket {
            label: format!("近 {days} 天创建"),
            ids: recent.iter().flat_map(|d| d.created_ids.clone()).collect(),
        });
        metrics.push(Bucket {
            label: format!("近 {days} 天投递"),
            ids: recent.iter().flat_map(|d| d.applied_ids.clone()).collect(),
        });
    }
    let mut funnel = Vec::new();
    for (key, label) in [
        ("applied", "已投递"),
        ("assessment", "在线测评"),
        ("written_exam", "笔试"),
        ("interview", "面试"),
        ("interview_passed", "面试通过"),
        ("signing", "待签约"),
        ("offer", "Offer"),
    ] {
        let mut q = connection.prepare("SELECT DISTINCT e.application_id FROM workflow_events e JOIN workflow_stages s ON e.stage_id=s.id JOIN applications a ON a.id=e.application_id WHERE s.stable_key=?1 AND e.next_state<>'failed' AND a.deleted_at_utc IS NULL AND a.archived_at_utc IS NULL ORDER BY e.application_id").map_err(|_| CoreError::DatabaseInvalid)?;
        let ids = q
            .query_map([key], |r| r.get(0))
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?;
        funnel.push(Bucket {
            label: label.into(),
            ids,
        });
    }
    Ok(Overview {
        generated_at_utc: utc.to_rfc3339(),
        metrics,
        stages: group(&records, |r| {
            if r.terminal && r.stage_key != "offer" {
                "已挂".into()
            } else {
                r.stage_name.clone()
            }
        }),
        industries: group(&records, |r| r.industry.clone()),
        locations: group(&records, |r| r.work_location.clone()),
        company_types: group(&records, |r| r.company_type.clone()),
        records,
        trend,
        funnel,
        tasks,
        interviews,
        reminders,
        events,
        schedule,
        due_metrics,
    })
}

pub fn get(session: &WarehouseSession, now: DateTime<FixedOffset>) -> Result<Overview, CoreError> {
    let tx = session
        .connection()
        .unchecked_transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    calculate(&tx, now)
}

pub fn respond(
    session: &mut WarehouseSession,
    key: &str,
    fingerprint: &str,
    snooze: bool,
) -> Result<(), CoreError> {
    let tx = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let now = chrono::Local::now().fixed_offset();
    let data = calculate(&tx, now)?;
    if !data
        .reminders
        .iter()
        .any(|r| r.key == key && r.fingerprint == fingerprint)
    {
        return Err(CoreError::RevisionConflict);
    }
    let until = snooze.then(|| (now + Duration::hours(24)).with_timezone(&Utc).to_rfc3339());
    tx.execute("INSERT INTO reminder_actions (reminder_key,fingerprint,until_utc,updated_at_utc) VALUES (?1,?2,?3,?4) ON CONFLICT(reminder_key) DO UPDATE SET fingerprint=excluded.fingerprint,until_utc=excluded.until_utc,updated_at_utc=excluded.updated_at_utc",params![key,fingerprint,until,now.with_timezone(&Utc).to_rfc3339()]).map_err(|_| CoreError::DatabaseInvalid)?;
    tx.commit().map_err(|_| CoreError::DatabaseInvalid)
}

#[cfg(test)]
#[path = "productivity_tests.rs"]
mod tests;
