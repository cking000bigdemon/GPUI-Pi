use std::{
    path::PathBuf,
    process::Command as ProcessCommand,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};

use pi_rpc::{
    Client, ClientConfig, ClientError, ClientEvent, Command, CommandsData, ImageContent, ImageKind,
    LifecycleEvent, RpcSessionState, SlashCommandSource,
};
use serde_json::Value;

fn fake_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_child"))
}

fn config() -> ClientConfig {
    let mut config = ClientConfig::new(fake_binary());
    config.restart_delay = Duration::from_millis(20);
    config
}

#[test]
fn correlates_concurrent_requests_and_drains_stderr() {
    let client = Client::spawn(config()).unwrap();
    let events = client.subscribe();
    let client = Arc::new(client);
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let client = Arc::clone(&client);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            client
                .request(Command::GetMessages, Duration::from_secs(2))
                .unwrap()
        }));
    }
    barrier.wait();
    let first = handles.remove(0).join().unwrap();
    let second = handles.remove(0).join().unwrap();
    assert_ne!(first.id, second.id);

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_stderr = false;
    while Instant::now() < deadline {
        if let Ok(ClientEvent::Lifecycle(LifecycleEvent::Stderr { line })) =
            events.recv_timeout(Duration::from_millis(50))
        {
            saw_stderr = line.contains("fake child ready");
            if saw_stderr {
                break;
            }
        }
    }
    assert!(saw_stderr);
    client.shutdown().unwrap();
}

#[test]
fn initial_session_is_used_by_the_first_spawn() {
    let initial = std::env::temp_dir().join(format!(
        "pi-rpc-initial-session-{}.jsonl",
        std::process::id()
    ));
    let mut child_config = config();
    child_config.initial_session = Some(initial.clone());
    let client = Client::spawn(child_config).unwrap();
    let state: RpcSessionState = client
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        state.session_file.as_deref().map(PathBuf::from),
        Some(initial.clone())
    );
    assert_eq!(client.resume_session(), Some(initial));
    client.shutdown().unwrap();
}

#[test]
fn new_client_can_resume_the_same_session_with_a_new_tool_allowlist() {
    let session = std::env::temp_dir().join(format!(
        "pi-rpc-tool-restart-session-{}.jsonl",
        std::process::id()
    ));
    let mut initial = config();
    initial.initial_session = Some(session.clone());
    initial
        .args
        .extend(["--tools".into(), "read,bash,edit,write".into()]);
    let first = Client::spawn(initial).unwrap();
    let first_state: Value = first
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        first_state["sessionFile"],
        session.to_string_lossy().as_ref()
    );
    assert_eq!(first_state["toolAllowlist"], "read,bash,edit,write");
    let first_pid = first.pid().unwrap();
    first.shutdown().unwrap();

    let mut restarted = config();
    restarted.initial_session = Some(session.clone());
    restarted
        .args
        .extend(["--tools".into(), "read,grep,find,ls".into()]);
    let second = Client::spawn(restarted).unwrap();
    let second_state: Value = second
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        second_state["sessionFile"],
        session.to_string_lossy().as_ref()
    );
    // fake child 的 sessionId 由同一个 --session 路径派生；这里只验证重启参数透传。
    assert_eq!(second_state["sessionId"], first_state["sessionId"]);
    assert_eq!(second_state["toolAllowlist"], "read,grep,find,ls");
    assert_ne!(second.pid().unwrap(), first_pid);
    second.shutdown().unwrap();
}

#[test]
fn empty_tool_allowlist_is_preserved_as_an_explicit_argument() {
    let mut child_config = config();
    child_config.args.extend(["--tools".into(), "".into()]);
    let client = Client::spawn(child_config).unwrap();
    let state: Value = client
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    assert_eq!(state["toolAllowlist"], "");
    client.shutdown().unwrap();
}

#[test]
fn crash_fails_old_pending_then_restarts_with_session() {
    let client = Client::spawn(config()).unwrap();
    let events = client.subscribe();
    let state: RpcSessionState = client
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    let session_file = state.session_file.map(PathBuf::from).unwrap();

    let pending_client = client.clone();
    let pending = thread::spawn(move || {
        pending_client.request(
            Command::Prompt {
                message: "ignored".into(),
                images: None,
                streaming_behavior: None,
            },
            Duration::from_secs(5),
        )
    });
    thread::sleep(Duration::from_millis(50));
    client.kill_process_tree().unwrap();
    assert!(matches!(
        pending.join().unwrap(),
        Err(ClientError::ProcessExited { .. })
    ));

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut resumed = false;
    while Instant::now() < deadline {
        if let Ok(ClientEvent::Lifecycle(LifecycleEvent::Restarted {
            session_file: actual,
            ..
        })) = events.recv_timeout(Duration::from_millis(100))
        {
            resumed = actual.as_deref() == Some(session_file.as_path());
            if resumed {
                break;
            }
        }
    }
    assert!(resumed);
    let restored: RpcSessionState = client
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    assert_eq!(restored.session_id, state.session_id);
    client.shutdown().unwrap();
}

#[test]
fn ephemeral_state_clears_a_previous_resume_target() {
    let mut child_config = config();
    child_config.args.push("--no-session".into());
    let client = Client::spawn(child_config).unwrap();
    client.set_resume_session(Some(PathBuf::from("stale-session.jsonl")));
    let state: RpcSessionState = client
        .request_data(Command::GetState, Duration::from_secs(2))
        .unwrap();
    assert!(state.session_file.is_none());
    assert!(client.resume_session().is_none());
    client.shutdown().unwrap();
}

