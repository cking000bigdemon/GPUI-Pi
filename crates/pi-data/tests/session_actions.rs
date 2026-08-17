use std::fs;
use std::io::Write as _;
use std::path::Path;

use pi_data::{
    SessionActionError, delete_leaf_session, export_session_jsonl, list_sessions, load_session,
    rename_session,
};
use tempfile::tempdir;

fn session(path: &Path, id: &str, parent: Option<&Path>) {
    let mut header = serde_json::json!({
        "type": "session",
        "version": 3,
        "id": id,
        "timestamp": "2026-01-01T00:00:00Z",
        "cwd": "C:\\fixture\\project"
    });
    if let Some(parent) = parent {
        header["parentSession"] = serde_json::Value::String(parent.display().to_string());
    }
    fs::write(
        path,
        format!("{}\n", serde_json::to_string(&header).unwrap()),
    )
    .unwrap();
}

#[test]
fn rename_delete_and_unicode_export_obey_safety_rules() {
    let root = tempdir().unwrap();
    let project = root.path().join("--fixture--");
    fs::create_dir(&project).unwrap();
    let parent_path = project.join("parent.jsonl");
    let child_path = project.join("child.jsonl");
    session(&parent_path, "parent", None);
    session(&child_path, "child", Some(&parent_path));

    let listed = list_sessions(root.path());
    let parent = listed
        .sessions
        .iter()
        .find(|item| item.id == "parent")
        .unwrap();
    let child = listed
        .sessions
        .iter()
        .find(|item| item.id == "child")
        .unwrap();

    assert!(matches!(
        delete_leaf_session(parent, &listed.sessions, false),
        Err(SessionActionError::HasChildren)
    ));
    assert!(matches!(
        rename_session(child, "blocked", true),
        Err(SessionActionError::Running)
    ));

    let original = fs::read(&child.path).unwrap();
    rename_session(child, " 新\r\n标题 ", false).unwrap();
    let renamed = fs::read(&child.path).unwrap();
    assert!(renamed.starts_with(&original));
    let parsed = load_session(&child.path).unwrap();
    assert!(matches!(
        parsed.entries.last().unwrap(),
        pi_data::SessionEntry::SessionInfo { name, .. } if name.as_deref() == Some("新  标题")
    ));
    let entry_id = match parsed.entries.last().unwrap() {
        pi_data::SessionEntry::SessionInfo { base, .. } => base.id.as_deref().unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(entry_id.len(), 8);
    assert!(entry_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(project.read_dir().unwrap().count(), 2);

    let refreshed = list_sessions(root.path());
    let child = refreshed
        .sessions
        .iter()
        .find(|item| item.id == "child")
        .unwrap();
    let expected = fs::read(&child.path).unwrap();
    let exported = root.path().join("导出").join("原始会话.jsonl");
    export_session_jsonl(child, &exported).unwrap();
    assert_eq!(fs::read(exported).unwrap(), expected);

    let refreshed = list_sessions(root.path());
    let child = refreshed
        .sessions
        .iter()
        .find(|item| item.id == "child")
        .unwrap();
    delete_leaf_session(child, &refreshed.sessions, false).unwrap();
    assert!(!child.path.exists());
}

#[test]
fn rename_and_delete_reject_a_source_changed_after_scan() {
    let root = tempdir().unwrap();
    let project = root.path().join("--fixture--");
    fs::create_dir(&project).unwrap();
    let path = project.join("current.jsonl");
    session(&path, "session", None);

    let listed = list_sessions(root.path());
    let summary = listed.sessions.first().unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"type\":\"custom\",\"id\":\"concurrent\"}\n")
        .unwrap();

    assert!(matches!(
        rename_session(summary, "stale", false),
        Err(SessionActionError::ConcurrentModification)
    ));
    assert!(matches!(
        delete_leaf_session(summary, &listed.sessions, false),
        Err(SessionActionError::ConcurrentModification)
    ));
    assert!(path.exists());
    assert!(fs::read_to_string(path).unwrap().contains("concurrent"));
}
