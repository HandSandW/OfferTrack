use super::*;
use crate::{applications, domain::CreateApplicationRequest, warehouse};

#[test]
fn serialized_wire_limit_includes_both_structured_and_escaped_text_without_partial_output() {
    let response = success(
        json!("output-test"),
        tool_result(Ok(json!({"notes":"\n\"中".repeat(300)}))),
    );
    let mut bytes = Vec::new();
    send_with_limit(&mut bytes, response, 4096).unwrap();
    assert!(bytes.len() < 4096);
    assert_eq!(std::str::from_utf8(&bytes).unwrap().lines().count(), 1);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["id"], "output-test");
    assert_eq!(value["result"]["isError"], true);
    assert_eq!(
        value["result"]["structuredContent"]["error"]["code"],
        "AGENT_LIMIT"
    );
    let mut input = std::io::BufReader::with_capacity(
        7,
        std::io::Cursor::new(format!("{}\r\n", " ".repeat(framing::MAX_INPUT - 1))),
    );
    assert!(matches!(
        framing::read(&mut input).unwrap(),
        framing::Frame::Json(_)
    ));
    assert!(matches!(
        framing::read(&mut input).unwrap(),
        framing::Frame::End
    ));
}

fn init(version: &str) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":version,"capabilities":{},"clientInfo":{"name":"synthetic-client","version":"1"}}})
}
fn ready(server: &mut Server) {
    assert!(
        server
            .dispatch(init(PROTOCOL_VERSIONS[0]))
            .unwrap()
            .get("result")
            .is_some()
    );
    assert!(
        server
            .dispatch(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none()
    );
}
fn call(server: &mut Server, name: &str, args: Value) -> Value {
    server
        .dispatch(json!({"jsonrpc":"2.0","id":"query","method":"tools/call",
        "params":{"name":format!("offertrack_{name}"),"arguments":args}}))
        .unwrap()
}
fn fixture() -> (tempfile::TempDir, warehouse::WarehouseSession, String) {
    let temp = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(&temp.path().join("warehouse")).unwrap();
    let a = applications::create(
        &mut s,
        CreateApplicationRequest {
            company_name: "MCP 测试公司".into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: "".into(),
            position_category: "".into(),
            work_location: "".into(),
        },
    )
    .unwrap();
    (temp, s, a.record.id)
}
fn wire(input: &[u8]) -> (i32, Vec<Value>) {
    let mut output = Vec::new();
    let code = crate::agent_cli::run(
        [
            "--warehouse".into(),
            "X:/synthetic-not-a-warehouse".into(),
            "mcp".into(),
        ],
        &mut &*input,
        &mut output,
    );
    (
        code,
        std::str::from_utf8(&output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
    )
}

#[test]
fn lifecycle_negotiates_supported_versions_requires_initialized_and_keeps_read_only_capabilities() {
    for version in [PROTOCOL_VERSIONS[0], PROTOCOL_VERSIONS[1], "future-unknown"] {
        let mut server = Server::new(Path::new("X:/synthetic"));
        assert_eq!(
            call(&mut server, "summary", json!({}))["error"]["code"],
            -32002
        );
        let hello = server.dispatch(init(version)).unwrap();
        assert_eq!(
            hello["result"]["protocolVersion"],
            if version == "future-unknown" {
                PROTOCOL_VERSIONS[0]
            } else {
                version
            }
        );
        assert_eq!(
            hello["result"]["capabilities"],
            json!({"tools":{"listChanged":false}})
        );
        assert_eq!(
            call(&mut server, "summary", json!({}))["error"]["code"],
            -32002
        );
        assert!(server.dispatch(json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{"_meta":{}}})).is_none());
        assert_eq!(
            server.dispatch(init(version)).unwrap()["error"]["code"],
            -32600
        );
        let result = call(&mut server, "describe", json!({}));
        assert_eq!(
            result["result"]["structuredContent"]["data"]["write_enabled"],
            false
        );
        assert_eq!(
            result["result"]["structuredContent"]["data"]["transport"],
            "mcp-stdio"
        );
        let text: Value =
            serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, result["result"]["structuredContent"]);
    }
}

