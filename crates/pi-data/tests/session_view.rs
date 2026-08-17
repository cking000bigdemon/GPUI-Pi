use std::fs;

use pi_data::{RunningSessionOverlay, build_session_view, list_sessions};
use tempfile::tempdir;

#[test]
fn production_layout_builds_sorted_parent_child_view_with_overlay() {
    let root = tempdir().unwrap();
    let project = root.path().join("--project--");
    fs::create_dir(&project).unwrap();
    let parent = project.join("parent.jsonl");
    let child = project.join("child.jsonl");
    fs::write(
        &parent,
        concat!(
            "{\"type\":\"session\",\"id\":\"parent\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"C:\\\\fixture\\\\project\"}\n",
            "{\"type\":\"message\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":\"Parent title\"}}\n",
            "this is a malformed production line\n",
            "{\"type\":\"message\",\"timestamp\":\"2026-01-01T00:00:02Z\",\"message\":{\"role\":\"assistant\",\"usage\":{\"totalTokens\":42,\"cost\":{\"total\":0.25}}}}\n"
        ),
    )
    .unwrap();
    let parent_json = serde_json::to_string(&parent).unwrap();
    fs::write(
        &child,
        format!(
            "{{\"type\":\"session\",\"id\":\"child\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"C:\\\\fixture\\\\project\",\"parentSession\":{parent_json}}}\n{{\"type\":\"message\",\"timestamp\":\"2026-01-01T00:00:03Z\",\"message\":{{\"role\":\"user\",\"content\":\"Child title\"}}}}\n"
        ),
    )
    .unwrap();

    let listed = list_sessions(root.path());
    assert_eq!(listed.sessions.len(), 2);
    assert_eq!(listed.diagnostics.len(), 1);
    assert_eq!(listed.diagnostics[0].path, parent);
    assert!(listed.diagnostics[0].message.starts_with("第 3 行："));
    let groups = build_session_view(
        listed.sessions,
        &RunningSessionOverlay::new(["child".to_owned()]),
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].sessions[0].id, "parent");
    assert_eq!(groups[0].sessions[0].message_count, 2);
    assert_eq!(
        groups[0].sessions[0].metrics.recent_context_tokens,
        Some(42)
    );
    assert!(groups[0].sessions[0].children[0].running);
}
