use std::path::PathBuf;

#[test]
fn semantic_fixture_matches_text_golden() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let document = pi_render::render_path(root.join("semantic.jsonl")).unwrap();
    let actual = document.text_snapshot().replace("\r\n", "\n");
    let expected = std::fs::read_to_string(root.join("semantic.golden.txt"))
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(actual, expected);

    let stats = document.stats();
    assert_eq!(stats.messages, 7);
    assert!(stats.blocks >= 16);
    assert_eq!(stats.tools, 5);
    assert_eq!(stats.images, 2);
}