#[test]
fn active_shutdown_does_not_restart() {
    let client = Client::spawn(config()).unwrap();
    let events = client.subscribe();
    client.shutdown().unwrap();
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        if let Ok(ClientEvent::Lifecycle(LifecycleEvent::Restarting { .. })) =
            events.recv_timeout(Duration::from_millis(20))
        {
            panic!("active shutdown restarted the process");
        }
    }
}

#[test]
fn external_tree_kill_works_for_fake_child() {
    let client = Client::spawn(config()).unwrap();
    let pid = client.pid().unwrap();
    #[cfg(windows)]
    let status = ProcessCommand::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()
        .unwrap();
    #[cfg(unix)]
    let status = ProcessCommand::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .unwrap();
    assert!(status.status.success());
    client.shutdown().unwrap();
}

#[test]
fn shutdown_kills_a_child_that_does_not_handle_stdin_eof() {
    let mut child_config = config();
    child_config.shutdown_grace_period = Duration::from_millis(50);
    let client = Client::spawn(child_config).unwrap();
    let pending_client = client.clone();
    let pending = thread::spawn(move || {
        pending_client.request(
            Command::Prompt {
                message: "ignored".into(),
                images: None,
                streaming_behavior: None,
            },
            Duration::from_secs(5),
        )
    });
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    client.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        pending.join().unwrap(),
        Err(ClientError::ProcessExited { .. })
    ));
}

#[test]
fn burst_subscription_keeps_authoritative_tail_events() {
    let client = Client::spawn(config()).unwrap();
    let events = client.subscribe();
    let response = client
        .request(
            Command::Prompt {
                message: "stream".into(),
                images: None,
                streaming_behavior: None,
            },
            Duration::from_secs(5),
        )
        .unwrap();
    assert!(response.success);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut updates = 0;
    let mut saw_message_end = false;
    let mut saw_agent_end = false;
    let mut saw_settled = false;
    while Instant::now() < deadline && !saw_settled {
        if let Ok(ClientEvent::Rpc(event)) = events.recv_timeout(Duration::from_millis(50)) {
            match *event {
                pi_rpc::RpcEvent::MessageUpdate { .. } => updates += 1,
                pi_rpc::RpcEvent::MessageEnd { .. } => saw_message_end = true,
                pi_rpc::RpcEvent::AgentEnd { .. } => saw_agent_end = true,
                pi_rpc::RpcEvent::AgentSettled => saw_settled = true,
                _ => {}
            }
        }
    }
    assert_eq!(updates, 1500);
    assert!(saw_message_end && saw_agent_end && saw_settled);
    client.shutdown().unwrap();
}

#[test]
fn get_commands_decodes_typed_sources_and_image_prompt_preserves_wire() {
    let client = Client::spawn(config()).unwrap();
    let commands: CommandsData = client
        .request_data(Command::GetCommands, Duration::from_secs(2))
        .unwrap();
    assert_eq!(commands.commands.len(), 3);
    assert_eq!(commands.commands[0].source, SlashCommandSource::Extension);
    assert_eq!(commands.commands[1].source, SlashCommandSource::Prompt);
    assert_eq!(commands.commands[2].source, SlashCommandSource::Skill);

    let response = client
        .request(
            Command::Prompt {
                message: "wire-image".into(),
                images: Some(vec![ImageContent {
                    kind: ImageKind::Image,
                    data: "iVBORw0KGgo=".into(),
                    mime_type: "image/png".into(),
                }]),
                streaming_behavior: Some(pi_rpc::StreamingBehavior::Steer),
            },
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(response.success);
    client.shutdown().unwrap();
}

#[test]
fn fake_queue_and_abort_emit_settled_tails() {
    let client = Client::spawn(config()).unwrap();
    let events = client.subscribe();
    client
        .request(
            Command::Prompt {
                message: "queue".into(),
                images: None,
                streaming_behavior: Some(pi_rpc::StreamingBehavior::FollowUp),
            },
            Duration::from_secs(2),
        )
        .unwrap();
    let mut queue_snapshots = Vec::new();
    let mut settled = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && settled == 0 {
        if let Ok(ClientEvent::Rpc(event)) = events.recv_timeout(Duration::from_millis(50)) {
            match *event {
                pi_rpc::RpcEvent::QueueUpdate {
                    steering,
                    follow_up,
                } => {
                    queue_snapshots.push((steering, follow_up));
                }
                pi_rpc::RpcEvent::AgentSettled => settled += 1,
                _ => {}
            }
        }
    }
    assert_eq!(queue_snapshots.len(), 2);
    assert_eq!(queue_snapshots[1].0, ["replacement"]);
    assert!(queue_snapshots[1].1.is_empty());

    client
        .request(Command::Abort, Duration::from_secs(2))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut abort_message_end = false;
    while Instant::now() < deadline {
        if let Ok(ClientEvent::Rpc(event)) = events.recv_timeout(Duration::from_millis(50)) {
            match *event {
                pi_rpc::RpcEvent::MessageEnd { .. } => abort_message_end = true,
                pi_rpc::RpcEvent::AgentSettled => break,
                _ => {}
            }
        }
    }
    assert!(abort_message_end);
    client.shutdown().unwrap();
}
