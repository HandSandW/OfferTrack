use super::*;
use crate::domain::CreateApplicationRequest;
use crate::warehouse::{self, WarehouseSession};
use std::path::Path;

fn fixture() -> (tempfile::TempDir, WarehouseSession, String) {
    let temp = tempfile::tempdir().unwrap();
    let mut session = warehouse::create(temp.path()).unwrap();
    let application = crate::applications::create(
        &mut session,
        CreateApplicationRequest {
            company_name: "只读 CLI 示例".into(),
            position_name: "工程师".into(),
            company_type: "private".into(),
            industry: "".into(),
            position_category: "".into(),
            work_location: "".into(),
        },
    )
    .unwrap();
    (temp, session, application.record.id)
}
fn call(root: &Path, body: Value) -> (i32, Value) {
    let mut out = Vec::new();
    let code = run(
        [
            OsString::from("--warehouse"),
            root.as_os_str().to_owned(),
            OsString::from("query"),
        ],
        &mut serde_json::to_vec(&body).unwrap().as_slice(),
        &mut out,
    );
    let text = std::str::from_utf8(&out).unwrap();
    assert_eq!(text.lines().count(), 1);
    (code, serde_json::from_slice(&out).unwrap())
}

#[test]
fn phase_three_acceptance_fixture_supports_snapshot_and_cli_document_queries() {
    let (temp, mut session, id) = fixture();
    let application = crate::applications::get(&session, &id).unwrap();
    std::fs::write(
        session
            .root()
            .join(&application.record.folder_relative_path)
            .join("acceptance-resume.pdf"),
        b"OFFERTRACK SYNTHETIC ACCEPTANCE DOCUMENT - NOT A REAL RESUME",
    )
    .unwrap();
    crate::applications::scan_all_documents(&mut session).unwrap();
    crate::agent_access::create(&session).unwrap();
    let (code, docs) = call(
        session.root(),
        json!({"version":1,"request":{"operation":"list_documents","application_id":id}}),
    );
    assert_eq!(code, 0);
    assert_eq!(docs["data"]["result"].as_array().unwrap().len(), 1);
    let document_id = &docs["data"]["result"][0]["id"];
    let (code, resolved) = call(
        session.root(),
        json!({"version":1,"request":{"operation":"resolve_document","application_id":id,"document_id":document_id}}),
    );
    assert_eq!(code, 0);
    assert!(
        resolved["data"]["result"]["relative_path"]
            .as_str()
            .unwrap()
            .ends_with("/acceptance-resume.pdf")
    );
    drop(session);
    // Explicit developer-only handoff for a subsequent native client smoke.
    // No caller path accepted, no production command and no test is skipped.
    if std::env::var("OFFERTRACK_KEEP_ACCEPTANCE_FIXTURE").as_deref() == Ok("1") {
        println!("OFFERTRACK_ACCEPTANCE_FIXTURE={}", temp.keep().display());
    }
}

#[test]
fn live_cli_reads_committed_wal_while_gui_writer_remains_open_without_scan_or_backup() {
    let (_temp, mut s, id) = fixture();
    let backups_before = std::fs::read_dir(s.root().join("backups/database"))
        .unwrap()
        .count();
    let changes_before = s.connection().total_changes();
    let (code, result) = call(
        s.root(),
        json!({"version":1,"request":{"operation":"get_application","id":id}}),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["data"]["result"]["company_name"], "只读 CLI 示例");
    assert_eq!(s.connection().total_changes(), changes_before);
    assert_eq!(
        std::fs::read_dir(s.root().join("backups/database"))
            .unwrap()
            .count(),
        backups_before
    );
    assert_eq!(
        std::fs::read_dir(s.root().join("agent-access"))
            .unwrap()
            .count(),
        0
    );
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET notes='刚刚提交的 WAL 内容' WHERE id=?1",
            [&id],
        )
        .unwrap();
    let (code, next) = call(
        s.root(),
        json!({"version":1,"request":{"operation":"get_application","id":id}}),
    );
    assert_eq!(code, 0, "{next}");
    assert_eq!(next["data"]["result"]["notes"], "刚刚提交的 WAL 内容");
    let read = open(s.root()).unwrap();
    assert!(
        read.session
            .connection()
            .execute("UPDATE applications SET notes='forbidden'", [])
            .is_err()
    );
}