#[test]
fn catalogue_marks_controlled_write_and_never_accepts_arbitrary_authority() {
    let mut server = Server::new(Path::new("X:/synthetic"));
    ready(&mut server);
    let listed = server
        .dispatch(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}))
        .unwrap();
    let definitions = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(definitions.len(), 11);
    for definition in definitions {
        let write = definition["name"] == "offertrack_write";
        assert_eq!(definition["annotations"]["readOnlyHint"], !write);
        assert_eq!(definition["annotations"]["destructiveHint"], write);
        assert_eq!(definition["inputSchema"]["additionalProperties"], false);
    }
    for name in [
        "clear_trash",
        "delete_path",
        "execute_sql",
        "update_application",
        "set_write_enabled",
    ] {
        assert_eq!(call(&mut server, name, json!({}))["error"]["code"], -32602);
    }
    for (name, args) in [
        ("summary", json!({"operation":"clear_trash"})),
        ("summary", json!({"path":"private-user-path"})),
        ("summary", json!({"write_enabled":true})),
        ("get_application", json!({"id":3})),
        ("list_applications", json!({"scope":"trash"})),
        ("list_applications", json!({"limit":0})),
        ("list_tasks", json!({"offset":10001})),
        ("list_events", json!({"limit":201})),
        ("list_applications", json!({"search":"中".repeat(501)})),
    ] {
        let result = call(&mut server, name, args);
        assert_eq!(result["result"]["isError"], true, "{result}");
        assert_eq!(
            result["result"]["structuredContent"]["error"]["code"],
            "VALIDATION_FAILED"
        );
        assert!(!result.to_string().contains("private-user-path"));
    }
    assert_eq!(server.dispatch(json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"file:///outside"}})).unwrap()["error"]["code"],-32601);
    assert_eq!(
        server
            .dispatch(
                json!({"jsonrpc":"2.0","id":4,"method":"tools/list","params":{"cursor":"invalid"}})
            )
            .unwrap()["error"]["code"],
        -32602
    );
    assert_eq!(server.dispatch(json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"offertrack_summary","task":{}}})).unwrap()["error"]["code"],-32602);
}

