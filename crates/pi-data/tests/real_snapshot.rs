use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use pi_data::{group_sessions, list_sessions, load_session, read_session_summary, resolve_project};
use tempfile::tempdir;

const FIXTURE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sessions");

#[test]
fn all_redacted_real_sessions_parse_without_panicking() {
    let root = Path::new(FIXTURE_ROOT);
    let mut fixtures: Vec<PathBuf> = fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    fixtures.sort();
    assert!(fixtures.len() >= 20, "fixture 数量不足: {}", fixtures.len());

    for path in &fixtures {
        let session =
            load_session(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(session.header.id.starts_with("fixture-session-"));
        assert!(session.header.cwd.starts_with(r"C:\fixture\project-"));
        let _summary = read_session_summary(path).unwrap();
        let contents = fs::read_to_string(path).unwrap();
        for marker in [
            "ZhuanZ",
            "variFlight",
            "https://",
            "http://",
            "apiKey",
            "aig_",
            "sk-",
        ] {
            assert!(
                !contents.contains(marker),
                "{} 泄露 marker {marker}",
                path.display()
            );
        }
    }

    let listed = list_sessions(root);
    // fixture 直接放在扫描根目录；生产目录则是 `<project>/<session>.jsonl`。
    assert!(listed.sessions.is_empty());
    assert!(listed.diagnostics.is_empty());
}

#[test]
fn session_listing_follows_pi_default_layout_and_resolves_parent_ids() {
    let temp = tempdir().unwrap();
    let project = temp.path().join("--fixture--");
    fs::create_dir(&project).unwrap();
    let parent = project.join("parent.jsonl");
    let child = project.join("child.jsonl");
    fs::write(
        &parent,
        "{\"type\":\"session\",\"version\":3,\"id\":\"parent\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/fixture\"}\n",
    )
    .unwrap();
    fs::write(
        &child,
        format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"child\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/fixture\",\"parentSession\":{}}}\n",
            serde_json::to_string(&parent).unwrap()
        ),
    )
    .unwrap();
    let nested = project.join("child").join("run-0");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("session.jsonl"),
        "{\"type\":\"session\",\"id\":\"nested\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/fixture\"}\n",
    )
    .unwrap();

    let listed = list_sessions(temp.path());
    assert_eq!(listed.sessions.len(), 2);
    let child = listed
        .sessions
        .iter()
        .find(|session| session.id == "child")
        .unwrap();
    assert_eq!(child.parent_session_id.as_deref(), Some("parent"));
}

#[test]
fn real_fixture_summaries_group_by_normalized_projects() {
    let summaries: Vec<_> = fs::read_dir(FIXTURE_ROOT)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .map(|path| read_session_summary(path).unwrap())
        .collect();
    let groups = group_sessions(summaries);
    assert_eq!(groups.len(), 4);
    assert!(groups.iter().all(|group| !group.sessions.is_empty()));
}

#[test]
fn linked_worktree_resolves_to_main_project_root() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let linked = temp.path().join("linked");
    run(temp.path(), ["init", repo.to_str().unwrap()]);
    run(&repo, ["config", "user.name", "Pi Data Test"]);
    run(&repo, ["config", "user.email", "pi-data@example.invalid"]);
    run(&repo, ["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("README.md"), "# fixture\n").unwrap();
    run(&repo, ["add", "README.md"]);
    run(&repo, ["commit", "-m", "initial"]);
    run(
        &repo,
        [
            "worktree",
            "add",
            "-b",
            "feature/test",
            linked.to_str().unwrap(),
        ],
    );

    let main_project = resolve_project(&repo);
    let linked_project = resolve_project(&linked);
    assert!(main_project.is_top_level);
    assert!(!main_project.is_worktree);
    assert!(linked_project.is_top_level);
    assert!(linked_project.is_worktree);
    assert_eq!(linked_project.branch.as_deref(), Some("feature/test"));
    assert_eq!(main_project.project_key, linked_project.project_key);
    assert_eq!(main_project.project_root, linked_project.project_root);

    let subdirectory = linked.join("nested");
    fs::create_dir(&subdirectory).unwrap();
    let nested_project = resolve_project(&subdirectory);
    assert!(!nested_project.is_top_level);
    assert_eq!(nested_project.project_root, subdirectory);
}

#[test]
fn grouping_merges_main_and_linked_worktree_sessions() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let linked = temp.path().join("linked");
    run(temp.path(), ["init", repo.to_str().unwrap()]);
    run(&repo, ["config", "user.name", "Pi Data Test"]);
    run(&repo, ["config", "user.email", "pi-data@example.invalid"]);
    run(&repo, ["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("README.md"), "# fixture\n").unwrap();
    run(&repo, ["add", "README.md"]);
    run(&repo, ["commit", "-m", "initial"]);
    run(
        &repo,
        ["worktree", "add", "-b", "r3", linked.to_str().unwrap()],
    );

    let base = read_session_summary(Path::new(FIXTURE_ROOT).join("session-01.jsonl")).unwrap();
    let mut main = base.clone();
    main.id = "main".to_owned();
    main.cwd = repo;
    main.modified = UNIX_EPOCH + Duration::from_secs(1);
    let mut worktree = base;
    worktree.id = "linked".to_owned();
    worktree.cwd = linked;
    worktree.modified = UNIX_EPOCH + Duration::from_secs(2);

    let groups = group_sessions([main, worktree]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].sessions[0].session.id, "linked");
}

fn run<const N: usize>(cwd: &Path, args: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git 失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