#[test]
fn unknown_fields_versions_mutations_and_raw_commands_are_rejected_without_opening() {
    let nonexistent = Path::new("X:/synthetic-nonexistent-warehouse");
    for body in [
        json!({"version":2,"request":{"operation":"summary"}}),
        json!({"version":1,"request":{"operation":"summary"},"sql":"DELETE FROM applications"}),
        json!({"version":1,"request":{"operation":"summary","path":"outside"}}),
        json!({"version":1,"request":{"operation":"clear_trash"}}),
        json!({"version":1,"request":{"operation":"update_application","write_enabled":true}}),
        json!({"version":1,"request":{"operation":"list_applications","scope":"trash"}}),
    ] {
        let (code, result) = call(nonexistent, body);
        assert_eq!(code, 2);
        assert!(
            ["VALIDATION_FAILED", "AGENT_VERSION_UNSUPPORTED"]
                .contains(&result["error"]["code"].as_str().unwrap())
        );
        assert!(!result.to_string().contains("synthetic-nonexistent"));
    }
    let mut out = Vec::new();
    assert_eq!(run([OsString::from("--help")], &mut &b""[..], &mut out), 0);
    let help: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(help["data"]["contract"]["write_enabled"], false);
    let mut output = Vec::new();
    assert_eq!(
        run(
            [
                "--warehouse".into(),
                nonexistent.as_os_str().into(),
                "query".into()
            ],
            &mut vec![b' '; MAX_INPUT as usize + 1].as_slice(),
            &mut output
        ),
        2
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output).unwrap()["error"]["code"],
        "AGENT_LIMIT"
    );
}

#[test]
fn old_schema_is_rejected_without_migration_or_backup() {
    let (_temp, mut s, _) = fixture();
    crate::migrations::fixture_remove_migration_nine(s.connection_mut().unwrap());
    let (code, result) = call(
        s.root(),
        json!({"version":1,"request":{"operation":"summary"}}),
    );
    assert_eq!(code, 2, "{result}");
    let version: i64 = s
        .connection()
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 8);
    assert_eq!(
        std::fs::read_dir(s.root().join("backups/database"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn requests_require_one_utf8_json_value_and_output_failures_have_distinct_exit_status() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed pipe"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(run(["--help".into()], &mut &b""[..], &mut Broken), 3);
    for input in [
        b"{} {}".as_slice(),
        b"\xff",
        b"{\"version\":1,\"version\":1,\"request\":{\"operation\":\"summary\"}}",
    ] {
        let mut output = Vec::new();
        assert_eq!(
            run(
                ["--warehouse".into(), "X:/synthetic".into(), "query".into()],
                &mut &*input,
                &mut output
            ),
            2
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&output).unwrap()["error"]["code"],
            "VALIDATION_FAILED"
        );
    }
}

#[test]
fn moved_warehouse_resolves_new_paths_and_never_reuses_snapshot_absolute_paths() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    let moved = temp.path().join("moved");
    let mut s = warehouse::create(&original).unwrap();
    let a = crate::applications::create(
        &mut s,
        CreateApplicationRequest {
            company_name: "迁移示例".into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: "".into(),
            position_category: "".into(),
            work_location: "".into(),
        },
    )
    .unwrap();
    let relative = format!("{}/简历.pdf", a.record.folder_relative_path);
    std::fs::write(s.root().join(&relative), b"synthetic").unwrap();
    crate::applications::scan_all_documents(&mut s).unwrap();
    let doc = crate::applications::get(&s, &a.record.id)
        .unwrap()
        .documents[0]
        .id
        .clone();
    agent_access::create(&s).unwrap();
    drop(s);
    std::fs::rename(&original, &moved).unwrap();
    let (code, result) = call(
        &moved,
        json!({"version":1,"request":{"operation":"resolve_document","application_id":a.record.id,"document_id":doc}}),
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["data"]["result"]["relative_path"], relative);
    assert_eq!(
        Path::new(result["data"]["result"]["resolved_path"].as_str().unwrap()),
        std::fs::canonicalize(moved.join(relative)).unwrap()
    );
}

#[cfg(windows)]
#[test]
fn cli_rejects_warehouse_junction_before_canonicalization() {
    let (temp, s, _) = fixture();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("warehouse-link");
    let output = std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command",
        "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:OFFERTRACK_TEST_LINK -Target $env:OFFERTRACK_TEST_TARGET | Out-Null"])
        .env("OFFERTRACK_TEST_LINK",&link).env("OFFERTRACK_TEST_TARGET",temp.path()).output().unwrap();
    assert!(output.status.success());
    let (code, result) = call(
        &link,
        json!({"version":1,"request":{"operation":"summary"}}),
    );
    assert_eq!(code, 2);
    assert_eq!(result["error"]["code"], "UNSAFE_PATH_REJECTED");
    assert_eq!(
        std::fs::read_dir(s.root().join("agent-access"))
            .unwrap()
            .count(),
        0
    );
}
