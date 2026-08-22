//! 活会话事件的纯逻辑 reducer。
//!
//! RPC wire 类型刻意不泄漏到本 crate；app 只需把事件投影成这里的 `LiveEvent`。

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use serde_json::Value;

use crate::{
    Block, ConversationDocument, ConversationItem, Message, MessageRole, MinimapNode, ModelRef,
    NoticeBlock, RenderDiagnostic, ToolCard, ToolOutput, ToolStatus, parse_ansi,
    parse_unified_diff,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePhase {
    Idle,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveBlockKind {
    Text,
    Thinking,
    ToolCall,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiveAssistantUpdate {
    Start,
    BlockStart {
        index: usize,
        kind: LiveBlockKind,
    },
    BlockDelta {
        index: usize,
        kind: LiveBlockKind,
        delta: String,
    },
    BlockEnd {
        index: usize,
        kind: LiveBlockKind,
        content: Value,
    },
    Done,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    AgentStart,
    AgentEnd,
    AgentSettled,
    MessageStart {
        message: Value,
    },
    MessageUpdate(LiveAssistantUpdate),
    MessageEnd {
        message: Value,
    },
    ToolExecutionStart {
        id: String,
        name: String,
        arguments: Value,
    },
    ToolExecutionUpdate {
        id: String,
        name: String,
        arguments: Value,
        partial_result: Value,
    },
    ToolExecutionEnd {
        id: String,
        name: String,
        result: Value,
        is_error: bool,
    },
    QueueUpdate {
        steering: Vec<String>,
        follow_up: Vec<String>,
    },
    Diagnostic(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReduceOutcome {
    pub changed: bool,
    pub follow_tail: bool,
    pub settled: bool,
}

#[derive(Debug, Clone)]
struct DraftBlock {
    kind: LiveBlockKind,
    accumulated: String,
    complete: Option<Value>,
}

impl DraftBlock {
    fn new(kind: LiveBlockKind) -> Self {
        Self {
            kind,
            accumulated: String::new(),
            complete: None,
        }
    }
}

#[derive(Debug, Clone)]
struct DraftMessage {
    seed: Value,
    blocks: Vec<Option<DraftBlock>>,
}

impl DraftMessage {
    fn new(seed: Value) -> Self {
        Self {
            seed,
            blocks: Vec::new(),
        }
    }

    fn block_mut(&mut self, index: usize, kind: LiveBlockKind) -> &mut DraftBlock {
        if self.blocks.len() <= index {
            self.blocks.resize_with(index + 1, || None);
        }
        let block = self.blocks[index].get_or_insert_with(|| DraftBlock::new(kind));
        if block.kind != kind {
            *block = DraftBlock::new(kind);
        }
        block
    }

    fn snapshot(&self) -> Value {
        let mut message = self.seed.clone();
        let Value::Object(object) = &mut message else {
            message = serde_json::json!({ "role": "assistant" });
            return message;
        };
        object.insert("role".to_owned(), Value::String("assistant".to_owned()));
        let content = self
            .blocks
            .iter()
            .filter_map(|block| block.as_ref())
            .map(|block| {
                if let Some(complete) = &block.complete {
                    return match block.kind {
                        LiveBlockKind::Text => serde_json::json!({"type":"text","text":complete.as_str().unwrap_or_default()}),
                        LiveBlockKind::Thinking => serde_json::json!({"type":"thinking","thinking":complete.as_str().unwrap_or_default()}),
                        LiveBlockKind::ToolCall => complete.clone(),
                    };
                }
                match block.kind {
                    LiveBlockKind::Text => serde_json::json!({"type":"text","text":block.accumulated}),
                    LiveBlockKind::Thinking => serde_json::json!({"type":"thinking","thinking":block.accumulated}),
                    LiveBlockKind::ToolCall => serde_json::from_str(&block.accumulated).unwrap_or_else(|_| {
                        serde_json::json!({
                            "type":"toolCall",
                            "id":format!("streaming-tool-{}", block.accumulated.len()),
                            "name":"tool",
                            "arguments":{},
                            "streamingArguments":block.accumulated
                        })
                    }),
                }
            })
            .collect();
        object.insert("content".to_owned(), Value::Array(content));
        message
    }
}

#[derive(Debug, Clone)]
struct LiveTool {
    name: String,
    arguments: Value,
    result: Option<Value>,
    status: ToolStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MessageIdentity {
    Id(String),
    RunRoleContent {
        run: u64,
        role: String,
        content: String,
    },
}

#[derive(Debug, Clone)]
struct CompletedMessage {
    value: Value,
    rendered: Arc<Message>,
}

#[derive(Debug, Clone)]
pub struct LiveSessionReducer {
    session_id: String,
    source_path: PathBuf,
    history: Arc<ConversationDocument>,
    completed: Vec<CompletedMessage>,
    completed_indexes: HashMap<MessageIdentity, usize>,
    draft: Option<DraftMessage>,
    tools: HashMap<String, LiveTool>,
    phase: LivePhase,
    run_sequence: u64,
    steering: Vec<String>,
    follow_up: Vec<String>,
    diagnostics: Vec<RenderDiagnostic>,
    cached_messages: Arc<[Arc<Message>]>,
    cached_items: Arc<[ConversationItem]>,
    cached_minimap: Arc<[MinimapNode]>,
    cached_diagnostics: Arc<[RenderDiagnostic]>,
    structure_dirty: bool,
    draft_dirty: bool,
    diagnostics_dirty: bool,
}

impl LiveSessionReducer {
    pub fn new(history: ConversationDocument) -> Self {
        let history = Arc::new(history);
        Self {
            session_id: history.session_id.clone(),
            source_path: history.source_path.clone(),
            cached_messages: history.messages.clone(),
            cached_items: history.items.clone(),
            cached_minimap: history.minimap.clone(),
            cached_diagnostics: history.diagnostics.clone(),
            history,
            completed: Vec::new(),
            completed_indexes: HashMap::new(),
            draft: None,
            tools: HashMap::new(),
            phase: LivePhase::Idle,
            run_sequence: 0,
            steering: Vec::new(),
            follow_up: Vec::new(),
            diagnostics: Vec::new(),
            structure_dirty: false,
            draft_dirty: false,
            diagnostics_dirty: false,
        }
    }

    pub fn empty(session_id: impl Into<String>, source_path: impl Into<PathBuf>) -> Self {
        let session_id = session_id.into();
        let source_path = source_path.into();
        Self::new(ConversationDocument {
            session_id,
            source_path,
            cwd: PathBuf::new(),
            messages: Arc::from([]),
            items: Arc::from([]),
            minimap: Arc::from([]),
            diagnostics: Arc::from([]),
        })
    }

    pub const fn phase(&self) -> LivePhase {
        self.phase
    }

    pub fn set_running(&mut self) {
        self.phase = LivePhase::Running;
    }

    pub fn set_stopping(&mut self) {
        self.phase = LivePhase::Stopping;
    }

    pub fn restore_phase(&mut self, phase: LivePhase) {
        self.phase = phase;
    }

    pub fn restore_running_if_stopping(&mut self) -> bool {
        if self.phase != LivePhase::Stopping {
            return false;
        }
        self.phase = LivePhase::Running;
        true
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.phase = LivePhase::Error;
        self.push_diagnostic(message);
    }

    pub fn steering_queue(&self) -> &[String] {
        &self.steering
    }

    pub fn follow_up_queue(&self) -> &[String] {
        &self.follow_up
    }

    /// fresh RPC 启动后，`get_state` 才给出 pi 分配的真实身份。
    /// 只更新身份，不触碰已缓存的消息与流式草稿。
    pub fn set_session_identity(
        &mut self,
        session_id: impl Into<String>,
        source_path: impl Into<PathBuf>,
    ) {
        self.session_id = session_id.into();
        self.source_path = source_path.into();
    }

    /// 用 settled 后从持久文件重读的权威快照替换临时流式状态。
    pub fn calibrate(&mut self, history: ConversationDocument) {
        self.session_id = history.session_id.clone();
        self.source_path = history.source_path.clone();
        self.history = Arc::new(history);
        self.completed.clear();
        self.completed_indexes.clear();
        self.draft = None;
        self.tools.clear();
        self.diagnostics.clear();
        self.structure_dirty = true;
        self.draft_dirty = false;
        self.diagnostics_dirty = true;
    }

    pub fn apply_batch<I>(&mut self, events: I) -> ReduceOutcome
    where
        I: IntoIterator<Item = LiveEvent>,
    {
        let mut outcome = ReduceOutcome::default();
        for event in events {
            let next = self.apply(event);
            outcome.changed |= next.changed;
            outcome.follow_tail |= next.follow_tail;
            outcome.settled |= next.settled;
        }
        outcome
    }

    pub fn apply(&mut self, event: LiveEvent) -> ReduceOutcome {
        let mut outcome = ReduceOutcome {
            changed: true,
            follow_tail: false,
            settled: false,
        };
        match event {
            LiveEvent::AgentStart => {
                // dispatch 会乐观地把 phase 置为 Running，run 序号不能依赖 phase。
                self.run_sequence = self.run_sequence.wrapping_add(1);
                self.phase = LivePhase::Running;
            }
            // agent_end 之后仍可能 retry/compaction/queued continuation，不能提前 idle。
            LiveEvent::AgentEnd => {}
            LiveEvent::AgentSettled => {
                self.phase = LivePhase::Idle;
                // 活跃尾 turn 在 settled 后需要从展开态切换为已完成折叠态。
                self.structure_dirty = true;
                outcome.settled = true;
            }
            LiveEvent::MessageStart { message } => {
                outcome.follow_tail = true;
                match message.get("role").and_then(Value::as_str) {
                    Some("assistant") => {
                        self.draft = Some(DraftMessage::new(message));
                        self.draft_dirty = true;
                    }
                    _ => self.upsert_completed(message),
                }
            }
            LiveEvent::MessageUpdate(update) => {
                outcome.follow_tail = true;
                self.apply_update(update);
                self.draft_dirty = true;
            }
            LiveEvent::MessageEnd { message } => {
                outcome.follow_tail = true;
                if message.get("role").and_then(Value::as_str) == Some("assistant") {
                    self.draft = None;
                    self.draft_dirty = true;
                }
                self.upsert_completed(message);
            }
            LiveEvent::ToolExecutionStart {
                id,
                name,
                arguments,
            } => {
                outcome.follow_tail = true;
                self.tools.insert(
                    id,
                    LiveTool {
                        name,
                        arguments,
                        result: None,
                        status: ToolStatus::Pending,
                    },
                );
                self.rerender_completed_tools();
            }
            LiveEvent::ToolExecutionUpdate {
                id,
                name,
                arguments,
                partial_result,
            } => {
                outcome.follow_tail = true;
                let tool = self.tools.entry(id).or_insert_with(|| LiveTool {
                    name: name.clone(),
                    arguments: arguments.clone(),
                    result: None,
                    status: ToolStatus::Pending,
                });
                tool.name = name;
                tool.arguments = arguments;
                // partialResult 是累计值，必须替换而不是追加。
                tool.result = Some(partial_result);
                self.rerender_completed_tools();
            }
            LiveEvent::ToolExecutionEnd {
                id,
                name,
                result,
                is_error,
            } => {
                outcome.follow_tail = true;
                let tool = self.tools.entry(id).or_insert_with(|| LiveTool {
                    name: name.clone(),
                    arguments: Value::Null,
                    result: None,
                    status: ToolStatus::Pending,
                });
                tool.name = name;
                tool.result = Some(result);
                tool.status = if is_error {
                    ToolStatus::Error
                } else {
                    ToolStatus::Success
                };
                self.rerender_completed_tools();
            }
            LiveEvent::QueueUpdate {
                steering,
                follow_up,
            } => {
                // queue_update 是完整权威快照。
                self.steering = steering;
                self.follow_up = follow_up;
            }
            LiveEvent::Diagnostic(message) => self.push_diagnostic(message),
        }
        outcome
    }

    fn identity(&self, message: &Value) -> MessageIdentity {
        if let Some(id) = message.get("id").and_then(Value::as_str) {
            MessageIdentity::Id(id.to_owned())
        } else {
            MessageIdentity::RunRoleContent {
                run: self.run_sequence,
                role: message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                // message_start/message_end 的字段可不同，但同一消息的可见 content 稳定；
                // 同一 run 的 steer/follow-up 文本不同，因此不会互相覆盖。
                content: canonical_message_content(message),
            }
        }
    }

    fn upsert_completed(&mut self, message: Value) {
        let identity = self.identity(&message);
        if let Some(index) = self.completed_indexes.get(&identity).copied() {
            if let Some(rendered) = render_live_message(&message, index as u64, &self.tools) {
                self.completed[index] = CompletedMessage {
                    value: message,
                    rendered: Arc::new(rendered),
                };
                self.structure_dirty = true;
            }
            return;
        }
        let index = self.completed.len();
        if let Some(rendered) = render_live_message(&message, index as u64, &self.tools) {
            self.completed.push(CompletedMessage {
                value: message,
                rendered: Arc::new(rendered),
            });
            self.completed_indexes.insert(identity, index);
            self.structure_dirty = true;
        }
    }

    fn rerender_completed_tools(&mut self) {
        let mut changed = false;
        for (index, completed) in self.completed.iter_mut().enumerate() {
            if completed
                .rendered
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Tool(_)))
                && let Some(rendered) =
                    render_live_message(&completed.value, index as u64, &self.tools)
            {
                completed.rendered = Arc::new(rendered);
                changed = true;
            }
        }
        self.structure_dirty |= changed;
        self.draft_dirty |= self.draft.is_some();
    }

    fn push_diagnostic(&mut self, message: impl Into<String>) {
        self.diagnostics.push(RenderDiagnostic {
            entry_id: None,
            message: message.into(),
        });
        self.diagnostics_dirty = true;
    }

    fn apply_update(&mut self, update: LiveAssistantUpdate) {
        if self.draft.is_none() {
            self.push_diagnostic("message_update 早于 message_start；已创建兼容草稿");
            self.draft = Some(DraftMessage::new(serde_json::json!({"role":"assistant"})));
        }
        if let LiveAssistantUpdate::Error { message } = update {
            self.push_diagnostic(message);
            return;
        }
        let draft = self.draft.as_mut().expect("draft initialized");
        match update {
            LiveAssistantUpdate::Start | LiveAssistantUpdate::Done => {}
            LiveAssistantUpdate::Error { .. } => unreachable!(),
            LiveAssistantUpdate::BlockStart { index, kind } => {
                draft.block_mut(index, kind);
            }
            LiveAssistantUpdate::BlockDelta { index, kind, delta } => {
                draft.block_mut(index, kind).accumulated.push_str(&delta);
            }
            LiveAssistantUpdate::BlockEnd {
                index,
                kind,
                content,
            } => {
                let block = draft.block_mut(index, kind);
                block.complete = Some(content);
            }
        }
    }

    /// 刷新可共享的文档快照。已定稿历史以 Arc 缓存；每帧最多重渲染当前草稿。
    pub fn document(&mut self) -> ConversationDocument {
        let needs_messages = self.structure_dirty || self.draft_dirty;
        if needs_messages {
            let mut live_messages =
                Vec::with_capacity(self.completed.len() + usize::from(self.draft.is_some()));
            live_messages.extend(
                self.completed
                    .iter()
                    .map(|message| message.rendered.clone()),
            );
            if let Some(draft) = &self.draft
                && let Some(message) =
                    render_live_message(&draft.snapshot(), self.completed.len() as u64, &self.tools)
            {
                live_messages.push(Arc::new(message));
            }

            let active_tail = self.phase != LivePhase::Idle;
            // 历史文档已完成 written-files 与 turn 投影。流式帧只投影 live 段，
            // 再复用历史 item/message Arc；draft delta 不扫描或克隆整段历史。
            crate::attach_written_files(&mut live_messages, &self.history.cwd, active_tail);
            let (live_items, live_minimap) =
                crate::project_conversation(&live_messages, active_tail);
            let mut messages =
                Vec::with_capacity(self.history.messages.len() + live_messages.len());
            messages.extend(self.history.messages.iter().cloned());
            messages.extend(live_messages);
            let mut items = Vec::with_capacity(self.history.items.len() + live_items.len());
            items.extend(self.history.items.iter().cloned());
            items.extend(live_items);
            let turn_offset = self
                .history
                .minimap
                .iter()
                .map(|node| node.turn)
                .max()
                .unwrap_or(0);
            let mut minimap = Vec::with_capacity(self.history.minimap.len() + live_minimap.len());
            minimap.extend(self.history.minimap.iter().cloned());
            minimap.extend(live_minimap.into_iter().map(|mut node| {
                node.turn += turn_offset;
                node
            }));

            self.cached_items = items.into();
            self.cached_minimap = minimap.into();
            self.cached_messages = messages.into();
            self.structure_dirty = false;
            self.draft_dirty = false;
        }
        if self.diagnostics_dirty {
            let mut diagnostics =
                Vec::with_capacity(self.history.diagnostics.len() + self.diagnostics.len());
            diagnostics.extend(self.history.diagnostics.iter().cloned());
            diagnostics.extend(self.diagnostics.iter().cloned());
            self.cached_diagnostics = diagnostics.into();
            self.diagnostics_dirty = false;
        }
        ConversationDocument {
            session_id: self.session_id.clone(),
            source_path: self.source_path.clone(),
            cwd: self.history.cwd.clone(),
            messages: self.cached_messages.clone(),
            items: self.cached_items.clone(),
            minimap: self.cached_minimap.clone(),
            diagnostics: self.cached_diagnostics.clone(),
        }
    }
}

fn canonical_message_content(message: &Value) -> String {
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(|block| match block.get("type").and_then(Value::as_str) {
                Some("image") => canonical_image_identity(block),
                _ => block
                    .get("text")
                    .or_else(|| block.get("thinking"))
                    .and_then(Value::as_str)
                    .map_or_else(|| block.to_string(), str::to_owned),
            })
            .collect::<Vec<_>>()
            .join("\u{1f}"),
        other => other.to_string(),
    }
}

fn canonical_image_identity(value: &Value) -> String {
    let source = value.get("source");
    let mime = value
        .get("mimeType")
        .or_else(|| value.get("mime_type"))
        .or_else(|| source.and_then(|source| source.get("media_type")))
        .or_else(|| source.and_then(|source| source.get("mimeType")))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let data = value
        .get("data")
        .or_else(|| source.and_then(|source| source.get("data")))
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("image:{mime}:{}:{}", data.len(), stable_text_hash(data))
}

fn stable_text_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn render_live_message(
    value: &Value,
    sequence: u64,
    live_tools: &HashMap<String, LiveTool>,
) -> Option<Message> {
    let role = match value.get("role").and_then(Value::as_str) {
        Some("user") => MessageRole::User,
        Some("assistant") => MessageRole::Assistant,
        Some(_) => MessageRole::Unknown,
        None => return None,
    };
    let mut blocks = Vec::new();
    match value.get("content") {
        Some(Value::String(text)) => blocks.push(Block::Markdown(crate::MarkdownBlock {
            source: text.clone(),
        })),
        Some(Value::Array(content)) => {
            for item in content {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => blocks.push(Block::Markdown(crate::MarkdownBlock {
                        source: item
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })),
                    Some("thinking") => blocks.push(Block::Thinking(
                        item.get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    )),
                    Some("toolCall") => {
                        blocks.push(Block::Tool(render_live_tool(item, live_tools)))
                    }
                    Some("image") => blocks.push(Block::Image(crate::parse_image(item))),
                    Some(kind) => blocks.push(Block::Unknown(crate::UnknownBlock {
                        kind: kind.to_owned(),
                        text: item.to_string(),
                    })),
                    None => {}
                }
            }
        }
        _ => {}
    }
    if blocks.is_empty() && role == MessageRole::Assistant {
        blocks.push(Block::Notice(NoticeBlock {
            title: "Assistant 正在响应".to_owned(),
            text: "等待流式内容".to_owned(),
        }));
    }
    Some(Message {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("live-{sequence}")),
        role,
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned),
        label: None,
        model: live_model_ref(value),
        written_files: Vec::new(),
        blocks,
    })
}

