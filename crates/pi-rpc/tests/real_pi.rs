use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::{Duration, Instant},
};

use tempfile::TempDir;

use pi_rpc::{
    BashResult, Client, ClientConfig, ClientEvent, Command, CommandsData, LifecycleEvent,
    MessagesData, PINNED_PI_VERSION, QueueMode, RpcSessionState, StreamingBehavior, ThinkingLevel,
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

struct TestClient {
    client: Client,
    _temp: TempDir,
}

fn client_config(temp: &TempDir) -> ClientConfig {
    let binary = configured_binary();
    assert_version(&binary);
    let session_dir = temp.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap();
    let agent_dir = temp.path().join("agent");
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
    config
}

fn client() -> TestClient {
    let temp = tempfile::tempdir().unwrap();
    let client = Client::spawn(client_config(&temp)).unwrap();
    TestClient {
        client,
        _temp: temp,
    }
}

#[test]
#[ignore = "requires PI_RPC_TEST_BINARY=official pi 0.84.2"]
fn zero_token_command_matrix() {
    let test = client();
    let client = &test.client;
    let state: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    assert!(!state.session_id.is_empty());

    let commands: CommandsData = client.request_data(Command::GetCommands, TIMEOUT).unwrap();
    assert!(commands.commands.iter().all(|command| {
        !command.name.is_empty()
            && !command.source_info.path.is_empty()
            && !command.source_info.source.is_empty()
    }));

    for command in [
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
fn first_spawn_resumes_fixture_directly() {
    let temp = tempfile::tempdir().unwrap();
    let fixture_path = temp.path().join("initial-resume.jsonl");
    let cwd = env::current_dir().unwrap();
    fs::write(
        &fixture_path,
        format!(
            "{}\n",
            serde_json::json!({
                "type": "session",
                "version": 3,
                "id": "pi-rpc-initial-resume-fixture",
                "timestamp": "2026-01-01T00:00:00.000Z",
                "cwd": cwd
            })
        ),
    )
    .unwrap();
    let mut config = client_config(&temp);
    config.initial_session = Some(fixture_path.clone());
    let client = Client::spawn(config).unwrap();
    let state: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    assert_eq!(state.session_id, "pi-rpc-initial-resume-fixture");
    assert_eq!(
        state.session_file.as_deref().map(Path::new),
        Some(fixture_path.as_path())
    );
    client.shutdown().unwrap();
}

#[test]
#[ignore = "requires PI_RPC_TEST_BINARY=official pi 0.84.2"]
fn kill_restart_and_resume_fixture() {
    let test = client();
    let client = &test.client;
    let events = client.subscribe();
    let fixture_path = test._temp.path().join("resume.jsonl");
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

#[test]
#[ignore = "spends real model tokens; requires PI_RPC_R7_LIVE=1 and existing pi credentials"]
fn r7_live_stream_requires_explicit_opt_in() {
    if env::var("PI_RPC_R7_LIVE").as_deref() != Ok("1") {
        eprintln!("skip: set PI_RPC_R7_LIVE=1 explicitly before spending tokens");
        return;
    }
    let test = client();
    let client = &test.client;
    let events = client.subscribe();
    let prompt = env::var("PI_RPC_R7_PROMPT")
        .unwrap_or_else(|_| "Reply with exactly: r7-live-ok".to_owned());
    let response = client
        .request(
            Command::Prompt {
                message: prompt,
                images: None,
                streaming_behavior: None,
            },
            Duration::from_secs(120),
        )
        .unwrap();
    assert!(response.success, "{:?}", response.error);

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut deltas = 0;
    let mut message_end = None;
    let mut settled = false;
    while Instant::now() < deadline && !settled {
        if let Ok(ClientEvent::Rpc(event)) = events.recv_timeout(Duration::from_millis(250)) {
            match *event {
                pi_rpc::RpcEvent::MessageUpdate {
                    assistant_message_event: pi_rpc::AssistantMessageEvent::TextDelta { .. },
                    ..
                } => deltas += 1,
                pi_rpc::RpcEvent::MessageEnd { message } => message_end = Some(message),
                pi_rpc::RpcEvent::AgentSettled => settled = true,
                _ => {}
            }
        }
    }
    assert!(deltas > 0, "no text deltas observed");
    let authoritative = message_end.expect("no authoritative message_end observed");
    assert!(settled, "no agent_settled observed");

    let messages: MessagesData = client.request_data(Command::GetMessages, TIMEOUT).unwrap();
    assert!(
        messages
            .messages
            .iter()
            .any(|message| message == &authoritative),
        "get_messages did not contain authoritative assistant"
    );
    let state: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
    let session_file = state.session_file.expect("persistent live session");
    let session_text = fs::read_to_string(session_file).unwrap();
    let assistant_text = authoritative
        .0
        .get("content")
        .and_then(Value::as_array)
        .and_then(|blocks| {
            blocks
                .iter()
                .find_map(|block| block.get("text").and_then(Value::as_str))
        })
        .expect("authoritative assistant text");
    assert!(session_text.contains(assistant_text));
    client.shutdown().unwrap();
}

#[test]
#[ignore = "spends real model tokens; requires PI_RPC_R7_QUEUE=1 and existing pi credentials"]
fn r7_live_queue_contract_requires_explicit_opt_in() {
    if env::var("PI_RPC_R7_QUEUE").as_deref() != Ok("1") {
        eprintln!("skip: set PI_RPC_R7_QUEUE=1 explicitly before spending tokens");
        return;
    }
    let test = client();
    let client = &test.client;
    let events = client.subscribe();
    let prompt = env::var("PI_RPC_R7_LONG_PROMPT").unwrap_or_else(|_| {
        "Write a numbered list from 1 to 80, with one short sentence per item.".to_owned()
    });
    assert!(
        client
            .request(
                Command::Prompt {
                    message: prompt,
                    images: None,
                    streaming_behavior: None
                },
                Duration::from_secs(120),
            )
            .unwrap()
            .success
    );
    assert!(
        client
            .request(
                Command::Prompt {
                    message: "Steer: keep every item under eight words.".into(),
                    images: None,
                    streaming_behavior: Some(StreamingBehavior::Steer),
                },
                TIMEOUT,
            )
            .unwrap()
            .success
    );
    assert!(
        client
            .request(
                Command::Prompt {
                    message: "Afterward reply with queue-contract-done.".into(),
                    images: None,
                    streaming_behavior: Some(StreamingBehavior::FollowUp),
                },
                TIMEOUT,
            )
            .unwrap()
            .success
    );
    assert!(client.request(Command::Abort, TIMEOUT).unwrap().success);

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut queue_updates = 0;
    let mut settled = false;
    while Instant::now() < deadline && !settled {
        if let Ok(ClientEvent::Rpc(event)) = events.recv_timeout(Duration::from_millis(250)) {
            match *event {
                pi_rpc::RpcEvent::QueueUpdate { .. } => queue_updates += 1,
                pi_rpc::RpcEvent::AgentSettled => settled = true,
                _ => {}
            }
        }
    }
    assert!(queue_updates > 0, "no queue_update observed");
    assert!(settled, "no agent_settled observed after abort");
    client.shutdown().unwrap();
}
