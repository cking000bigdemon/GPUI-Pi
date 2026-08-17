use std::path::PathBuf;

use pi_render::render_path;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../pi-data/tests/fixtures/sessions")
}

#[test]
fn all_redacted_real_fixtures_render_without_panicking() {
    let mut paths = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 24, "R3 fixture 数量意外变化");

    let mut entries = 0;
    let mut blocks = 0;
    let mut tools = 0;
    let mut images = 0;
    let mut diagnostics = 0;
    for path in &paths {
        let document =
            render_path(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let stats = document.stats();
        entries += stats.messages;
        blocks += stats.blocks;
        tools += stats.tools;
        images += stats.images;
        diagnostics += stats.diagnostics;
        assert!(!document.session_id.is_empty());
    }
    println!(
        "R6 fixtures: files={}, messages={}, blocks={}, tools={}, images={}, diagnostics={}",
        paths.len(),
        entries,
        blocks,
        tools,
        images,
        diagnostics
    );
    assert!(entries >= 24);
    assert!(blocks >= entries);
}
