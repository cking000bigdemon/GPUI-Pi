use std::path::Path;

use pi_data::list_sessions;

#[test]
fn scans_real_agent_directory_read_only() {
    let Some(agent_dir) = std::env::var_os("PI_DATA_TEST_REAL_AGENT_DIR") else {
        eprintln!("skip: set PI_DATA_TEST_REAL_AGENT_DIR to opt in");
        return;
    };
    let sessions = list_sessions(Path::new(&agent_dir).join("sessions"));
    println!(
        "real session scan: sessions={}, diagnostics={}",
        sessions.sessions.len(),
        sessions.diagnostics.len()
    );
    assert!(sessions.sessions.len() >= 20);
}