#[test]
fn live_calls_read_latest_wal_and_preserve_database_backups_and_snapshot_directories() {
    let (_temp, mut s, id) = fixture();
    let mut server = Server::new(s.root());
    ready(&mut server);
    let changes = s.connection().total_changes();
    let first = call(&mut server, "get_application", json!({"id":id}));
    assert_eq!(
        first["result"]["structuredContent"]["data"]["result"]["company_name"],
        "MCP 测试公司"
    );
    assert_eq!(s.connection().total_changes(), changes);
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET notes='已提交的最新备注' WHERE id=?1",
            [&id],
        )
        .unwrap();
    assert_eq!(
        call(&mut server, "get_application", json!({"id":id}))["result"]["structuredContent"]["data"]
            ["result"]["notes"],
        "已提交的最新备注"
    );
    assert_eq!(
        std::fs::read_dir(s.root().join("backups/database"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(s.root().join("agent-access"))
            .unwrap()
            .count(),
        0
    );
    let root = s.root().to_owned();
    drop(s);
    let renamed = root.with_file_name("renamed");
    // An idle MCP connection must not retain Windows directory/database handles.
    std::fs::rename(&root, &renamed).unwrap();
    assert_eq!(
        call(&mut server, "summary", json!({}))["result"]["isError"],
        true
    );
    let other = warehouse::create(&root).unwrap();
    assert_eq!(
        call(&mut server, "summary", json!({}))["result"]["structuredContent"]["error"]["code"],
        "AGENT_WAREHOUSE_CHANGED"
    );
    drop(other);
}

#[test]
fn full_projection_filters_deleted_records_and_resolves_only_indexed_safe_documents() {
    let (_temp, mut s, id) = fixture();
    let a = applications::get(&s, &id).unwrap();
    let file = s
        .root()
        .join(&a.record.folder_relative_path)
        .join("测试简历.pdf");
    std::fs::write(&file, b"synthetic PDF fixture").unwrap();
    applications::scan_all_documents(&mut s).unwrap();
    let document = applications::get(&s, &id).unwrap().documents[0].id.clone();
    let mut server = Server::new(s.root());
    ready(&mut server);
    for (name, args) in [
        ("summary", json!({})),
        ("list_applications", json!({})),
        ("list_tasks", json!({})),
        ("list_events", json!({})),
        ("list_documents", json!({"application_id":id})),
    ] {
        assert_eq!(call(&mut server, name, args)["result"]["isError"], false);
    }
    let result = call(
        &mut server,
        "resolve_document",
        json!({"application_id":id,"document_id":document}),
    );
    assert_eq!(
        Path::new(
            result["result"]["structuredContent"]["data"]["result"]["resolved_path"]
                .as_str()
                .unwrap()
        ),
        std::fs::canonicalize(&file).unwrap()
    );
    assert!(!result.to_string().contains("synthetic PDF fixture"));
    assert_eq!(
        call(
            &mut server,
            "resolve_document",
            json!({"application_id":id,"document_id":"../outside"})
        )["result"]["isError"],
        true
    );
    crate::recycle_bin::move_application_to_trash(&mut s, &id).unwrap();
    assert_eq!(
        call(&mut server, "get_application", json!({"id":id}))["result"]["structuredContent"]["error"]
            ["code"],
        "NOT_FOUND"
    );
    assert_eq!(
        call(&mut server, "list_applications", json!({}))["result"]["structuredContent"]["data"]["result"]
            ["total"],
        0
    );
}

#[test]
fn framing_recovers_after_bad_utf8_duplicate_keys_batches_and_oversized_lines() {
    let mut bytes = Vec::new();
    for line in [b"\xff".as_slice(),b"{\"jsonrpc\":\"2.0\",\"id\":1,\"id\":2}",
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"params\":{\"_meta\":{\"x\":1,\"x\":2}}}",b"{} {}",b"[]"] {
        bytes.extend_from_slice(line); bytes.push(b'\n');
    }
    bytes.extend(vec![b' '; framing::MAX_INPUT + 1]);
    bytes.push(b'\n');
    // Use UTF-8 source text for the valid frame, not an ASCII-only byte literal.
    let valid = "{\"jsonrpc\":\"2.0\",\"id\":\"中文 ID\",\"method\":\"ping\"}\r\n";
    bytes.extend_from_slice(valid.as_bytes());
    let (code, output) = wire(&bytes);
    assert_eq!(code, 0);
    assert_eq!(output.len(), 7);
    for result in &output[..6] {
        assert!(result.get("error").is_some(), "{result}");
    }
    assert_eq!(
        output[6],
        json!({"jsonrpc":"2.0","id":"中文 ID","result":{}})
    );
    let (_, incomplete) = wire(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}");
    assert_eq!(incomplete[0]["error"]["code"], -32700);
}

#[test]
fn notifications_are_silent_and_stream_handshake_and_tools_call_work_without_gui() {
    let values = [
        init(PROTOCOL_VERSIONS[0]),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"offertrack_summary"}}),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}),
        json!({"jsonrpc":"2.0","method":"unknown/notification"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"offertrack_describe","_meta":{"progressToken":"test"}}}),
    ];
    let input = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let (code, output) = wire(input.as_bytes());
    assert_eq!(code, 0);
    assert_eq!(output.len(), 3);
    assert_eq!(output[1]["result"]["tools"].as_array().unwrap().len(), 11);
    assert_eq!(output[2]["result"]["structuredContent"]["ok"], true);
    assert!(!input.contains("write_enabled"));
}

