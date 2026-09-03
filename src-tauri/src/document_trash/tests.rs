use super::*;
use crate::{applications, domain::CreateApplicationRequest, warehouse};
use std::fs;
use tempfile::tempdir;

fn fixture() -> (
    tempfile::TempDir,
    WarehouseSession,
    crate::domain::ApplicationDetail,
) {
    let dir = tempdir().unwrap();
    let mut session = warehouse::create(dir.path()).unwrap();
    let record = applications::create(
        &mut session,
        CreateApplicationRequest {
            company_name: "测试公司".into(),
            position_name: "工程师".into(),
            company_type: "private".into(),
            industry: String::new(),
            position_category: String::new(),
            work_location: String::new(),
        },
    )
    .unwrap();
    let root = dir.path().join(&record.record.folder_relative_path);
    fs::create_dir(root.join("材料")).unwrap();
    fs::write(root.join("材料/简历.pdf"), b"resume").unwrap();
    applications::scan_documents(&mut session, &record.record.id).unwrap();
    let detail = applications::get(&session, &record.record.id).unwrap();
    (dir, session, detail)
}
fn request(detail: &crate::domain::ApplicationDetail) -> TrashRequest {
    let d = &detail.documents[0];
    TrashRequest {
        application_id: detail.record.id.clone(),
        document_id: d.id.clone(),
        expected_relative_path: d.relative_path.clone(),
    }
}

#[test]
fn trash_restore_preserves_id_and_never_overwrites_collision() {
    let (dir, mut session, detail) = fixture();
    let id = detail.documents[0].id.clone();
    let next = trash(&mut session, request(&detail)).unwrap();
    assert!(next.documents.is_empty());
    let entries = list(&session).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].document_id, id);
    let original = dir
        .path()
        .join(&detail.record.folder_relative_path)
        .join("材料/简历.pdf");
    fs::write(&original, b"new").unwrap();
    let restored = restore(&mut session, &entries[0].id).unwrap();
    assert!(restored.relocated);
    assert_eq!(restored.document_id, id);
    assert_eq!(fs::read(original).unwrap(), b"new");
    let detail = applications::get(&session, &detail.record.id).unwrap();
    assert_eq!(detail.documents[0].id, id);
    assert_eq!(
        fs::read(
            dir.path()
                .join(&detail.record.folder_relative_path)
                .join(restored.relative_path)
        )
        .unwrap(),
        b"resume"
    );
}

#[test]
fn trash_rejects_stale_missing_and_read_only_requests() {
    let (dir, mut session, detail) = fixture();
    let mut stale = request(&detail);
    stale.expected_relative_path = "wrong.pdf".into();
    assert!(matches!(
        trash(&mut session, stale),
        Err(CoreError::RevisionConflict)
    ));
    fs::remove_file(
        dir.path()
            .join(&detail.record.folder_relative_path)
            .join(&detail.documents[0].relative_path),
    )
    .unwrap();
    assert!(matches!(
        trash(&mut session, request(&detail)),
        Err(CoreError::FileMissing)
    ));
    drop(session);
    let mut readonly =
        warehouse::open(dir.path(), crate::warehouse::WarehouseAccessMode::ReadOnly).unwrap();
    assert!(matches!(
        trash(&mut readonly, request(&detail)),
        Err(CoreError::ReadOnlyWarehouse)
    ));
}

#[test]
fn restore_requires_live_parent_and_missing_parent_directory_falls_back_to_root() {
    let (dir, mut session, detail) = fixture();
    trash(&mut session, request(&detail)).unwrap();
    let item = list(&session).unwrap().remove(0);
    fs::remove_dir(
        dir.path()
            .join(&detail.record.folder_relative_path)
            .join("材料"),
    )
    .unwrap();
    let restored = restore(&mut session, &item.id).unwrap();
    assert!(restored.relocated);
    assert_eq!(restored.relative_path, "简历.pdf");
    trash(
        &mut session,
        TrashRequest {
            application_id: detail.record.id.clone(),
            document_id: item.document_id,
            expected_relative_path: "简历.pdf".into(),
        },
    )
    .unwrap();
    crate::recycle_bin::move_application_to_trash(&mut session, &detail.record.id).unwrap();
    let item = list(&session).unwrap().remove(0);
    assert!(item.parent_deleted);
    assert!(matches!(
        restore(&mut session, &item.id),
        Err(CoreError::NotFound)
    ));
}

#[test]
fn confirmed_cleanup_is_bound_to_set_identity_and_never_deletes_outside() {
    let (dir, mut session, detail) = fixture();
    trash(&mut session, request(&detail)).unwrap();
    let (confirmation, challenge) = cleanup::prepare(&session).unwrap();
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, b"safe").unwrap();
    let result = cleanup::purge(&mut session, confirmation, &challenge.confirmation_token).unwrap();
    assert_eq!(result.deleted_ids.len(), 1);
    assert!(outside.exists());
    assert!(list(&session).unwrap().is_empty());
    let (confirmation, _) = cleanup::prepare(&session).unwrap();
    assert!(matches!(
        cleanup::purge(&mut session, confirmation, "wrong"),
        Err(CoreError::InvalidConfirmation)
    ));
}

#[test]
fn cleanup_confirmation_expires_when_the_registered_set_changes() {
    let (dir, mut session, detail) = fixture();
    trash(&mut session, request(&detail)).unwrap();
    let (confirmation, challenge) = cleanup::prepare(&session).unwrap();
    let root = dir.path().join(&detail.record.folder_relative_path);
    fs::write(root.join("second.pdf"), b"second").unwrap();
    let docs = applications::scan_documents(&mut session, &detail.record.id).unwrap();
    let second = docs
        .iter()
        .find(|d| d.display_name == "second.pdf")
        .unwrap();
    trash(
        &mut session,
        TrashRequest {
            application_id: detail.record.id,
            document_id: second.id.clone(),
            expected_relative_path: second.relative_path.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        cleanup::purge(&mut session, confirmation, &challenge.confirmation_token),
        Err(CoreError::InvalidConfirmation)
    ));
    assert_eq!(list(&session).unwrap().len(), 2);
}

#[test]
fn interrupted_moves_reconcile_without_guessing() {
    let (dir, mut session, detail) = fixture();
    let request = request(&detail);
    let folder = detail.record.folder_relative_path.clone();
    let relative = request.expected_relative_path.clone();
    let trash_id = Uuid::new_v4().to_string();
    let source = dir.path().join(&folder).join(&relative);
    let target = trash_path(dir.path(), &trash_id).unwrap();
    let identity = files::file_identity(&source).unwrap();
    let doc = &detail.documents[0];
    session.connection_mut().unwrap().execute("INSERT INTO document_trash(id,version,document_id,application_id,relative_path,display_name,media_type,size_bytes,content_hash,discovered_at_utc,last_observed_at_utc,deleted_at_utc,state) VALUES(?1,1,?2,?3,?4,?5,NULL,NULL,NULL,'now','now','now','pending')",params![trash_id,doc.id,detail.record.id,relative,doc.display_name]).unwrap();
    let intent = Intent {
        id: Uuid::new_v4().to_string(),
        trash_id,
        kind: "trash".into(),
        folder,
        relative,
        identity,
        created: now(),
    };
    intent.persist(session.connection_mut().unwrap()).unwrap();
    files::rename_file_no_replace(&source, &target, &intent.identity).unwrap();
    recover(&mut session).unwrap();
    assert_eq!(list(&session).unwrap().len(), 1);
}
