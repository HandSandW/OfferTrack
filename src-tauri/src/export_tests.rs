use super::*;
use crate::{
    domain::CreateApplicationRequest,
    warehouse::{self, WarehouseAccessMode},
};
use std::io::{Cursor, Read};

fn create_record(s: &mut WarehouseSession, name: &str) -> ApplicationListItem {
    applications::create(
        s,
        CreateApplicationRequest {
            company_name: name.into(),
            position_name: "研发".into(),
            company_type: "private".into(),
            industry: "".into(),
            position_category: "".into(),
            work_location: "".into(),
        },
    )
    .unwrap()
    .record
}
fn request() -> Request {
    Request {
        version: 1,
        scope: Scope::All {},
        columns: vec!["companyName".into(), "notes".into()],
        format: FileFormat::Csv,
    }
}

#[test]
fn ranges_include_archive_exclude_deleted_and_never_expand_empty_or_stale_selection() {
    let temp = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(temp.path()).unwrap();
    let a = create_record(&mut s, "A");
    let b = create_record(&mut s, "B");
    let c = create_record(&mut s, "C");
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET archived_at_utc=created_at_utc WHERE id=?1",
            [&b.id],
        )
        .unwrap();
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET deleted_at_utc=created_at_utc WHERE id=?1",
            [&c.id],
        )
        .unwrap();
    assert_eq!(project(&s, &request()).unwrap().rows.len(), 2);
    let mut req = request();
    req.scope = Scope::Records {
        partition: Partition::Active,
        targets: vec![],
    };
    assert!(project(&s, &req).unwrap().rows.is_empty());
    req.scope = Scope::Records {
        partition: Partition::Active,
        targets: vec![Target {
            id: a.id.clone(),
            revision: a.revision,
        }],
    };
    assert_eq!(project(&s, &req).unwrap().rows[0][0], "A");
    for (id, revision) in [
        (a.id.clone(), a.revision + 1),
        (b.id, b.revision),
        (c.id, c.revision),
        ("unknown".into(), 1),
    ] {
        req.scope = Scope::Records {
            partition: Partition::Active,
            targets: vec![Target { id, revision }],
        };
        assert!(project(&s, &req).is_err());
    }
    req.scope = Scope::Records {
        partition: Partition::Active,
        targets: vec![
            Target {
                id: a.id.clone(),
                revision: a.revision
            };
            2
        ],
    };
    assert!(matches!(project(&s, &req), Err(CoreError::Validation)));
}

#[test]
fn mapping_keeps_custom_types_long_text_and_explicit_index_paths_without_reading_files() {
    let temp = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(temp.path()).unwrap();
    let a = create_record(&mut s, "示例");
    let fields = applications::save_field_definition(
        &mut s,
        crate::domain::FieldDefinitionRequest {
            id: None,
            revision: None,
            display_name: "隐藏数值".into(),
            field_type: "number".into(),
            config: serde_json::json!({}),
        },
    )
    .unwrap();
    let field = &fields[0];
    s.connection_mut()
        .unwrap()
        .execute(
            "INSERT INTO field_values (application_id,field_definition_id,value_json,updated_at_utc) VALUES (?1,?2,'12.5','2026-09-03T00:00:00Z')",
            rusqlite::params![a.id, field.id],
        )
        .unwrap();
    let folder = s.root().join(&a.folder_relative_path);
    fs::create_dir(folder.join("子目录")).unwrap();
    fs::write(folder.join("子目录/简历.pdf"), b"not read by export").unwrap();
    applications::scan_all_documents(&mut s).unwrap();
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE documents SET missing_at_utc='2026-09-01T00:00:00Z' WHERE application_id=?1",
            [&a.id],
        )
        .unwrap();
    let long = "岗位说明\n".repeat(9000);
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE applications SET notes=?1 WHERE id=?2",
            rusqlite::params![long, a.id],
        )
        .unwrap();
    let mut req = request();
    req.columns = vec![
        format!("custom:{}", field.id),
        "notes".into(),
        "documentNames".into(),
        "documentPaths".into(),
    ];
    let table = project(&s, &req).unwrap();
    assert_eq!(table.rows[0][0], "12.5");
    assert_eq!(table.rows[0][1], long);
    assert_eq!(table.manifest.columns[0].field_type, "number");
    assert_eq!(table.rows[0][2], "简历.pdf [缺失]");
    assert_eq!(
        table.rows[0][3],
        format!("{} [缺失]", folder.join("子目录/简历.pdf").display())
    );
    req.format = FileFormat::Xlsx;
    assert!(matches!(project(&s, &req), Err(CoreError::ExportLimit)));
    req.format = FileFormat::Csv;
    s.connection_mut()
        .unwrap()
        .execute(
            "UPDATE documents SET relative_path='../escape.pdf' WHERE application_id=?1",
            [&a.id],
        )
        .unwrap();
    assert!(matches!(project(&s, &req), Err(CoreError::UnsafePath)));
    req.columns = vec!["companyName".into()];
    assert!(
        !String::from_utf8(csv(&project(&s, &req).unwrap()))
            .unwrap()
            .contains(&s.root().to_string_lossy().to_string())
    );
}

