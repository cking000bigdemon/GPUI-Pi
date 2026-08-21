use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::{Duration, Instant},
};

use tempfile::TempDir;

use pi_rpc::{
    AvailableModelsData, BashResult, Client, ClientConfig, ClientEvent, Command, CommandsData,
    ExtensionUiRequest, ExtensionUiResponse, LifecycleEvent, MessagesData, PINNED_PI_VERSION,
    QueueMode, RpcEvent, RpcSessionState, StreamingBehavior, ThinkingLevel, ThinkingLevelsData,
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
    let models: AvailableModelsData = client
        .request_data(Command::GetAvailableModels, TIMEOUT)
        .unwrap();
    assert!(models.models.iter().all(|model| {
        !model.id.is_empty() && !model.provider.is_empty() && !model.name.is_empty()
    }));
    let levels: ThinkingLevelsData = client
        .request_data(Command::GetAvailableThinkingLevels, TIMEOUT)
        .unwrap();
    assert!(!levels.levels.is_empty());
    assert!(levels.levels.contains(&state.thinking_level));

    let commands: CommandsData = client.request_data(Command::GetCommands, TIMEOUT).unwrap();
    assert!(commands.commands.iter().all(|command| {
        !command.name.is_empty()
            && !command.source_info.path.is_empty()
            && !command.source_info.source.is_empty()
    }));

    let can_cycle_model = models.models.len() > 1;
    let can_cycle_thinking = levels.levels.len() > 1;

    for command in [
        Command::GetMessages,
        Command::GetEntries { since: None },
        Command::GetTree,
        Command::GetSessionStats,
        Command::GetLastAssistantText,
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
    ] {
        let response = client.request(command, TIMEOUT).unwrap();
        assert!(
            response.success,
            "{}: {:?}",
            response.command, response.error
        );
    }

    if can_cycle_model {
        let response = client.request(Command::CycleModel, TIMEOUT).unwrap();
        assert!(response.success, "cycle_model: {:?}", response.error);
        let state: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
        let selected = state.model.expect("cycled model must be selected");
        assert!(
            models
                .models
                .iter()
                .any(|model| { model.provider == selected.provider && model.id == selected.id })
        );
    }
    if can_cycle_thinking {
        let response = client
            .request(Command::CycleThinkingLevel, TIMEOUT)
            .unwrap();
        assert!(
            response.success,
            "cycle_thinking_level: {:?}",
            response.error
        );
        let state: RpcSessionState = client.request_data(Command::GetState, TIMEOUT).unwrap();
        assert!(levels.levels.contains(&state.thinking_level));
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
fn extension_ui_zero_token_fixture_reaches_nine_methods_and_custom_is_unreachable() {
    let temp = tempfile::tempdir().unwrap();
    let extension_path = temp.path().join("extension-ui-fixture.ts");
    fs::write(
        &extension_path,
        r#"import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
export default function (pi: ExtensionAPI) {
  pi.registerCommand("r14-ui", {
    description: "R14 zero-token extension UI fixture",
    handler: async (_args, ctx) => {
      ctx.ui.notify("notify", "warning");
      ctx.ui.setStatus("r14", "status");
      ctx.ui.setWidget("above", ["above"], { placement: "aboveEditor" });
      ctx.ui.setWidget("below", ["below"], { placement: "belowEditor" });
      ctx.ui.setTitle("R14 Fixture");
      ctx.ui.setEditorText("fixture editor text");
      await ctx.ui.select("Select", ["A", "B"]);
      await ctx.ui.confirm("Confirm", "Continue?");
      await ctx.ui.input("Input", "value");
      await ctx.ui.editor("Editor", "prefill");
      const custom = await ctx.ui.custom(() => { throw new Error("must not run"); });
      if (custom !== undefined) throw new Error("custom unexpectedly returned a value");
      ctx.ui.notify("custom:UNSUPPORTED_BY_PINNED_RPC", "info");
    },
  });
}
"#,
    )
    .unwrap();
    let mut config = client_config(&temp);
    config.args.retain(|arg| arg != "--no-extensions");
    config.args.push("--extension".into());
    config.args.push(extension_path.into_os_string());
    let client = Client::spawn(config).unwrap();
    let events = client.subscribe();
    let prompt_client = client.clone();
    let prompt = std::thread::spawn(move || {
        prompt_client.request(
            Command::Prompt {
                message: "/r14-ui".into(),
                images: None,
                streaming_behavior: None,
            },
            TIMEOUT,
        )
    });

    let deadline = Instant::now() + TIMEOUT;
    let mut methods = Vec::new();
    let mut custom_marker = false;
    let mut unknown_events = Vec::new();
    let mut custom_wire_requests = Vec::new();
    while Instant::now() < deadline && (!custom_marker || methods.len() < 10) {
        match events.recv_timeout(Duration::from_millis(100)) {
            Ok(ClientEvent::Rpc(event)) => {
                if let RpcEvent::ExtensionUiRequest { id, request } = *event {
                    let serialized = serde_json::to_value(&request).unwrap();
                    if serialized.get("method").and_then(Value::as_str) == Some("custom") {
                        custom_wire_requests.push(serialized);
                    }
                    match &request {
                        ExtensionUiRequest::Select { .. }
                        | ExtensionUiRequest::Input { .. }
                        | ExtensionUiRequest::Editor { .. } => client
                            .send_extension_ui_response(&ExtensionUiResponse::value(&id, "fixture"))
                            .unwrap(),
                        ExtensionUiRequest::Confirm { .. } => client
                            .send_extension_ui_response(&ExtensionUiResponse::confirmed(&id, true))
                            .unwrap(),
                        ExtensionUiRequest::Notify { message, .. } => {
                            custom_marker |= message.contains("UNSUPPORTED_BY_PINNED_RPC");
                        }
                        _ => {}
                    }
                    methods.push(request);
                }
            }
            Ok(ClientEvent::Unknown(value)) => unknown_events.push(value),
            Ok(ClientEvent::Lifecycle(_)) | Err(_) => {}
        }
    }
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::Select { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::Confirm { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::Input { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::Editor { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::Notify { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::SetStatus { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::SetWidget { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::SetTitle { .. }))
    );
    assert!(
        methods
            .iter()
            .any(|request| matches!(request, ExtensionUiRequest::SetEditorText { .. }))
    );
    assert!(
        custom_marker,
        "custom() did not prove UNSUPPORTED_BY_PINNED_RPC"
    );
    assert!(
        unknown_events.is_empty(),
        "fixture emitted unknown wire events: {unknown_events:?}"
    );
    assert!(
        custom_wire_requests.is_empty(),
        "custom() unexpectedly emitted extension_ui_request: {custom_wire_requests:?}"
    );
    let response = prompt.join().unwrap().unwrap();
    assert!(response.success, "{:?}", response.error);
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
