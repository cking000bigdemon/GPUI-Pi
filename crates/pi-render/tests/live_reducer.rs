use pi_render::{
    Block, ConversationDocument, ConversationItem, LiveAssistantUpdate, LiveBlockKind, LiveEvent,
    LivePhase, LiveSessionReducer, MarkdownBlock, Message, MessageRole, MinimapNode, ModelRef,
    ToolOutput, ToolStatus,
};
use serde_json::json;
use std::sync::Arc;

#[test]
fn assembles_multiple_blocks_and_message_end_is_authoritative() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply_batch([
        LiveEvent::AgentStart,
        LiveEvent::MessageStart {
            message: json!({"role":"assistant","content":[]}),
        },
        LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockStart {
            index: 1,
            kind: LiveBlockKind::Text,
        }),
        LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockDelta {
            index: 1,
            kind: LiveBlockKind::Text,
            delta: "draft".into(),
        }),
        LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockStart {
            index: 0,
            kind: LiveBlockKind::Thinking,
        }),
        LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockEnd {
            index: 0,
            kind: LiveBlockKind::Thinking,
            content: json!("final thought"),
        }),
    ]);
    let draft = reducer.document();
    assert!(
        matches!(&draft.messages[0].blocks[0], Block::Thinking(text) if text == "final thought")
    );
    assert!(
        matches!(&draft.messages[0].blocks[1], Block::Markdown(text) if text.source == "draft")
    );

    reducer.apply(LiveEvent::MessageEnd {
        message: json!({
            "id":"authoritative",
            "role":"assistant",
            "content":[{"type":"text","text":"final snapshot"}]
        }),
    });
    let final_document = reducer.document();
    assert_eq!(final_document.messages[0].id, "authoritative");
    assert!(
        matches!(&final_document.messages[0].blocks[0], Block::Markdown(text) if text.source == "final snapshot")
    );
}

#[test]
fn live_assistant_preserves_model_metadata_from_start_and_end() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageStart {
        message: json!({
            "role":"assistant",
            "provider":"provider-one",
            "model":"model-one",
            "content":[]
        }),
    });
    reducer.apply(LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockDelta {
        index: 0,
        kind: LiveBlockKind::Text,
        delta: "draft".to_owned(),
    }));
    assert_eq!(
        reducer.document().messages[0].model,
        Some(ModelRef {
            provider: "provider-one".to_owned(),
            id: "model-one".to_owned(),
        })
    );

    reducer.apply(LiveEvent::MessageEnd {
        message: json!({
            "id":"authoritative",
            "role":"assistant",
            "provider":"provider-two",
            "model":"model-two",
            "content":"final"
        }),
    });
    assert_eq!(
        reducer.document().messages[0].model,
        Some(ModelRef {
            provider: "provider-two".to_owned(),
            id: "model-two".to_owned(),
        })
    );
}

#[test]
fn user_start_and_end_upsert_by_stable_run_identity() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageStart {
        message: json!({
            "role":"user",
            "content":"hello",
            "timestamp":"start-only"
        }),
    });
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({
            "role":"user",
            "content":[{"type":"text","text":"hello"}],
            "timestamp":"authoritative",
            "providerMetadata":{"different":true}
        }),
    });
    let document = reducer.document();
    assert_eq!(document.messages.len(), 1);
    assert_eq!(
        document.messages[0].timestamp.as_deref(),
        Some("authoritative")
    );
}

#[test]
fn optimistic_running_then_agent_start_still_advances_run_identity() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.set_running();
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({"role":"user","content":"first"}),
    });
    reducer.apply(LiveEvent::AgentSettled);

    reducer.set_running();
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({"role":"user","content":"second"}),
    });

    let document = reducer.document();
    assert_eq!(document.messages.len(), 2);
    assert!(
        matches!(&document.messages[0].blocks[0], Block::Markdown(text) if text.source == "first")
    );
    assert!(
        matches!(&document.messages[1].blocks[0], Block::Markdown(text) if text.source == "second")
    );
}

