use std::path::{Path, PathBuf};

fn collect_sessions(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = directory.read_dir() else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_sessions(&path, paths);
        } else if path.extension().is_some_and(|ext| ext == "jsonl")
            && path.file_name().is_some_and(|name| name != "session.jsonl")
        {
            paths.push(path);
        }
    }
}

#[test]
fn optional_real_agent_directory_is_read_only_and_renders_twenty_sessions() {
    let Some(root) = std::env::var_os("PI_DATA_TEST_REAL_AGENT_DIR").map(PathBuf::from) else {
        eprintln!("skip: PI_DATA_TEST_REAL_AGENT_DIR 未设置");
        return;
    };
    let sessions = if root.join("sessions").is_dir() {
        root.join("sessions")
    } else {
        root
    };
    let mut paths = Vec::new();
    collect_sessions(&sessions, &mut paths);
    paths.sort();
    assert!(paths.len() >= 20, "只找到 {} 个会话", paths.len());

    let before = paths
        .iter()
        .map(|path| {
            let metadata = path.metadata().unwrap();
            (path.clone(), metadata.len(), metadata.modified().ok())
        })
        .collect::<Vec<_>>();
    let mut rendered = 0;
    let mut blocks = 0;
    for path in &paths {
        if let Ok(document) = pi_render::render_path(path) {
            rendered += 1;
            blocks += document.stats().blocks;
        }
    }
    let after = paths
        .iter()
        .map(|path| {
            let metadata = path.metadata().unwrap();
            (path.clone(), metadata.len(), metadata.modified().ok())
        })
        .collect::<Vec<_>>();
    assert_eq!(before, after, "只读渲染改变了共享会话文件");
    assert!(rendered >= 20, "成功渲染的会话只有 {rendered} 个");
    println!(
        "R6 real sessions: scanned={}, rendered={}, blocks={blocks}",
        paths.len(),
        rendered
    );
}
