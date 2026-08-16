use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use pi_rpc::{
    BashResult, Client, ClientConfig, ClientEvent, Command, LifecycleEvent, PINNED_PI_VERSION,
    QueueMode, RpcSessionState, ThinkingLevel,
};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(20);

fn configured_binary() -> PathBuf {
    env::var_os("PI_RPC_TEST_BINARY")
        .map(PathBuf::from)
        .expect("PI_RPC_TEST_BINARY must point to the official pi 0.84.2 binary")
}

fn assert_version(binary: &Path) {
    let output = ProcessCommand::new(binary)
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        PINNED_PI_VERSION
    );
}

fn client() -> Client {
    let binary = configured_binary();
    assert_version(&binary);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let session_dir = env::current_dir()
        .unwrap()
        .join("target")
        .join("pi-rpc-tests")
        .join("real-sessions")
        .join(format!("{}-{nonce}", std::process::id()));
    fs::create_dir_all(&session_dir).unwrap();
    let agent_dir = session_dir.join("agent");
    fs::create_dir_all(&agent_dir).unwrap();
    let mut config = ClientConfig::new(binary);
    config
        .env
        .push(("PI_CODING_AGENT_DIR".into(), agent_dir.into_os_string()));
    config.args = vec![
        "--no-extensions".into(),
        "--no-skills".into(),
        "--no-prompt-templates".into(),
        "--no-context-files".into(),
        "--offline".into(),
        "--session-dir".into(),
        session_dir.into_os_string(),
    ];
    config.restart_delay = Duration::from_millis(100);
    Client::spawn(config).unwrap()
}

#[test]
#[ignore = "requires PI_RPC_TEST_BINARY=official pi 0.84.2"]
fn zero_token_command_matrix() {
    let client = client();
    let state: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    assert!(!state.session_id.is_empty());

    for command in [
        Command::GetCommands,
        Command::GetMessages,
        Command::GetEntries { since: None },
        Command::GetTree,
        Command::GetSessionStats,
        Command::GetLastAssistantText,
        Command::GetAvailableModels,
        Command::GetAvailableThinkingLevels,
        Command::SetThinkingLevel {
            level: ThinkingLevel::Off,
        },
        Command::SetSteeringMode {
            mode: QueueMode::All,
        },
        Command::SetSteeringMode {
            mode: QueueMode::OneAtATime,
        },
        Command::SetFollowUpMode {
            mode: QueueMode::All,
        },
        Command::SetFollowUpMode {
            mode: QueueMode::OneAtATime,
        },
        Command::SetAutoCompaction { enabled: false },
        Command::SetAutoCompaction { enabled: true },
        Command::SetAutoRetry { enabled: false },
        Command::SetAutoRetry { enabled: true },
        Command::Abort,
        Command::AbortRetry,
        Command::AbortBash,
        Command::CycleModel,
        Command::CycleThinkingLevel,
    ] {
        let response = client.request(command, TIMEOUT).unwrap();
        assert!(
            response.success,
            "{}: {:?}",
            response.command, response.error
        );
    }

    let invalid = client
        .request(
            Command::SetModel {
                provider: "missing-provider".into(),
                model_id: "missing-model".into(),
            },
            TIMEOUT,
        )
        .unwrap();
    assert!(!invalid.success);

    let invalid_cursor = client
        .request(
            Command::GetEntries {
                since: Some("missing-entry".into()),
            },
            TIMEOUT,
        )
        .unwrap();
    assert!(!invalid_cursor.success);

    let bash_events = client.subscribe();
    let bash_response = client
        .request(
            Command::Bash {
                command: "printf pi-rpc-ok".into(),
                exclude_from_context: Some(true),
            },
            TIMEOUT,
        )
        .unwrap();
    let bash_id = bash_response.id.clone().unwrap();
    let bash: BashResult = bash_response.decode_data().unwrap();
    assert!(bash.output.contains("pi-rpc-ok"));
    assert_eq!(bash.exit_code, Some(0));
    let event_deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_correlated_update = false;
    while Instant::now() < event_deadline {
        if let Ok(ClientEvent::Rpc(event)) = bash_events.recv_timeout(Duration::from_millis(100))
            && let pi_rpc::RpcEvent::BashExecutionUpdate { id, delta } = *event
            && id.as_deref() == Some(&bash_id)
            && delta.contains("pi-rpc-ok")
        {
            saw_correlated_update = true;
            break;
        }
    }
    assert!(saw_correlated_update);

    let before = state.session_id;
    let new_session: Value = client
        .request_data(
            Command::NewSession {
                parent_session: None,
            },
            TIMEOUT,
        )
        .unwrap();
    assert_eq!(new_session["cancelled"], false);
    let after: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    assert_ne!(before, after.session_id);
    client.shutdown().unwrap();
}

#[test]
#[ignore = "requires PI_RPC_TEST_BINARY=official pi 0.84.2"]
fn kill_restart_and_resume_fixture() {
    let client = client();
    let events = client.subscribe();
    let fixture_dir = env::current_dir()
        .unwrap()
        .join("target")
        .join("pi-rpc-tests")
        .join("real-fixtures");
    fs::create_dir_all(&fixture_dir).unwrap();
    let fixture_path = fixture_dir.join(format!("resume-{}.jsonl", std::process::id()));
    let cwd = env::current_dir().unwrap();
    let fixture = format!(
        "{}\n{}\n",
        serde_json::json!({
            "type": "session",
            "version": 3,
            "id": "pi-rpc-resume-fixture",
            "timestamp": "2026-01-01T00:00:00.000Z",
            "cwd": cwd
        }),
        serde_json::json!({
            "type": "session_info",
            "id": "fixture-entry",
            "parentId": null,
            "timestamp": "2026-01-01T00:00:00.001Z",
            "name": "pi-rpc-resume-fixture"
        })
    );
    fs::write(&fixture_path, fixture).unwrap();
    let switched: Value = client
        .request_data(
            Command::SwitchSession {
                session_path: fixture_path.to_string_lossy().into_owned(),
            },
            TIMEOUT,
        )
        .unwrap();
    assert_eq!(switched["cancelled"], false);
    client.set_resume_session(Some(fixture_path.clone()));
    let before: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    let session_file = before
        .session_file
        .clone()
        .expect("persistent session file");
    assert_eq!(Path::new(&session_file), fixture_path);
    client.kill_process_tree().unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut saw_exit = false;
    let mut saw_restart = false;
    while Instant::now() < deadline && !saw_restart {
        match events.recv_timeout(Duration::from_millis(200)) {
            Ok(ClientEvent::Lifecycle(LifecycleEvent::Exited { .. })) => saw_exit = true,
            Ok(ClientEvent::Lifecycle(LifecycleEvent::Restarted {
                session_file: resumed,
                ..
            })) => {
                saw_restart = resumed.as_deref() == Some(Path::new(&session_file));
            }
            _ => {}
        }
    }
    assert!(saw_exit && saw_restart);
    let after: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    assert_eq!(after.session_id, before.session_id);
    assert_eq!(after.session_name, before.session_name);
    assert_eq!(after.message_count, before.message_count);
    client.shutdown().unwrap();
}
