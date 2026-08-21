use std::path::Path;

use pi_data::{list_sessions, scan_plugin_packages, scan_skills};

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

    let cwd = std::env::var_os("PI_DATA_TEST_REAL_CWD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let trust_lock = Path::new(&agent_dir).join("trust.json.lock");
    let lock_existed = trust_lock.exists();
    let skills = scan_skills(&agent_dir, &cwd, dirs::home_dir().as_deref());
    let plugins = scan_plugin_packages(&agent_dir, &cwd, dirs::home_dir().as_deref());
    assert_eq!(
        trust_lock.exists(),
        lock_existed,
        "只读扫描不得创建 trust lock"
    );
    println!(
        "real resource scan: skills={}, packages={}, diagnostics={}",
        skills.skills.len(),
        plugins.packages.len(),
        skills.diagnostics.len() + plugins.diagnostics.len()
    );
}
