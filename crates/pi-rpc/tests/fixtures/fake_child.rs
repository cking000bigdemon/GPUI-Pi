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
    let session_id = session_file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("fake-session")
        .to_owned();
    let available_models = json!([
        {
            "id":"model-one",
            "name":"Model One",
            "api":"fixture",
            "provider":"provider-one",
            "baseUrl":"https://fixture.invalid",
            "reasoning":true,
            "input":["text"],
            "cost":{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0},
            "contextWindow":100000,
            "maxTokens":4096
        },
        {
            "id":"model-two",
            "name":"Model Two",
            "api":"fixture",
            "provider":"provider-two",
            "baseUrl":"https://fixture.invalid",
            "reasoning":true,
            "input":["text","image"],
            "cost":{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0},
            "contextWindow":200000,
            "maxTokens":8192
        }
    ]);
    let mut current_model = available_models[0].clone();
    let mut thinking_level = "off".to_owned();
    let tool_allowlist = env::args_os()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|args| args[0] == "--tools")
        .map(|args| args[1].to_string_lossy().into_owned());

    eprintln!(
        "fake child ready tools={}",
        tool_allowlist.as_deref().unwrap_or("<unset>")
    );
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
                    "model": current_model,
                    "thinkingLevel": thinking_level,
                    "toolAllowlist": tool_allowlist,
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
            "get_available_models" => json!({
                "id":id,
                "type":"response",
                "command":"get_available_models",
                "success":true,
                "data":{"models":available_models}
            }),
            "get_available_thinking_levels" => json!({
                "id":id,
                "type":"response",
                "command":"get_available_thinking_levels",
                "success":true,
                "data":{"levels":["off","low","high"]}
            }),
            "set_model" => {
                let target = available_models.as_array().unwrap().iter().find(|model| {
                    model["provider"] == value["provider"] && model["id"] == value["modelId"]
                });
                if let Some(target) = target {
                    current_model = target.clone();
                    json!({"id":id,"type":"response","command":"set_model","success":true,"data":current_model})
                } else {
                    json!({"id":id,"type":"response","command":"set_model","success":false,"error":"model not found"})
                }
            }
            "cycle_model" => {
                current_model = if current_model["id"] == "model-one" {
                    available_models[1].clone()
                } else {
                    available_models[0].clone()
                };
                json!({"id":id,"type":"response","command":"cycle_model","success":true,"data":{"model":current_model,"thinkingLevel":thinking_level,"isScoped":false}})
            }
            "set_thinking_level" => {
                thinking_level = value["level"].as_str().unwrap_or("off").to_owned();
                json!({"id":id,"type":"response","command":"set_thinking_level","success":true})
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