#[test]
fn same_run_distinct_user_messages_do_not_overwrite_each_other() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.set_running();
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({"role":"user","content":"original prompt"}),
    });
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({"role":"user","content":"steer message"}),
    });

    let document = reducer.document();
    assert_eq!(document.messages.len(), 2);
}

#[test]
fn completed_history_is_arc_cached_across_draft_frames() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({
            "id":"completed",
            "role":"assistant",
            "content":[{"type":"text","text":"fixed"}]
        }),
    });
    let completed = reducer.document().messages[0].clone();
    for delta in ["a", "b", "c"] {
        reducer.apply(LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockDelta {
            index: 0,
            kind: LiveBlockKind::Text,
            delta: delta.to_owned(),
        }));
        let frame = reducer.document();
        assert!(Arc::ptr_eq(&completed, &frame.messages[0]));
    }
}

#[test]
fn static_history_items_and_minimap_are_reused_across_draft_frames() {
    let history_message = Arc::new(Message {
        id: "history".to_owned(),
        role: MessageRole::Assistant,
        timestamp: None,
        label: None,
        model: None,
        written_files: Vec::new(),
        blocks: vec![Block::Markdown(MarkdownBlock {
            source: "fixed history".to_owned(),
        })],
    });
    let history = ConversationDocument {
        session_id: "s".to_owned(),
        source_path: "fixture.jsonl".into(),
        cwd: std::env::temp_dir(),
        messages: Arc::from([history_message.clone()]),
        items: Arc::from([ConversationItem::Message(history_message.clone())]),
        minimap: Arc::from([MinimapNode {
            message_id: "history".to_owned(),
            turn: 0,
            role: MessageRole::Assistant,
            label: "history".to_owned(),
            level: None,
        }]),
        diagnostics: Arc::from([]),
    };
    let mut reducer = LiveSessionReducer::new(history);
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageStart {
        message: json!({"role":"assistant","content":[]}),
    });
    for delta in ["a", "b", "c"] {
        reducer.apply(LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockDelta {
            index: 0,
            kind: LiveBlockKind::Text,
            delta: delta.to_owned(),
        }));
        let frame = reducer.document();
        assert!(matches!(
            &frame.items[0],
            ConversationItem::Message(message) if Arc::ptr_eq(message, &history_message)
        ));
        assert_eq!(frame.minimap[0].message_id, "history");
        assert_eq!(frame.minimap[0].turn, 0);
    }
}

#[test]
fn history_and_live_minimap_turns_are_continuous() {
    let user = Arc::new(Message {
        id: "history-user".to_owned(),
        role: MessageRole::User,
        timestamp: None,
        label: None,
        model: None,
        written_files: Vec::new(),
        blocks: vec![Block::Markdown(MarkdownBlock {
            source: "old".to_owned(),
        })],
    });
    let history = ConversationDocument {
        session_id: "s".to_owned(),
        source_path: "fixture.jsonl".into(),
        cwd: std::env::temp_dir(),
        messages: Arc::from([user.clone()]),
        items: Arc::from([ConversationItem::Message(user)]),
        minimap: Arc::from([MinimapNode {
            message_id: "history-user".to_owned(),
            turn: 1,
            role: MessageRole::User,
            label: "old".to_owned(),
            level: None,
        }]),
        diagnostics: Arc::from([]),
    };
    let mut reducer = LiveSessionReducer::new(history);
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({"id":"live-user","role":"user","content":"new"}),
    });
    let document = reducer.document();
    assert_eq!(document.minimap[0].turn, 1);
    assert_eq!(document.minimap[1].turn, 2);
}