#[test]
fn csv_quotes_unicode_multiline_bom_and_formula_prefixes() {
    assert_eq!(
        csv_cell("引号\"和,换行\n下一行"),
        "\"引号\"\"和,换行\n下一行\""
    );
    for value in [
        "=1+1",
        "+cmd",
        "-2",
        "@SUM(A1)",
        "\t=1",
        " \n=1",
        "\u{feff}=1",
    ] {
        assert!(csv_cell(value).starts_with("\"'"));
    }
    let table = Table {
        manifest: Manifest {
            version: 1,
            generated_at_utc: "now".into(),
            format: FileFormat::Csv,
            row_count: 1,
            columns: vec![Column {
                key: "k".into(),
                label: "=恶意表头".into(),
                field_type: "text".into(),
            }],
            cell_encoding: "text",
            csv_formula_protection: "apostrophe",
            document_source: "index",
        },
        rows: vec![vec!["中文,\"说明\"\n第二行".into()]],
    };
    let encoded = String::from_utf8(csv(&table)).unwrap();
    assert!(encoded.starts_with("\u{feff}\"'=恶意表头\"\r\n"));
    assert!(encoded.ends_with("\"中文,\"\"说明\"\"\n第二行\"\r\n"));
}

#[test]
fn xlsx_uses_literal_cells_and_contains_mapping_freeze_filter_and_no_formulas() {
    let temp = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(temp.path()).unwrap();
    create_record(&mut s, "=HYPERLINK(\"https://example.invalid\")");
    let mut req = request();
    req.format = FileFormat::Xlsx;
    let bytes = xlsx(&project(&s, &req).unwrap()).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("xl/worksheets/sheet1.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(!xml.contains("<f>"));
    assert!(xml.contains("t=\"s\""));
    assert!(xml.contains("autoFilter"));
    assert!(xml.contains("frozen"));
    assert!(archive.by_name("xl/worksheets/sheet2.xml").is_ok());
    xml.clear();
    archive
        .by_name("xl/sharedStrings.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("=HYPERLINK"));
    assert!(xml.contains("companyName"));
}

#[test]
fn readonly_export_publishes_new_directory_keeps_source_and_prior_output_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let mut s = warehouse::create(temp.path()).unwrap();
    create_record(&mut s, "源数据");
    drop(s);
    let s = warehouse::open(temp.path(), WarehouseAccessMode::ReadOnly).unwrap();
    let output = tempfile::tempdir().unwrap();
    let before = fs::read(s.root().join("offertrack.sqlite")).unwrap();
    let first = create(&s, output.path(), &request()).unwrap();
    let content = fs::read(&first.path).unwrap();
    let second = create(&s, output.path(), &request()).unwrap();
    assert_ne!(first.path, second.path);
    assert_eq!(content, fs::read(&first.path).unwrap());
    assert_eq!(first.row_count, 1);
    assert_eq!(
        before,
        fs::read(s.root().join("offertrack.sqlite")).unwrap()
    );
    let mapping: serde_json::Value =
        serde_json::from_slice(&fs::read(&first.mapping_path).unwrap()).unwrap();
    assert_eq!(mapping["version"], 1);
    assert_eq!(mapping["columns"][0]["key"], "companyName");
    assert!(matches!(
        create(&s, s.root(), &request()),
        Err(CoreError::UnsafePath)
    ));
    assert!(matches!(
        create(&s, &s.root().join("applications"), &request()),
        Err(CoreError::UnsafePath)
    ));
}

#[test]
fn export_validates_dto_fields_limits_and_does_not_create_output_on_error() {
    let temp = tempfile::tempdir().unwrap();
    let s = warehouse::create(temp.path()).unwrap();
    let output = tempfile::tempdir().unwrap();
    let mut req = request();
    req.version = 2;
    assert!(create(&s, output.path(), &req).is_err());
    req.version = 1;
    for keys in [
        vec![],
        vec!["companyName".into(); 2],
        vec!["unknown".into()],
        vec!["companyName".into(); 257],
    ] {
        req.columns = keys;
        assert!(create(&s, output.path(), &req).is_err());
    }
    assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    assert!(serde_json::from_value::<Request>(serde_json::json!({"version":1,"format":"csv","scope":{"kind":"all"},"columns":["id"],"sql":"DELETE"})).is_err());
    assert!(serde_json::from_value::<Request>(serde_json::json!({"version":1,"format":"csv","scope":{"kind":"all","path":"evil"},"columns":["id"]})).is_err());
}

#[cfg(windows)]
#[test]
fn destination_junction_is_rejected_without_touching_its_target() {
    let temp = tempfile::tempdir().unwrap();
    let s = warehouse::create(temp.path()).unwrap();
    let output = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let link = output.path().join("junction");
    let status = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
                link.display(),
                outside.path().display()
            ),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(matches!(
        create(&s, &link, &request()),
        Err(CoreError::UnsafePath)
    ));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}