fn live_model_ref(value: &Value) -> Option<ModelRef> {
    Some(ModelRef {
        provider: value.get("provider")?.as_str()?.to_owned(),
        id: value.get("model")?.as_str()?.to_owned(),
    })
}

fn render_live_tool(call: &Value, live_tools: &HashMap<String, LiveTool>) -> ToolCard {
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("streaming-tool")
        .to_owned();
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned();
    let arguments = call.get("arguments").cloned().unwrap_or(Value::Null);
    let live = live_tools.get(&id);
    let arguments = live.map_or(arguments, |tool| tool.arguments.clone());
    let input_json =
        serde_json::to_string_pretty(&arguments).unwrap_or_else(|_| arguments.to_string());
    let mut output = Vec::new();
    let details = live
        .and_then(|tool| tool.result.as_ref())
        .and_then(|result| result.get("details"))
        .cloned();
    if let Some(result) = live.and_then(|tool| tool.result.as_ref()) {
        if let Some(patch) = result
            .get("details")
            .and_then(|details| details.get("patch").or_else(|| details.get("diff")))
            .and_then(Value::as_str)
        {
            output.push(ToolOutput::Diff(parse_unified_diff(patch)));
        }
        append_live_tool_content(result.get("content").unwrap_or(result), &name, &mut output);
    }
    ToolCard {
        id,
        name: live.map_or(name, |tool| tool.name.clone()),
        preview: input_json.chars().take(240).collect(),
        arguments,
        input_json,
        status: live.map_or(ToolStatus::Pending, |tool| tool.status),
        output,
        details,
        orphan: false,
    }
}

fn append_live_tool_content(content: &Value, name: &str, output: &mut Vec<ToolOutput>) {
    match content {
        Value::String(text) => {
            if name.eq_ignore_ascii_case("bash") || text.contains('\u{1b}') {
                output.push(ToolOutput::Ansi(parse_ansi(text)));
            } else {
                output.push(ToolOutput::Text(text.clone()));
            }
        }
        Value::Array(items) => {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(text) = item.get("text").and_then(Value::as_str)
                {
                    append_live_tool_content(&Value::String(text.to_owned()), name, output);
                }
            }
        }
        Value::Null => {}
        other => output.push(ToolOutput::Text(other.to_string())),
    }
}