#[test]
fn tool_progress_replaces_accumulated_result_and_queue_snapshot_replaces() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({
            "role":"assistant",
            "content":[{"type":"toolCall","id":"call","name":"bash","arguments":{"command":"test"}}]
        }),
    });
    reducer.apply(LiveEvent::ToolExecutionStart {
        id: "call".into(),
        name: "bash".into(),
        arguments: json!({"command":"test"}),
    });
    reducer.apply(LiveEvent::ToolExecutionUpdate {
        id: "call".into(),
        name: "bash".into(),
        arguments: json!({"command":"test"}),
        partial_result: json!({"content":[{"type":"text","text":"old"}]}),
    });
    reducer.apply(LiveEvent::ToolExecutionUpdate {
        id: "call".into(),
        name: "bash".into(),
        arguments: json!({"command":"test"}),
        partial_result: json!({"content":[{"type":"text","text":"new cumulative"}]}),
    });
    reducer.apply(LiveEvent::QueueUpdate {
        steering: vec!["one".into(), "two".into()],
        follow_up: vec!["later".into()],
    });
    reducer.apply(LiveEvent::QueueUpdate {
        steering: vec!["replacement".into()],
        follow_up: Vec::new(),
    });

    assert_eq!(reducer.steering_queue(), ["replacement"]);
    assert!(reducer.follow_up_queue().is_empty());
    let document = reducer.document();
    let Block::Tool(tool) = &document.messages[0].blocks[0] else {
        panic!("expected tool")
    };
    assert_eq!(tool.status, ToolStatus::Pending);
    assert!(matches!(&tool.output[0], ToolOutput::Ansi(output) if output.text == "new cumulative"));
}

#[test]
fn active_tail_process_stays_expanded_until_settled() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply(LiveEvent::AgentStart);
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({"id":"u","role":"user","content":"question"}),
    });
    reducer.apply(LiveEvent::MessageEnd {
        message: json!({
            "id":"mixed",
            "role":"assistant",
            "content":[
                {"type":"thinking","thinking":"reasoning"},
                {"type":"text","text":"provisional answer"}
            ]
        }),
    });
    let running = reducer.document();
    assert!(matches!(
        &running.items[1],
        ConversationItem::Process(group) if !group.collapsible && group.message_count == 1
    ));
    assert_eq!(running.minimap.len(), 1);

    reducer.apply(LiveEvent::AgentSettled);
    let settled = reducer.document();
    assert!(matches!(
        &settled.items[1],
        ConversationItem::Process(group) if group.collapsible
    ));
    assert!(matches!(
        &settled.items[2],
        ConversationItem::Message(message) if message.id == "mixed"
    ));
    assert_eq!(settled.minimap.len(), 2);
}

#[test]
fn agent_end_is_not_idle_abort_waits_for_settled() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.apply(LiveEvent::AgentStart);
    assert_eq!(reducer.phase(), LivePhase::Running);
    reducer.set_stopping();
    reducer.apply(LiveEvent::AgentEnd);
    assert_eq!(reducer.phase(), LivePhase::Stopping);
    let outcome = reducer.apply(LiveEvent::AgentSettled);
    assert!(outcome.settled);
    assert_eq!(reducer.phase(), LivePhase::Idle);
}

#[test]
fn abort_error_restores_running_only_while_stopping() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    reducer.set_stopping();
    assert!(reducer.restore_running_if_stopping());
    assert_eq!(reducer.phase(), LivePhase::Running);

    reducer.apply(LiveEvent::AgentSettled);
    assert!(!reducer.restore_running_if_stopping());
    assert_eq!(reducer.phase(), LivePhase::Idle);
}

#[test]
fn out_of_order_and_burst_updates_degrade_safely() {
    let mut reducer = LiveSessionReducer::empty("s", "fixture.jsonl");
    let mut events = Vec::new();
    for _ in 0..2048 {
        events.push(LiveEvent::MessageUpdate(LiveAssistantUpdate::BlockDelta {
            index: 0,
            kind: LiveBlockKind::Text,
            delta: "x".into(),
        }));
    }
    events.push(LiveEvent::AgentEnd);
    events.push(LiveEvent::AgentSettled);
    let outcome = reducer.apply_batch(events);
    assert!(outcome.settled);
    assert_eq!(reducer.phase(), LivePhase::Idle);
    let document = reducer.document();
    assert!(
        matches!(&document.messages[0].blocks[0], Block::Markdown(text) if text.source.len() == 2048)
    );
    assert!(!document.diagnostics.is_empty());
}
