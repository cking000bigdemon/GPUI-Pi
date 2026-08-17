use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

use serde_json::{Value, json};

fn main() {
    let ephemeral = env::args_os().any(|arg| arg == "--no-session");
    let mut session_file = env::args_os()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|args| args[0] == "--session")
        .map(|args| PathBuf::from(&args[1]));
    if session_file.is_none() {
        session_file = env::var_os("PI_RPC_FAKE_SESSION").map(PathBuf::from);
    }
    let session_file =
        session_file.unwrap_or_else(|| env::temp_dir().join("pi-rpc-fake-session.jsonl"));
    let session_id = "fake-session";

    eprintln!("fake child ready");
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in BufReader::new(stdin.lock()).lines() {
        let line = line.unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let command = value["type"].as_str().unwrap();
        if command == "prompt" {
            let message = value["message"].as_str().unwrap_or_default();
            if message == "stream" {
                writeln!(stdout, "{}", json!({"type":"agent_start"})).unwrap();
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"message_start","message":{"role":"assistant","content":[]}})
                )
                .unwrap();
                for index in 0..1500 {
                    writeln!(stdout, "{}", json!({
                        "type":"message_update",
                        "usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,"cost":{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0,"total":0.0}},
                        "assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":if index == 1499 {"done"} else {"x"}}
                    })).unwrap();
                }
                writeln!(stdout, "{}", json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"authoritative"}]}})).unwrap();
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"agent_end","messages":[],"willRetry":false})
                )
                .unwrap();
                writeln!(stdout, "{}", json!({"type":"agent_settled"})).unwrap();
                let response = json!({"id":id,"type":"response","command":command,"success":true});
                writeln!(stdout, "{response}").unwrap();
                stdout.flush().unwrap();
                continue;
            }
            if message == "wire-image" {
                let valid = value["streamingBehavior"] == "steer"
                    && value["images"][0]["type"] == "image"
                    && value["images"][0]["data"] == "iVBORw0KGgo="
                    && value["images"][0]["mimeType"] == "image/png";
                let response = if valid {
                    json!({"id":id,"type":"response","command":command,"success":true})
                } else {
                    json!({"id":id,"type":"response","command":command,"success":false,"error":"image wire mismatch"})
                };
                writeln!(stdout, "{response}").unwrap();
                stdout.flush().unwrap();
                continue;
            }
            if message == "queue" {
                writeln!(stdout, "{}", json!({"type":"agent_start"})).unwrap();
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"queue_update","steering":["first"],"followUp":["later"]})
                )
                .unwrap();
                writeln!(
                    stdout,
                    "{}",
                    json!({"type":"queue_update","steering":["replacement"],"followUp":[]})
                )
                .unwrap();
                writeln!(stdout, "{}", json!({"type":"agent_settled"})).unwrap();
                let response = json!({"id":id,"type":"response","command":command,"success":true});
                writeln!(stdout, "{response}").unwrap();
                stdout.flush().unwrap();
                continue;
            }
            thread::sleep(Duration::from_secs(60));
            continue;
        }
        if command == "abort" {
            writeln!(stdout, "{}", json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"aborted tail"}],"stopReason":"aborted"}})).unwrap();
            writeln!(
                stdout,
                "{}",
                json!({"type":"agent_end","messages":[],"willRetry":false})
            )
            .unwrap();
            writeln!(stdout, "{}", json!({"type":"agent_settled"})).unwrap();
            let response = json!({"id":id,"type":"response","command":command,"success":true});
            writeln!(stdout, "{response}").unwrap();
            stdout.flush().unwrap();
            continue;
        }
        if command == "set_session_name" && value["name"] == "emit_many" {
            for _ in 0..2000 {
                writeln!(stdout, "{}", json!({"type":"agent_start"})).unwrap();
            }
            let response = json!({"id":id,"type":"response","command":command,"success":true});
            writeln!(stdout, "{response}").unwrap();
            stdout.flush().unwrap();
            continue;
        }
        if command == "emit" {
            writeln!(stdout, "{}", json!({"type":"agent_start"})).unwrap();
        }
        let response = match command {
            "get_state" => json!({
                "id": id,
                "type": "response",
                "command": "get_state",
                "success": true,
                "data": {
                    "thinkingLevel": "off",
                    "isStreaming": false,
                    "isCompacting": false,
                    "steeringMode": "all",
                    "followUpMode": "all",
                    "sessionFile": (!ephemeral).then_some(&session_file),
                    "sessionId": session_id,
                    "autoCompactionEnabled": true,
                    "messageCount": 0,
                    "pendingMessageCount": 0
                }
            }),
            "get_messages" => {
                json!({"id":id,"type":"response","command":"get_messages","success":true,"data":{"messages":[]}})
            }
            "get_commands" => json!({
                "id":id,
                "type":"response",
                "command":"get_commands",
                "success":true,
                "data":{"commands":[
                    {"name":"fixture-extension","description":"Extension fixture","source":"extension","sourceInfo":{"path":"/fixture/ext.ts","source":"fixture","scope":"user","origin":"top-level"}},
                    {"name":"fixture-prompt","description":"Prompt fixture","source":"prompt","sourceInfo":{"path":"/fixture/prompt.md","source":"fixture","scope":"project","origin":"top-level"}},
                    {"name":"skill:fixture","description":"Skill fixture","source":"skill","sourceInfo":{"path":"/fixture/SKILL.md","source":"fixture","scope":"user","origin":"top-level"}}
                ]}
            }),
            _ => json!({"id":id,"type":"response","command":command,"success":true}),
        };
        writeln!(stdout, "{response}").unwrap();
        stdout.flush().unwrap();
    }
}
