//! Phase-four performance baselines on synthetic, disposable warehouses.
//! Setup time is intentionally excluded from the measured service calls.

use std::{fs, time::Duration, time::Instant};

use chrono::{DateTime, FixedOffset};
use rusqlite::params;

use crate::{
    agent_access, applications, document_files,
    domain::{ApplicationScope, CreateApplicationRequest},
    overview, warehouse,
};

const LARGE_RECORD_COUNT: usize = 1_000;
const MAX_LIST_TIME: Duration = Duration::from_secs(5);
const MAX_OVERVIEW_TIME: Duration = Duration::from_secs(5);
const MAX_AGENT_TIME: Duration = Duration::from_secs(20);
const MAX_DIRECTORY_TIME: Duration = Duration::from_secs(10);

fn measured<T>(name: &str, budget: Duration, operation: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = operation();
    let elapsed = started.elapsed();
    eprintln!(
        "OfferTrack performance baseline: {name}={} ms (budget={} ms)",
        elapsed.as_millis(),
        budget.as_millis()
    );
    assert!(
        elapsed <= budget,
        "{name} took {} ms, exceeding the {} ms baseline budget",
        elapsed.as_millis(),
        budget.as_millis()
    );
    value
}

fn seed_large_warehouse(session: &mut warehouse::WarehouseSession) {
    let long_description = "岗位职责与任职要求。".repeat(200);
    let long_notes = "合成性能数据，仅用于测试。".repeat(160);
    let stages = [
        ("preparing", "准备投递", "application", 10, 0, None),
        ("applied", "已投递", "application", 20, 0, None),
        ("assessment", "在线测评", "assessment", 30, 0, None),
        ("written_exam", "现场/远程笔试", "written_exam", 40, 0, None),
        ("interview", "面试考核", "interview", 50, 0, None),
        ("interview_passed", "面试通过", "interview", 60, 0, None),
        ("signing", "待签约", "signing", 70, 0, None),
        ("offer", "offer✅️", "terminal", 80, 1, Some("offer")),
        ("failed_terminal", "已挂", "terminal", 90, 1, Some("failed")),
    ];
    let transaction = session.connection_mut().unwrap().transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO field_definitions
             (id,key,display_name,field_type,config_json,display_order,is_visible,created_at_utc,updated_at_utc)
             VALUES ('perf-field','custom_performance','性能字段','text','{}',100,1,?1,?1)",
            ["2026-01-01T00:00:00Z"],
        )
        .unwrap();
    for tag in 0..3 {
        transaction
            .execute(
                "INSERT INTO tags (id,name,color,created_at_utc,updated_at_utc,scope)
                 VALUES (?1,?2,?3,?4,?4,'record')",
                params![
                    format!("perf-tag-{tag}"),
                    format!("性能标签{tag}"),
                    format!("#3366{tag}{tag}"),
                    "2026-01-01T00:00:00Z"
                ],
            )
            .unwrap();
    }

    for index in 0..LARGE_RECORD_COUNT {
        let id = format!("00000000-0000-4000-8000-{index:012x}");
        let short_id = format!("P{index:05}");
        let created = format!(
            "2026-01-{:02}T{:02}:{:02}:00Z",
            index / 744 + 1,
            (index / 60) % 24,
            index % 60
        );
        let folder = format!("applications/performance-{index:04}");
        transaction
            .execute(
                "INSERT INTO applications
                 (id,short_id,created_at_utc,created_timezone_offset_minutes,company_name,
                  company_type,position_name,position_description,notes,folder_relative_path,
                  current_stage_state,status_updated_at_utc,updated_at_utc,industry,
                  position_category,work_location)
                 VALUES (?1,?2,?3,480,?4,'private',?5,?6,?7,?8,'awaitingResult',?3,?3,
                         '合成行业','研发','测试城市')",
                params![
                    id,
                    short_id,
                    created,
                    format!("性能测试公司 {index:04}"),
                    format!("工程师 {index:04}"),
                    long_description,
                    long_notes,
                    folder,
                ],
            )
            .unwrap();
        let mut current_stage_id = String::new();
        for (stage_index, (key, name, kind, order, terminal, outcome)) in stages.iter().enumerate()
        {
            let stage_id = format!("perf-stage-{index:04}-{stage_index:02}");
            transaction
                .execute(
                    "INSERT INTO workflow_stages
                     (id,application_id,stable_key,display_name,stage_kind,display_order,color,
                      is_terminal,terminal_outcome,created_at_utc,updated_at_utc)
                     VALUES (?1,?2,?3,?4,?5,?6,'#64748b',?7,?8,?9,?9)",
                    params![
                        stage_id, id, key, name, kind, order, terminal, outcome, created
                    ],
                )
                .unwrap();
            if *key == "interview" {
                current_stage_id = stage_id;
            }
        }
        transaction
            .execute(
                "UPDATE applications SET current_stage_id=?1 WHERE id=?2",
                params![current_stage_id, id],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO workflow_events
                 (id,application_id,stage_id,stage_name_snapshot,next_state,notes,
                  occurred_at_utc,actor_type)
                 VALUES (?1,?2,?3,'面试考核','awaitingResult','合成历史',?4,'user')",
                params![
                    format!("perf-event-{index:04}"),
                    id,
                    current_stage_id,
                    created
                ],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO field_values
                 (application_id,field_definition_id,value_json,updated_at_utc)
                 VALUES (?1,'perf-field',?2,?3)",
                params![id, format!("\"字段值 {index:04}\""), created],
            )
            .unwrap();
        for tag in 0..3 {
            transaction
                .execute(
                    "INSERT INTO application_tags (application_id,tag_id,display_order)
                     VALUES (?1,?2,?3)",
                    params![id, format!("perf-tag-{tag}"), tag * 10],
                )
                .unwrap();
        }
        for document in 0..3 {
            transaction
                .execute(
                    "INSERT INTO documents
                     (id,application_id,relative_path,display_name,media_type,size_bytes,
                      discovered_at_utc,last_observed_at_utc,modified_at_utc)
                     VALUES (?1,?2,?3,?4,'application/pdf',1024,?5,?5,?5)",
                    params![
                        format!("perf-document-{index:04}-{document}"),
                        id,
                        format!("简历/版本-{document}.pdf"),
                        format!("版本-{document}.pdf"),
                        created
                    ],
                )
                .unwrap();
        }
    }
    transaction.commit().unwrap();
}