#[test]
fn invalid_envelopes_and_initialization_parameters_are_rejected_without_state_transition() {
    let mut server = Server::new(Path::new("X:/synthetic"));
    for value in [
        json!(null),
        json!({"jsonrpc":"2.0","id":null,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":1.5,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":true,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":1,"method":"ping","params":[]}),
        json!({"jsonrpc":"1.0","id":1,"method":"ping"}),
    ] {
        assert_eq!(server.dispatch(value).unwrap()["error"]["code"], -32600);
    }
    for params in [
        json!({}),
        json!({"protocolVersion":"x","capabilities":{},"clientInfo":{}}),
        json!({"protocolVersion":"x","capabilities":false,"clientInfo":{"name":"a","version":"1"}}),
    ] {
        assert_eq!(
            server
                .dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":params}))
                .unwrap()["error"]["code"],
            -32602
        );
    }
    ready(&mut server);
}

#[test]
fn eof_and_broken_pipes_terminate_with_stable_exit_codes() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed output"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed input"))
        }
    }
    assert_eq!(
        run(Path::new("synthetic"), &mut &b""[..], &mut Vec::new()),
        0
    );
    assert_eq!(run(Path::new("synthetic"), &mut Broken, &mut Vec::new()), 2);
    assert_eq!(
        run(Path::new("synthetic"), &mut &b"{}\n"[..], &mut Broken),
        3
    );
}

#[test]
fn controlled_writes_require_permission_and_lock_and_are_idempotent_across_tools() {
    let (_temp, mut s, id) = fixture();
    let mut server = Server::new(s.root());
    ready(&mut server);
    let status = call(&mut server, "write_status", json!({}));
    assert_eq!(
        status["result"]["structuredContent"]["data"]["permission"]["enabled"],
        false
    );
    let request = json!({"version":1,"warehouse_id":s.summary().warehouse_id,"request_id":uuid::Uuid::new_v4(),"source":"mcp-test",
        "actions":[{"operation":"append_notes","application_id":id,"revision":applications::load_record(s.connection(),&id).unwrap().revision,"text":"只写一次"}]});
    assert_eq!(
        call(&mut server, "write", request.clone())["result"]["structuredContent"]["error"]["code"],
        "AGENT_WRITE_DISABLED"
    );
    crate::agent_write::settings::set(&mut s, true, 0).unwrap();
    assert_eq!(
        call(&mut server, "write", request.clone())["result"]["structuredContent"]["error"]["code"],
        "WAREHOUSE_LOCKED"
    );
    let path = s.root().to_owned();
    drop(s);
    let first = call(&mut server, "write", request.clone());
    assert_eq!(first["result"]["isError"], false, "{first}");
    let retry = call(&mut server, "write", request.clone());
    let mut first_commit = first["result"]["structuredContent"]["data"].clone();
    let mut retry_commit = retry["result"]["structuredContent"]["data"].clone();
    let first_observation = first_commit
        .as_object_mut()
        .unwrap()
        .remove("snapshot_status")
        .unwrap();
    let retry_observation = retry_commit
        .as_object_mut()
        .unwrap()
        .remove("snapshot_status")
        .unwrap();
    assert_eq!(first_commit, retry_commit); // immutable commit receipt, not the later observation clock
    assert_eq!(first_observation["state"], "current");
    assert_eq!(first_observation["published"], true);
    assert_eq!(retry_observation["published"], false);
    assert_eq!(first_observation["snapshot"], retry_observation["snapshot"]);
    for response in [&first, &retry] {
        let text: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(text, response["result"]["structuredContent"]);
        assert_eq!(response["result"]["isError"], false);
    }
    let freshness = call(&mut server, "snapshot_status", json!({}));
    assert_eq!(
        freshness["result"]["structuredContent"]["data"]["state"],
        "current"
    );
    assert_eq!(
        call(&mut server, "get_application", json!({"id":id}))["result"]["structuredContent"]["data"]
            ["result"]["notes"],
        "只写一次"
    );
    let mut wrong = request.clone();
    wrong["warehouse_id"] = json!(uuid::Uuid::new_v4());
    assert_eq!(
        call(&mut server, "write", wrong)["result"]["structuredContent"]["error"]["code"],
        "AGENT_WAREHOUSE_CHANGED"
    );
    let mut s = warehouse::open(&path, warehouse::WarehouseAccessMode::Write).unwrap();
    crate::agent_write::settings::set(&mut s, false, 1).unwrap();
    assert_eq!(
        call(&mut server, "write", request)["result"]["structuredContent"]["error"]["code"],
        "AGENT_WRITE_DISABLED"
    );
}
