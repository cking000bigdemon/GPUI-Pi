use std::fs;

use pi_data::{
    TrustError, has_trust_resources, read_project_trust_status, read_trust, trust_project,
};
use serde_json::json;
use tempfile::tempdir;

fn write_store(agent: &std::path::Path, value: serde_json::Value) {
    fs::write(
        agent.join("trust.json"),
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap()),
    )
    .unwrap();
}

#[test]
fn project_resource_matrix_and_atomic_preserving_trust_write() {
    for resource in [
        ".pi/extensions",
        ".pi/skills",
        ".pi/prompts",
        ".pi/themes",
        ".pi/settings.json",
        ".pi/SYSTEM.md",
        ".pi/APPEND_SYSTEM.md",
        ".agents/skills",
    ] {
        let project = tempdir().unwrap();
        let path = project.path().join(resource);
        if path.extension().is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "fixture").unwrap();
        } else {
            fs::create_dir_all(path).unwrap();
        }
        assert!(has_trust_resources(project.path(), None), "{resource}");
    }

    let agent = tempdir().unwrap();
    let project = tempdir().unwrap();
    let other = tempdir().unwrap();
    fs::create_dir_all(project.path().join(".pi/extensions")).unwrap();
    let other_key = dunce::canonicalize(other.path())
        .unwrap()
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    write_store(
        agent.path(),
        json!({other_key.clone(): false, "reserved": null}),
    );
    assert!(
        !read_project_trust_status(agent.path(), project.path(), None)
            .unwrap()
            .trusted
    );
    trust_project(agent.path(), project.path()).unwrap();
    assert!(
        read_project_trust_status(agent.path(), project.path(), None)
            .unwrap()
            .trusted
    );
    let stored = read_trust(agent.path()).unwrap();
    assert_eq!(stored[&other_key], false);
    assert!(stored["reserved"].is_null());
    assert!(!agent.path().join("trust.json.lock").exists());
    assert_eq!(agent.path().read_dir().unwrap().count(), 1);
}

#[test]
fn nearest_parent_decision_applies_and_invalid_values_fail() {
    let agent = tempdir().unwrap();
    let root = tempdir().unwrap();
    let parent = root.path().join("ParentCase");
    let child = parent.join("ChildCase");
    fs::create_dir_all(child.join(".pi/extensions")).unwrap();
    let parent = dunce::canonicalize(&parent).unwrap();
    let parent_key = parent.as_os_str().to_string_lossy().into_owned();
    write_store(agent.path(), json!({parent_key.clone(): true}));

    let status = read_project_trust_status(agent.path(), &child, None).unwrap();
    assert!(status.trusted);
    assert_eq!(status.decision_path.as_deref(), Some(parent.as_path()));

    write_store(agent.path(), json!({parent_key: "yes"}));
    assert!(matches!(
        read_project_trust_status(agent.path(), &child, None),
        Err(TrustError::InvalidDecision { .. })
    ));
}

#[cfg(windows)]
#[test]
fn windows_trust_key_keeps_canonical_native_spelling_and_case() {
    let agent = tempdir().unwrap();
    let root = tempdir().unwrap();
    let project = root.path().join("MixedCaseProject");
    fs::create_dir_all(project.join(".pi/extensions")).unwrap();

    trust_project(agent.path(), &project).unwrap();
    let canonical = dunce::canonicalize(&project).unwrap();
    let expected = canonical.as_os_str().to_string_lossy().into_owned();
    let stored = read_trust(agent.path()).unwrap();
    assert_eq!(
        stored.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec![&expected]
    );
    assert!(expected.contains("MixedCaseProject"));
    assert!(expected.contains('\\'));
}