#[test]
fn large_record_long_text_and_agent_projections_stay_within_baseline() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    seed_large_warehouse(&mut session);

    let records = measured("list 1000 records", MAX_LIST_TIME, || {
        applications::list(&session, ApplicationScope::Active).unwrap()
    });
    assert_eq!(records.len(), LARGE_RECORD_COUNT);
    assert_eq!(records[0].tags.len(), 3);
    assert_eq!(records[0].document_count, 3);
    assert_eq!(records[0].document_names.len(), 3);
    assert_eq!(records[0].custom_fields.len(), 1);
    assert_eq!(records[0].current_state_name, "待结果");

    let now: DateTime<FixedOffset> = "2026-09-04T12:00:00+08:00".parse().unwrap();
    let dashboard = measured("overview 1000 records", MAX_OVERVIEW_TIME, || {
        overview::get(&session, now).unwrap()
    });
    assert_eq!(dashboard.records.len(), LARGE_RECORD_COUNT);

    let dataset = measured(
        "Agent projection and JSON 1000 records",
        MAX_AGENT_TIME,
        || {
            let dataset = agent_access::collect(&session).unwrap();
            let encoded = agent_access::encode(&dataset).unwrap();
            assert!(encoded.len() > 10 * 1024 * 1024);
            dataset
        },
    );
    assert_eq!(dataset.applications.len(), LARGE_RECORD_COUNT);
    assert_eq!(dataset.applications[0].stages.len(), 9);
    assert_eq!(dataset.applications[0].documents.len(), 3);
}

#[test]
fn thousand_file_and_deep_directory_scan_stays_within_baseline() {
    let root = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(root.path()).unwrap();
    let detail = applications::create(
        &mut session,
        CreateApplicationRequest {
            company_name: "性能文件公司".into(),
            position_name: "目录扫描".into(),
            company_type: "private".into(),
            industry: "合成".into(),
            position_category: "测试".into(),
            work_location: "本机".into(),
        },
    )
    .unwrap();
    let folder = root.path().join(&detail.record.folder_relative_path);
    for index in 0..1_000 {
        fs::write(folder.join(format!("resume-{index:04}.pdf")), b"x").unwrap();
    }
    let mut deepest = folder.clone();
    for index in 0..48 {
        deepest.push(format!("d{index:02}"));
        fs::create_dir(&deepest).unwrap();
    }
    fs::write(deepest.join("deep.docx"), b"synthetic").unwrap();
    fs::create_dir(folder.join("empty-directory")).unwrap();

    let documents = measured("index 1001 files", MAX_DIRECTORY_TIME, || {
        applications::scan_documents(&mut session, &detail.record.id).unwrap()
    });
    assert_eq!(documents.iter().filter(|item| !item.missing).count(), 1_001);
    let directories = measured(
        "observe 48-level directory tree",
        MAX_DIRECTORY_TIME,
        || document_files::list_directories(&session, &detail.record.id).unwrap(),
    );
    assert_eq!(directories.directories.len(), 49);
    assert!(directories.directories.iter().any(|item| item.empty));
}
