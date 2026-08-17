//! 把静态 pi 会话转换成 owned、可跨线程传递的渲染文档。
//!
//! 本 crate 不依赖 GPUI。会话格式允许扩展和局部损坏，因此这里宁可保留未知内容与
//! diagnostics，也不因为一个坏块丢弃整段历史。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use pi_data::{EntryBase, SessionEntry, SessionFile};
use serde_json::Value;

const MAX_TEXT_CHARS: usize = 512 * 1024;
const MAX_IMAGE_BASE64_CHARS: usize = 8 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 6 * 1024 * 1024;
const PREVIEW_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationDocument {
    pub session_id: String,
    pub source_path: PathBuf,
    pub messages: Vec<Message>,
    pub minimap: Vec<MinimapNode>,
    pub diagnostics: Vec<RenderDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderDiagnostic {
    pub entry_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    Custom,
    Compaction,
    BranchSummary,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub timestamp: Option<String>,
    pub label: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Markdown(MarkdownBlock),
    Code(CodeBlock),
    Thinking(String),
    Tool(ToolCard),
    Diff(DiffBlock),
    Ansi(AnsiText),
    Image(ImageBlock),
    Frontmatter(FrontmatterCard),
    Notice(NoticeBlock),
    Unknown(UnknownBlock),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownBlock {
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    pub language: Option<String>,
    pub source: String,
    pub mermaid_source: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmatterCard {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub rows: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    Success,
    Error,
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCard {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub input_json: String,
    pub preview: String,
    pub status: ToolStatus,
    pub output: Vec<ToolOutput>,
    pub details: Option<Value>,
    pub orphan: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolOutput {
    Text(String),
    Ansi(AnsiText),
    Image(ImageBlock),
    Diff(DiffBlock),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffBlock {
    pub files: Vec<DiffFile>,
    pub raw: String,
    pub parsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    Header,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiText {
    pub text: String,
    pub spans: Vec<AnsiSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiSpan {
    pub range: std::ops::Range<usize>,
    pub style: AnsiStyle,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnsiStyle {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub foreground: Option<AnsiColor>,
    pub background: Option<AnsiColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageState {
    Inline,
    Remote,
    Unsupported,
    Invalid,
    TooLarge,
    Redacted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBlock {
    pub mime_type: Option<String>,
    pub state: ImageState,
    pub bytes: Option<Vec<u8>>,
    pub remote_url: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeBlock {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownBlock {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimapNode {
    pub message_id: String,
    pub turn: usize,
    pub role: MessageRole,
    pub label: String,
    pub level: Option<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentStats {
    pub messages: usize,
    pub blocks: usize,
    pub tools: usize,
    pub images: usize,
    pub diagnostics: usize,
}

impl ConversationDocument {
    pub fn stats(&self) -> DocumentStats {
        let mut stats = DocumentStats {
            messages: self.messages.len(),
            diagnostics: self.diagnostics.len(),
            ..DocumentStats::default()
        };
        for message in &self.messages {
            stats.blocks += message.blocks.len();
            for block in &message.blocks {
                match block {
                    Block::Tool(tool) => {
                        stats.tools += 1;
                        stats.images += tool
                            .output
                            .iter()
                            .filter(|output| matches!(output, ToolOutput::Image(_)))
                            .count();
                    }
                    Block::Image(_) => stats.images += 1,
                    _ => {}
                }
            }
        }
        stats
    }

    pub fn text_snapshot(&self) -> String {
        let mut out = String::new();
        for message in &self.messages {
            out.push_str(&format!("[{:#?}] {}\n", message.role, message.id));
            for block in &message.blocks {
                snapshot_block(block, &mut out);
            }
        }
        if !self.minimap.is_empty() {
            out.push_str("[Minimap]\n");
            for node in &self.minimap {
                out.push_str(&format!(
                    "{}:{}:{}\n",
                    node.turn, node.message_id, node.label
                ));
            }
        }
        out
    }
}

pub fn render_session(session: &SessionFile) -> ConversationDocument {
    let mut diagnostics = session
        .diagnostics
        .iter()
        .map(|diagnostic| RenderDiagnostic {
            entry_id: None,
            message: format!("JSONL 第 {} 行：{}", diagnostic.line, diagnostic.message),
        })
        .collect::<Vec<_>>();
    let selected = selected_entry_indexes(session, &mut diagnostics);
    let mut results = HashMap::<String, ToolResultData>::new();
    for index in &selected {
        if let SessionEntry::Message { message, .. } = &session.entries[*index]
            && message.get("role").and_then(Value::as_str) == Some("toolResult")
        {
            let data = parse_tool_result(message, &mut diagnostics);
            if let Some(id) = data.tool_call_id.clone()
                && results.insert(id.clone(), data).is_some()
            {
                diagnostics.push(RenderDiagnostic {
                    entry_id: Some(id),
                    message: "重复 toolResult，使用路径上最后一个结果".to_owned(),
                });
            }
        }
    }

    let mut consumed_results = HashSet::new();
    let mut messages = Vec::new();
    for index in selected {
        let entry = &session.entries[index];
        match entry {
            SessionEntry::Message { base, message } => {
                let fallback = messages.len();
                let role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if role == "toolResult" {
                    let id = message
                        .get("toolCallId")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !consumed_results.contains(id) {
                        messages.push(orphan_result_message(
                            base,
                            message,
                            fallback,
                            &mut diagnostics,
                        ));
                    }
                } else if let Some(rendered) = render_message(
                    base,
                    message,
                    fallback,
                    &results,
                    &mut consumed_results,
                    &mut diagnostics,
                ) {
                    messages.push(rendered);
                }
            }
            SessionEntry::CustomMessage {
                base,
                custom_type,
                content,
                display,
                ..
            } if display.unwrap_or(true) => {
                let mut blocks = Vec::new();
                render_content(content, &mut blocks, &mut diagnostics, base.id.as_deref());
                if blocks.is_empty() {
                    blocks.push(Block::Unknown(UnknownBlock {
                        kind: "custom_message".to_owned(),
                        text: visible_json(content),
                    }));
                }
                messages.push(Message {
                    id: entry_id(base, messages.len()),
                    role: MessageRole::Custom,
                    timestamp: base.timestamp.clone(),
                    label: Some(
                        custom_type
                            .clone()
                            .unwrap_or_else(|| "自定义消息".to_owned()),
                    ),
                    blocks,
                });
            }
            SessionEntry::Compaction {
                base,
                summary,
                tokens_before,
                ..
            } => messages.push(Message {
                id: entry_id(base, messages.len()),
                role: MessageRole::Compaction,
                timestamp: base.timestamp.clone(),
                label: Some("上下文压缩".to_owned()),
                blocks: vec![Block::Notice(NoticeBlock {
                    title: tokens_before.map_or_else(
                        || "上下文压缩".to_owned(),
                        |tokens| format!("上下文压缩 · {tokens} tokens"),
                    ),
                    text: summary.clone().unwrap_or_else(|| "没有摘要内容".to_owned()),
                })],
            }),
            SessionEntry::BranchSummary {
                base,
                summary,
                from_id,
                ..
            } => messages.push(Message {
                id: entry_id(base, messages.len()),
                role: MessageRole::BranchSummary,
                timestamp: base.timestamp.clone(),
                label: Some("分支摘要".to_owned()),
                blocks: vec![Block::Notice(NoticeBlock {
                    title: from_id.as_ref().map_or_else(
                        || "分支摘要".to_owned(),
                        |id| format!("分支摘要 · from {id}"),
                    ),
                    text: summary.clone().unwrap_or_else(|| "没有摘要内容".to_owned()),
                })],
            }),
            SessionEntry::Unknown {
                entry_type,
                base,
                raw,
            } => messages.push(Message {
                id: entry_id(base, messages.len()),
                role: MessageRole::Unknown,
                timestamp: base.timestamp.clone(),
                label: Some("未知会话条目".to_owned()),
                blocks: vec![Block::Unknown(UnknownBlock {
                    kind: entry_type.clone(),
                    text: visible_json(raw),
                })],
            }),
            // model/thinking/label/session_info/custom 都是元数据，不占据对话正文。
            _ => {}
        }
    }

    let minimap = build_minimap(&messages);
    ConversationDocument {
        session_id: session.header.id.clone(),
        source_path: session.path.clone(),
        messages,
        minimap,
        diagnostics,
    }
}

pub fn render_path(path: impl AsRef<Path>) -> Result<ConversationDocument, pi_data::SessionError> {
    pi_data::load_session(path).map(|session| render_session(&session))
}

fn selected_entry_indexes(
    session: &SessionFile,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> Vec<usize> {
    let has_tree = session
        .entries
        .iter()
        .any(|entry| entry_base(entry).id.is_some());
    if !has_tree {
        return (0..session.entries.len()).collect();
    }

    let mut by_id = HashMap::new();
    for (index, entry) in session.entries.iter().enumerate() {
        if let Some(id) = &entry_base(entry).id {
            by_id.insert(id.as_str(), index);
        }
    }
    let Some(mut index) = session
        .entries
        .iter()
        .enumerate()
        .rev()
        .find(|(_, entry)| entry_base(entry).id.is_some())
        .map(|(index, _)| index)
    else {
        return (0..session.entries.len()).collect();
    };

    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    loop {
        let base = entry_base(&session.entries[index]);
        let Some(id) = base.id.as_deref() else { break };
        if !visited.insert(id.to_owned()) {
            diagnostics.push(RenderDiagnostic {
                entry_id: Some(id.to_owned()),
                message: "会话 parentId 形成循环；已在循环处截断".to_owned(),
            });
            break;
        }
        chain.push(index);
        let Some(parent_id) = base.parent_id.as_deref() else {
            break;
        };
        let Some(parent) = by_id.get(parent_id).copied() else {
            diagnostics.push(RenderDiagnostic {
                entry_id: Some(id.to_owned()),
                message: format!("找不到 parentId {parent_id}；已保留可达路径"),
            });
            break;
        };
        index = parent;
    }
    chain.reverse();

    // v1 会话可能在树字段中途缺失。若祖先链只有很少条目，则线性兼容比静默丢历史可靠。
    if chain.len() < 2 && session.entries.len() > 1 {
        diagnostics.push(RenderDiagnostic {
            entry_id: None,
            message: "树字段不完整，按线性顺序兼容渲染".to_owned(),
        });
        (0..session.entries.len()).collect()
    } else {
        chain
    }
}

fn entry_base(entry: &SessionEntry) -> &EntryBase {
    match entry {
        SessionEntry::Message { base, .. }
        | SessionEntry::ModelChange { base, .. }
        | SessionEntry::ThinkingLevelChange { base, .. }
        | SessionEntry::Compaction { base, .. }
        | SessionEntry::BranchSummary { base, .. }
        | SessionEntry::Custom { base, .. }
        | SessionEntry::CustomMessage { base, .. }
        | SessionEntry::Label { base, .. }
        | SessionEntry::SessionInfo { base, .. }
        | SessionEntry::Unknown { base, .. } => base,
    }
}

fn render_message(
    base: &EntryBase,
    message: &Value,
    fallback: usize,
    results: &HashMap<String, ToolResultData>,
    consumed_results: &mut HashSet<String>,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> Option<Message> {
    let role_name = message.get("role").and_then(Value::as_str);
    if role_name == Some("bashExecution") {
        return Some(Message {
            id: entry_id(base, fallback),
            role: MessageRole::Assistant,
            timestamp: base.timestamp.clone(),
            label: Some("Bash execution".to_owned()),
            blocks: vec![Block::Tool(render_bash_execution(message))],
        });
    }
    let role = match role_name {
        Some("user") => MessageRole::User,
        Some("assistant") => MessageRole::Assistant,
        Some(role) => {
            diagnostics.push(RenderDiagnostic {
                entry_id: base.id.clone(),
                message: format!("未知 message role: {role}"),
            });
            MessageRole::Unknown
        }
        None => MessageRole::Unknown,
    };
    let mut blocks = Vec::new();
    if role == MessageRole::Assistant
        && let Some(error) = message.get("errorMessage").and_then(Value::as_str)
    {
        blocks.push(Block::Notice(NoticeBlock {
            title: "Assistant 错误".to_owned(),
            text: error.to_owned(),
        }));
    }
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::Array(items) => {
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("toolCall") {
                    let tool = render_tool_call(item, results, consumed_results, diagnostics);
                    blocks.push(Block::Tool(tool));
                } else {
                    render_content(item, &mut blocks, diagnostics, base.id.as_deref());
                }
            }
        }
        _ => render_content(content, &mut blocks, diagnostics, base.id.as_deref()),
    }
    if blocks.is_empty() && role == MessageRole::Assistant {
        blocks.push(Block::Notice(NoticeBlock {
            title: "空 assistant 消息".to_owned(),
            text: message
                .get("stopReason")
                .and_then(Value::as_str)
                .unwrap_or("没有正文")
                .to_owned(),
        }));
    }
    (!blocks.is_empty()).then(|| Message {
        id: entry_id(base, fallback),
        role,
        timestamp: base.timestamp.clone(),
        label: None,
        blocks,
    })
}

fn render_content(
    content: &Value,
    blocks: &mut Vec<Block>,
    diagnostics: &mut Vec<RenderDiagnostic>,
    entry_id: Option<&str>,
) {
    match content {
        Value::String(text) => blocks.extend(split_markdown(text, diagnostics, entry_id)),
        Value::Array(items) => {
            for item in items {
                render_content(item, blocks, diagnostics, entry_id);
            }
        }
        Value::Object(map) => match map.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    blocks.extend(split_markdown(text, diagnostics, entry_id));
                }
            }
            Some("thinking") => {
                if let Some(text) = map.get("thinking").and_then(Value::as_str) {
                    blocks.push(Block::Thinking(limit_text(text)));
                }
            }
            Some("image") => blocks.push(Block::Image(parse_image(content))),
            Some("bashExecution") => blocks.push(Block::Tool(render_bash_execution(content))),
            Some(kind) => {
                diagnostics.push(RenderDiagnostic {
                    entry_id: entry_id.map(str::to_owned),
                    message: format!("未知 content block: {kind}"),
                });
                blocks.push(Block::Unknown(UnknownBlock {
                    kind: kind.to_owned(),
                    text: visible_json(content),
                }));
            }
            None if !map.is_empty() => blocks.push(Block::Unknown(UnknownBlock {
                kind: "object".to_owned(),
                text: visible_json(content),
            })),
            None => {}
        },
        Value::Null => {}
        _ => blocks.push(Block::Unknown(UnknownBlock {
            kind: "content".to_owned(),
            text: visible_json(content),
        })),
    }
}

fn split_markdown(
    text: &str,
    diagnostics: &mut Vec<RenderDiagnostic>,
    entry_id: Option<&str>,
) -> Vec<Block> {
    let (frontmatter, body) = parse_frontmatter(text);
    let mut blocks = Vec::new();
    if let Some(frontmatter) = frontmatter {
        blocks.push(Block::Frontmatter(frontmatter));
    }
    let lines = body.split_inclusive('\n').collect::<Vec<_>>();
    let mut markdown = String::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if !markdown.is_empty() {
                blocks.push(Block::Markdown(MarkdownBlock {
                    source: sanitize_remote_markdown_images(&std::mem::take(&mut markdown)),
                }));
            }
            let language = rest.split_whitespace().next().filter(|s| !s.is_empty());
            let mut source = String::new();
            let mut closed = false;
            index += 1;
            while index < lines.len() {
                if lines[index].trim_start().starts_with("```") {
                    closed = true;
                    break;
                }
                source.push_str(lines[index]);
                index += 1;
            }
            if !closed {
                diagnostics.push(RenderDiagnostic {
                    entry_id: entry_id.map(str::to_owned),
                    message: "未闭合 Markdown code fence，按代码块显示到正文末尾".to_owned(),
                });
            }
            let (source, truncated) = limit_text_with_flag(&source);
            blocks.push(Block::Code(CodeBlock {
                language: language.map(normalize_language),
                mermaid_source: language.is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid")),
                source,
                truncated,
            }));
            if closed {
                index += 1;
            }
            continue;
        }
        markdown.push_str(line);
        index += 1;
    }
    if !markdown.is_empty() {
        blocks.push(Block::Markdown(MarkdownBlock {
            source: sanitize_remote_markdown_images(&markdown),
        }));
    }
    blocks
}

fn sanitize_remote_markdown_images(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("![") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start + 2..];
        let Some(alt_end) = candidate.find("](") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let url_start = alt_end + 2;
        let Some(url_end) = candidate[url_start..].find(')') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let url_end = url_start + url_end;
        let alt = &candidate[..alt_end];
        let url = &candidate[url_start..url_end];
        if url.trim_start().starts_with("http://") || url.trim_start().starts_with("https://") {
            output.push_str(&format!("[远程图片未自动加载：{alt}]({url})"));
        } else {
            output.push_str(&rest[start..start + 2 + url_end + 1]);
        }
        rest = &candidate[url_end + 1..];
    }
    output.push_str(rest);
    output
}

fn parse_frontmatter(text: &str) -> (Option<FrontmatterCard>, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut consumed = 0;
    let mut yaml = String::new();
    let mut body = None;
    for line in rest.split_inclusive('\n') {
        consumed += line.len();
        if line.trim_end_matches(['\r', '\n']) == "---" {
            body = Some(&rest[consumed..]);
            break;
        }
        yaml.push_str(line);
    }
    let Some(body) = body else {
        return (None, text);
    };
    let Some(card) = parse_simple_yaml(&yaml) else {
        // 固定行为：坏 YAML 完整保留为普通 Markdown，不吞 fence。
        return (None, text);
    };
    (Some(card), body)
}

fn parse_simple_yaml(yaml: &str) -> Option<FrontmatterCard> {
    let mut rows = Vec::new();
    let mut title = None;
    let mut tags = Vec::new();
    for line in yaml.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with([' ', '\t', '-']) {
            return None;
        }
        let (key, value) = line.split_once(':')?;
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let value = unquote_yaml(value.trim())?;
        match key {
            "title" => title = (!value.is_empty()).then_some(value),
            "tags" => tags = parse_yaml_tags(&value)?,
            _ => rows.push((key.to_owned(), value)),
        }
    }
    if title.is_none() && tags.is_empty() && rows.is_empty() {
        return None;
    }
    Some(FrontmatterCard { title, tags, rows })
}

fn unquote_yaml(value: &str) -> Option<String> {
    if value.starts_with('[') && value.ends_with(']') {
        return Some(value.to_owned());
    }
    if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return Some(inner.replace("\\\"", "\""));
    }
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return Some(inner.replace("''", "'"));
    }
    if value.contains(['{', '}', '[', ']']) {
        return None;
    }
    Some(value.to_owned())
}

fn parse_yaml_tags(value: &str) -> Option<Vec<String>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    let value = value.strip_prefix('[')?.strip_suffix(']')?;
    value
        .split(',')
        .map(|tag| unquote_yaml(tag.trim()))
        .collect()
}

fn normalize_language(language: &str) -> String {
    match language.trim().to_ascii_lowercase().as_str() {
        "sh" | "shell" | "zsh" => "bash".to_owned(),
        "js" | "jsx" => "javascript".to_owned(),
        "ts" => "typescript".to_owned(),
        "py" => "python".to_owned(),
        "yml" => "yaml".to_owned(),
        "rs" => "rust".to_owned(),
        value => value.to_owned(),
    }
}

#[derive(Debug, Clone)]
struct ToolResultData {
    tool_call_id: Option<String>,
    tool_name: String,
    content: Value,
    details: Option<Value>,
    is_error: bool,
}

fn parse_tool_result(message: &Value, diagnostics: &mut Vec<RenderDiagnostic>) -> ToolResultData {
    let tool_call_id = message
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if tool_call_id.is_none() {
        diagnostics.push(RenderDiagnostic {
            entry_id: None,
            message: "toolResult 缺少 toolCallId，作为 orphan 显示".to_owned(),
        });
    }
    ToolResultData {
        tool_call_id,
        tool_name: message
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("unknown-tool")
            .to_owned(),
        content: message.get("content").cloned().unwrap_or(Value::Null),
        details: message.get("details").cloned(),
        is_error: message
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn render_tool_call(
    call: &Value,
    results: &HashMap<String, ToolResultData>,
    consumed_results: &mut HashSet<String>,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> ToolCard {
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("missing-tool-call-id")
        .to_owned();
    let name = call
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown-tool")
        .to_owned();
    let arguments = call.get("arguments").cloned().unwrap_or(Value::Null);
    let input_json = limit_text(&pretty_json(&arguments));
    let result = results.get(&id);
    let status = result.map_or(ToolStatus::Pending, |result| {
        if result.is_error {
            ToolStatus::Error
        } else if content_is_empty(&result.content)
            && result.details.as_ref().is_none_or(value_empty)
        {
            ToolStatus::Empty
        } else {
            ToolStatus::Success
        }
    });
    let mut output = Vec::new();
    let details = result.and_then(|result| result.details.clone());
    if let Some(result) = result {
        consumed_results.insert(id.clone());
        if result.tool_name != name {
            diagnostics.push(RenderDiagnostic {
                entry_id: Some(id.clone()),
                message: format!(
                    "toolCall name {name} 与 toolResult toolName {} 不一致",
                    result.tool_name
                ),
            });
        }
        if let Some(patch) = preferred_patch(result.details.as_ref()) {
            output.push(ToolOutput::Diff(parse_unified_diff(patch)));
        }
        append_tool_content(&result.content, &name, &mut output);
    }
    ToolCard {
        id,
        name,
        preview: preview_arguments(&arguments),
        arguments,
        input_json,
        status,
        output,
        details,
        orphan: false,
    }
}

fn orphan_result_message(
    base: &EntryBase,
    message: &Value,
    fallback: usize,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> Message {
    let result = parse_tool_result(message, diagnostics);
    let mut output = Vec::new();
    if let Some(patch) = preferred_patch(result.details.as_ref()) {
        output.push(ToolOutput::Diff(parse_unified_diff(patch)));
    }
    append_tool_content(&result.content, &result.tool_name, &mut output);
    let empty = output.is_empty() && result.details.as_ref().is_none_or(value_empty);
    Message {
        id: entry_id(base, fallback),
        role: MessageRole::Unknown,
        timestamp: base.timestamp.clone(),
        label: Some("未配对工具结果".to_owned()),
        blocks: vec![Block::Tool(ToolCard {
            id: result
                .tool_call_id
                .unwrap_or_else(|| "missing-tool-call-id".to_owned()),
            name: result.tool_name,
            arguments: Value::Null,
            input_json: "null".to_owned(),
            preview: "未找到对应 assistant toolCall".to_owned(),
            status: if result.is_error {
                ToolStatus::Error
            } else if empty {
                ToolStatus::Empty
            } else {
                ToolStatus::Success
            },
            output,
            details: result.details,
            orphan: true,
        })],
    }
}

fn render_bash_execution(value: &Value) -> ToolCard {
    let command = value
        .get("command")
        .or_else(|| value.get("arguments").and_then(|v| v.get("command")))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = value
        .get("output")
        .or_else(|| value.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let output = limit_text(output);
    let is_error = value
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value
            .get("exitCode")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0);
    ToolCard {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("bash-execution")
            .to_owned(),
        name: "bash".to_owned(),
        arguments: serde_json::json!({ "command": command }),
        input_json: limit_text(&pretty_json(&serde_json::json!({ "command": command }))),
        preview: truncate_chars(command, PREVIEW_CHARS),
        status: if is_error {
            ToolStatus::Error
        } else if output.is_empty() {
            ToolStatus::Empty
        } else {
            ToolStatus::Success
        },
        output: (!output.is_empty())
            .then(|| ToolOutput::Ansi(parse_ansi(&output)))
            .into_iter()
            .collect(),
        details: value.get("details").cloned(),
        orphan: false,
    }
}

fn append_tool_content(content: &Value, tool_name: &str, output: &mut Vec<ToolOutput>) {
    match content {
        Value::String(text) => push_tool_text(text, tool_name, output),
        Value::Array(items) => {
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            push_tool_text(text, tool_name, output);
                        }
                    }
                    Some("image") => output.push(ToolOutput::Image(parse_image(item))),
                    _ => output.push(ToolOutput::Text(visible_json(item))),
                }
            }
        }
        Value::Null => {}
        value => output.push(ToolOutput::Text(visible_json(value))),
    }
}

fn push_tool_text(text: &str, tool_name: &str, output: &mut Vec<ToolOutput>) {
    let text = limit_text(text);
    if tool_name.eq_ignore_ascii_case("bash") || text.contains('\u{1b}') {
        output.push(ToolOutput::Ansi(parse_ansi(&text)));
    } else {
        output.push(ToolOutput::Text(text));
    }
}

fn preferred_patch(details: Option<&Value>) -> Option<&str> {
    let details = details?;
    details
        .get("patch")
        .and_then(Value::as_str)
        .filter(|patch| !patch.trim().is_empty())
        .or_else(|| {
            details
                .get("diff")
                .and_then(Value::as_str)
                .filter(|diff| !diff.trim().is_empty())
        })
}

pub fn parse_unified_diff(raw: &str) -> DiffBlock {
    let raw = limit_text(raw);
    let mut files = Vec::new();
    let mut current_file: Option<DiffFile> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut valid = false;
    for line in raw.lines() {
        if let Some(path) = line.strip_prefix("--- ") {
            if let Some(mut file) = current_file.take() {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                files.push(file);
            }
            current_file = Some(DiffFile {
                old_path: Some(path.trim().to_owned()),
                new_path: None,
                hunks: Vec::new(),
            });
            valid = true;
        } else if let Some(path) = line.strip_prefix("+++ ") {
            current_file.get_or_insert(DiffFile {
                old_path: None,
                new_path: None,
                hunks: Vec::new(),
            });
            if let Some(file) = &mut current_file {
                file.new_path = Some(path.trim().to_owned());
            }
        } else if line.starts_with("@@") {
            if let Some(hunk) = current_hunk.take()
                && let Some(file) = &mut current_file
            {
                file.hunks.push(hunk);
            }
            current_hunk = Some(DiffHunk {
                header: line.to_owned(),
                lines: Vec::new(),
            });
            valid = true;
        } else if let Some(hunk) = &mut current_hunk {
            let kind = if line.starts_with('+') {
                DiffLineKind::Added
            } else if line.starts_with('-') {
                DiffLineKind::Removed
            } else if line.starts_with("\\ No newline") {
                DiffLineKind::Header
            } else {
                DiffLineKind::Context
            };
            hunk.lines.push(DiffLine {
                kind,
                text: line.to_owned(),
            });
        }
    }
    if let Some(mut file) = current_file {
        if let Some(hunk) = current_hunk {
            file.hunks.push(hunk);
        }
        files.push(file);
    }
    DiffBlock {
        parsed: valid && !files.is_empty(),
        files,
        raw,
    }
}

pub fn parse_ansi(input: &str) -> AnsiText {
    let bytes = input.as_bytes();
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut style = AnsiStyle::default();
    let mut run_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b {
            if index + 1 < bytes.len() && bytes[index + 1] == b'[' {
                let mut end = index + 2;
                while end < bytes.len() && !((0x40..=0x7e).contains(&bytes[end])) {
                    end += 1;
                }
                if end >= bytes.len() {
                    break;
                }
                if bytes[end] == b'm' {
                    if text.len() > run_start && style != AnsiStyle::default() {
                        spans.push(AnsiSpan {
                            range: run_start..text.len(),
                            style: style.clone(),
                        });
                    }
                    let params = std::str::from_utf8(&bytes[index + 2..end]).unwrap_or_default();
                    apply_sgr(params, &mut style);
                    run_start = text.len();
                }
                index = end + 1;
                continue;
            }
            // OSC 与未知 ESC 控制串都跳过，不把控制字节泄漏到纯文本。
            if index + 1 < bytes.len() && bytes[index + 1] == b']' {
                index += 2;
                while index < bytes.len() {
                    if bytes[index] == 0x07 {
                        index += 1;
                        break;
                    }
                    if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\'
                    {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
                continue;
            }
            // 未识别的 ESC 只跳过 introducer 本身；后续可能是多字节 UTF-8，
            // 不能按单字节终止符假设前进，否则会落到非法字符边界。
            index += 1;
            continue;
        }
        let ch = input[index..]
            .chars()
            .next()
            .expect("valid UTF-8 char boundary");
        text.push(ch);
        index += ch.len_utf8();
    }
    if text.len() > run_start && style != AnsiStyle::default() {
        spans.push(AnsiSpan {
            range: run_start..text.len(),
            style,
        });
    }
    AnsiText { text, spans }
}

fn apply_sgr(params: &str, style: &mut AnsiStyle) {
    let values = if params.is_empty() {
        vec![0]
    } else {
        params
            .split(';')
            .map(|value| value.parse::<u16>().unwrap_or(0))
            .collect()
    };
    let mut index = 0;
    while index < values.len() {
        match values[index] {
            0 => *style = AnsiStyle::default(),
            1 => style.bold = true,
            2 => style.dim = true,
            3 => style.italic = true,
            4 => style.underline = true,
            22 => {
                style.bold = false;
                style.dim = false;
            }
            23 => style.italic = false,
            24 => style.underline = false,
            30..=37 => style.foreground = Some(AnsiColor::Indexed((values[index] - 30) as u8)),
            39 => style.foreground = None,
            40..=47 => style.background = Some(AnsiColor::Indexed((values[index] - 40) as u8)),
            49 => style.background = None,
            90..=97 => style.foreground = Some(AnsiColor::Indexed((values[index] - 90 + 8) as u8)),
            100..=107 => {
                style.background = Some(AnsiColor::Indexed((values[index] - 100 + 8) as u8))
            }
            38 | 48 => {
                let foreground = values[index] == 38;
                if values.get(index + 1) == Some(&5) {
                    if let Some(value) = values.get(index + 2).copied().filter(|v| *v <= 255) {
                        if foreground {
                            style.foreground = Some(AnsiColor::Indexed(value as u8));
                        } else {
                            style.background = Some(AnsiColor::Indexed(value as u8));
                        }
                        index += 2;
                    }
                } else if values.get(index + 1) == Some(&2)
                    && let (Some(r), Some(g), Some(b)) = (
                        values.get(index + 2),
                        values.get(index + 3),
                        values.get(index + 4),
                    )
                    && *r <= 255
                    && *g <= 255
                    && *b <= 255
                {
                    let color = AnsiColor::Rgb(*r as u8, *g as u8, *b as u8);
                    if foreground {
                        style.foreground = Some(color);
                    } else {
                        style.background = Some(color);
                    }
                    index += 4;
                }
            }
            _ => {}
        }
        index += 1;
    }
}

fn parse_image(value: &Value) -> ImageBlock {
    let mut mime = value
        .get("mimeType")
        .or_else(|| value.get("mime_type"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut data = value.get("data").and_then(Value::as_str);
    let mut remote_url = None;
    if let Some(source) = value.get("source") {
        let source_type = source.get("type").and_then(Value::as_str);
        if source_type == Some("base64") {
            data = source.get("data").and_then(Value::as_str);
            mime = source
                .get("media_type")
                .or_else(|| source.get("mimeType"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or(mime);
        } else if source_type == Some("url") {
            remote_url = source.get("url").and_then(Value::as_str).map(str::to_owned);
        }
    }
    if remote_url.is_none() {
        remote_url = value.get("url").and_then(Value::as_str).map(str::to_owned);
    }
    if let Some(url) = remote_url {
        return ImageBlock {
            mime_type: mime,
            state: ImageState::Remote,
            bytes: None,
            remote_url: Some(url),
            description: "远程图片未自动联网加载".to_owned(),
        };
    }
    let Some(data) = data else {
        return image_placeholder(mime, ImageState::Invalid, "图片缺少 base64 数据");
    };
    if data.contains("<redacted>") || data.contains("[redacted]") {
        return image_placeholder(mime, ImageState::Redacted, "图片数据已脱敏");
    }
    if data.len() > MAX_IMAGE_BASE64_CHARS {
        return image_placeholder(mime, ImageState::TooLarge, "图片 base64 字符数超过限制");
    }
    let supported = mime.as_deref().is_some_and(|mime| {
        matches!(
            mime,
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        )
    });
    if !supported {
        return image_placeholder(mime, ImageState::Unsupported, "图片格式不受支持");
    }
    match base64::engine::general_purpose::STANDARD.decode(data) {
        Ok(bytes)
            if bytes.len() <= MAX_IMAGE_BYTES
                && mime
                    .as_deref()
                    .is_some_and(|mime| image_signature_matches(mime, &bytes)) =>
        {
            ImageBlock {
                mime_type: mime,
                state: ImageState::Inline,
                bytes: Some(bytes),
                remote_url: None,
                description: "内嵌图片".to_owned(),
            }
        }
        Ok(bytes) if bytes.len() <= MAX_IMAGE_BYTES => {
            image_placeholder(mime, ImageState::Invalid, "图片内容与声明格式不匹配")
        }
        Ok(_) => image_placeholder(mime, ImageState::TooLarge, "解码后图片超过限制"),
        Err(_) => image_placeholder(mime, ImageState::Invalid, "图片 base64 损坏"),
    }
}

fn image_signature_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn image_placeholder(mime: Option<String>, state: ImageState, description: &str) -> ImageBlock {
    ImageBlock {
        mime_type: mime,
        state,
        bytes: None,
        remote_url: None,
        description: description.to_owned(),
    }
}

fn build_minimap(messages: &[Message]) -> Vec<MinimapNode> {
    let mut nodes = Vec::new();
    let mut turn = 0;
    for message in messages {
        if message.role == MessageRole::User {
            turn += 1;
            if let Some(label) = first_visible_text(message) {
                nodes.push(MinimapNode {
                    message_id: message.id.clone(),
                    turn,
                    role: message.role,
                    label: truncate_chars(&label, 80),
                    level: None,
                });
            }
        } else if message.role == MessageRole::Assistant {
            let mut headings = Vec::new();
            for block in &message.blocks {
                if let Block::Markdown(markdown) = block {
                    for line in markdown.source.lines() {
                        let trimmed = line.trim_start();
                        let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
                        if (1..=3).contains(&hashes)
                            && trimmed.as_bytes().get(hashes) == Some(&b' ')
                        {
                            headings.push((hashes as u8, trimmed[hashes + 1..].trim().to_owned()));
                        }
                    }
                }
            }
            if headings.is_empty() {
                if let Some(label) = first_visible_text(message) {
                    nodes.push(MinimapNode {
                        message_id: message.id.clone(),
                        turn,
                        role: message.role,
                        label: truncate_chars(&label, 80),
                        level: None,
                    });
                }
            } else {
                for (level, label) in headings {
                    nodes.push(MinimapNode {
                        message_id: message.id.clone(),
                        turn,
                        role: message.role,
                        label: truncate_chars(&label, 80),
                        level: Some(level),
                    });
                }
            }
        }
    }
    nodes
}

fn first_visible_text(message: &Message) -> Option<String> {
    message.blocks.iter().find_map(|block| match block {
        Block::Markdown(markdown) => first_paragraph(&markdown.source),
        Block::Thinking(text) | Block::Unknown(UnknownBlock { text, .. }) => first_paragraph(text),
        Block::Notice(notice) => Some(notice.text.clone()),
        Block::Tool(tool) => Some(format!("{} · {}", tool.name, tool.preview)),
        _ => None,
    })
}

fn first_paragraph(text: &str) -> Option<String> {
    let paragraph = text
        .split("\n\n")
        .find(|part| !part.trim().is_empty())?
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    (!paragraph.is_empty()).then_some(paragraph)
}

fn snapshot_block(block: &Block, out: &mut String) {
    match block {
        Block::Markdown(value) => out.push_str(&format!("  markdown:{}\n", value.source.trim())),
        Block::Code(value) => out.push_str(&format!(
            "  code:{}{}:{}\n",
            value.language.as_deref().unwrap_or("text"),
            if value.mermaid_source { "(source)" } else { "" },
            value.source.trim()
        )),
        Block::Thinking(value) => out.push_str(&format!("  thinking:{}\n", value.trim())),
        Block::Tool(tool) => {
            out.push_str(&format!(
                "  tool:{}:{}:{:?}:{}\n",
                tool.name, tool.id, tool.status, tool.preview
            ));
            for output in &tool.output {
                match output {
                    ToolOutput::Text(text) => {
                        out.push_str(&format!("    output:{}\n", text.trim()))
                    }
                    ToolOutput::Ansi(ansi) => {
                        out.push_str(&format!("    ansi:{}\n", ansi.text.trim()))
                    }
                    ToolOutput::Image(image) => out.push_str(&format!(
                        "    image:{:?}:{}\n",
                        image.state, image.description
                    )),
                    ToolOutput::Diff(diff) => out.push_str(&format!(
                        "    diff:{}:{} files\n",
                        if diff.parsed { "parsed" } else { "raw" },
                        diff.files.len()
                    )),
                }
            }
        }
        Block::Diff(diff) => out.push_str(&format!("  diff:{:?}\n", diff.parsed)),
        Block::Ansi(ansi) => out.push_str(&format!("  ansi:{}\n", ansi.text.trim())),
        Block::Image(image) => out.push_str(&format!(
            "  image:{:?}:{}\n",
            image.state, image.description
        )),
        Block::Frontmatter(frontmatter) => out.push_str(&format!(
            "  frontmatter:title={:?},tags={:?},rows={:?}\n",
            frontmatter.title, frontmatter.tags, frontmatter.rows
        )),
        Block::Notice(notice) => out.push_str(&format!(
            "  notice:{}:{}\n",
            notice.title,
            notice.text.trim()
        )),
        Block::Unknown(unknown) => out.push_str(&format!(
            "  unknown:{}:{}\n",
            unknown.kind,
            unknown.text.trim()
        )),
    }
}

fn entry_id(base: &EntryBase, fallback: usize) -> String {
    base.id
        .clone()
        .unwrap_or_else(|| format!("linear-entry-{fallback}"))
}

fn preview_arguments(arguments: &Value) -> String {
    if let Some(object) = arguments.as_object() {
        for key in ["command", "path", "query", "pattern", "url"] {
            if let Some(value) = object.get(key).and_then(Value::as_str) {
                return truncate_chars(value, PREVIEW_CHARS);
            }
        }
    }
    truncate_chars(&pretty_json(arguments), PREVIEW_CHARS)
}

fn pretty_json(value: &Value) -> String {
    let canonical = canonical_json(value);
    serde_json::to_string_pretty(&canonical).unwrap_or_else(|_| visible_json(&canonical))
}

fn visible_json(value: &Value) -> String {
    truncate_chars(&canonical_json(value).to_string(), MAX_TEXT_CHARS)
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_json(value)))
                    .collect(),
            )
        }
        value => value.clone(),
    }
}

fn content_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

fn value_empty(value: &Value) -> bool {
    matches!(value, Value::Null)
        || value.as_array().is_some_and(Vec::is_empty)
        || value.as_object().is_some_and(serde_json::Map::is_empty)
        || value.as_str().is_some_and(str::is_empty)
}

fn limit_text(value: &str) -> String {
    limit_text_with_flag(value).0
}

fn limit_text_with_flag(value: &str) -> (String, bool) {
    if value.chars().count() <= MAX_TEXT_CHARS {
        (value.to_owned(), false)
    } else {
        (
            format!(
                "{}\n…[内容因超过静态渲染上限而截断]",
                truncate_chars(value, MAX_TEXT_CHARS)
            ),
            true,
        )
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn render_fixture(lines: &[Value]) -> ConversationDocument {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"session","version":3,"id":"s","timestamp":"2026-01-01T00:00:00Z","cwd":"C:/fixture"})
        )
        .unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        render_path(path).unwrap()
    }

    #[test]
    fn markdown_frontmatter_code_mermaid_and_minimap() {
        let document = render_fixture(&[
            serde_json::json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":"---\ntitle: Demo\ntags: [one, two]\nauthor: Pi\n---\nHello world"}}),
            serde_json::json!({"type":"message","id":"a","parentId":"u","message":{"role":"assistant","content":[{"type":"text","text":"# Result\nText\n```rust\nfn main() {}\n```\n```mermaid\ngraph TD; A-->B\n```"}]}}),
        ]);
        assert!(matches!(
            document.messages[0].blocks[0],
            Block::Frontmatter(_)
        ));
        assert!(document.messages[1].blocks.iter().any(|block| matches!(
            block,
            Block::Code(CodeBlock {
                mermaid_source: true,
                ..
            })
        )));
        assert!(
            document
                .minimap
                .iter()
                .any(|node| node.label == "Result" && node.level == Some(1))
        );
    }

    #[test]
    fn bad_frontmatter_is_preserved_as_markdown() {
        let document = render_fixture(&[
            serde_json::json!({"type":"message","message":{"role":"user","content":"---\nbad yaml\n---\nbody"}}),
        ]);
        assert!(
            matches!(&document.messages[0].blocks[0], Block::Markdown(markdown) if markdown.source.starts_with("---"))
        );
    }

    #[test]
    fn tool_pairing_diff_error_pending_empty_and_orphan() {
        let document = render_fixture(&[
            serde_json::json!({"type":"message","id":"a","parentId":null,"message":{"role":"assistant","content":[
                {"type":"toolCall","id":"ok","name":"edit","arguments":{"path":"a.rs"}},
                {"type":"toolCall","id":"pending","name":"read","arguments":{}},
                {"type":"toolCall","id":"empty","name":"read","arguments":{}}
            ]}}),
            serde_json::json!({"type":"message","id":"r1","parentId":"a","message":{"role":"toolResult","toolCallId":"ok","toolName":"edit","content":[{"type":"text","text":"done"}],"details":{"patch":"--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new"},"isError":true}}),
            serde_json::json!({"type":"message","id":"r2","parentId":"r1","message":{"role":"toolResult","toolCallId":"empty","toolName":"read","content":[],"details":{},"isError":false}}),
            serde_json::json!({"type":"message","id":"r3","parentId":"r2","message":{"role":"toolResult","toolCallId":"orphan","toolName":"future","content":"visible","isError":false}}),
        ]);
        let tools = document
            .messages
            .iter()
            .flat_map(|message| &message.blocks)
            .filter_map(|block| match block {
                Block::Tool(tool) => Some(tool),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(tools.iter().any(|tool| {
            tool.id == "ok"
                && tool.status == ToolStatus::Error
                && tool
                    .output
                    .iter()
                    .any(|output| matches!(output, ToolOutput::Diff(diff) if diff.parsed))
        }));
        assert!(
            tools
                .iter()
                .any(|tool| tool.id == "pending" && tool.status == ToolStatus::Pending)
        );
        assert!(
            tools
                .iter()
                .any(|tool| tool.id == "empty" && tool.status == ToolStatus::Empty)
        );
        assert!(tools.iter().any(|tool| tool.id == "orphan" && tool.orphan));
    }

    #[test]
    fn ansi_supports_styles_palette_and_truecolor_without_leaking_controls() {
        let parsed =
            parse_ansi("plain \u{1b}[1;31;48;5;200mred\u{1b}[0m \u{1b}[38;2;1;2;3mtrue\u{1b}[31");
        assert_eq!(parsed.text, "plain red true");
        assert_eq!(parsed.spans.len(), 2);
        assert!(parsed.spans[0].style.bold);
        assert_eq!(
            parsed.spans[0].style.foreground,
            Some(AnsiColor::Indexed(1))
        );
        assert_eq!(
            parsed.spans[0].style.background,
            Some(AnsiColor::Indexed(200))
        );
        assert_eq!(
            parsed.spans[1].style.foreground,
            Some(AnsiColor::Rgb(1, 2, 3))
        );

        let unknown = parse_ansi("before\u{1b}中after");
        assert_eq!(unknown.text, "before中after");
    }

    #[test]
    fn linear_sessions_assign_unique_fallback_message_ids() {
        let document = render_fixture(&[
            serde_json::json!({"type":"message","message":{"role":"user","content":"one"}}),
            serde_json::json!({"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"two"}]}}),
            serde_json::json!({"type":"message","message":{"role":"user","content":"three"}}),
        ]);
        let ids = document
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["linear-entry-0", "linear-entry-1", "linear-entry-2"]);
    }

    #[test]
    fn image_flat_source_url_limits_and_redaction_are_safe() {
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nfixture");
        let inline =
            parse_image(&serde_json::json!({"type":"image","data":png,"mimeType":"image/png"}));
        let source = parse_image(
            &serde_json::json!({"type":"image","source":{"type":"base64","data":"%%%","media_type":"image/png"}}),
        );
        let remote = parse_image(
            &serde_json::json!({"type":"image","source":{"type":"url","url":"https://example.invalid/image.png"}}),
        );
        let redacted = parse_image(
            &serde_json::json!({"type":"image","data":"<redacted>","mimeType":"image/png"}),
        );
        assert_eq!(inline.state, ImageState::Inline);
        assert_eq!(source.state, ImageState::Invalid);
        assert_eq!(remote.state, ImageState::Remote);
        assert_eq!(redacted.state, ImageState::Redacted);
    }

    #[test]
    fn bash_execution_and_blank_patches_obey_static_limits() {
        let oversized = "x".repeat(MAX_TEXT_CHARS + 32);
        let bash = render_bash_execution(&serde_json::json!({
            "type":"bashExecution",
            "command": oversized,
            "output": oversized,
            "exitCode":0
        }));
        assert!(bash.input_json.chars().count() <= MAX_TEXT_CHARS + 64);
        let ToolOutput::Ansi(output) = &bash.output[0] else {
            panic!("expected ANSI bash output");
        };
        assert!(output.text.chars().count() <= MAX_TEXT_CHARS + 64);
        assert!(preferred_patch(Some(&serde_json::json!({"patch":" \n\t"}))).is_none());
    }

    #[test]
    fn custom_compaction_branch_unknown_and_bash_are_visible() {
        let document = render_fixture(&[
            serde_json::json!({"type":"custom_message","id":"c","parentId":null,"customType":"notice","display":true,"content":"custom"}),
            serde_json::json!({"type":"compaction","id":"co","parentId":"c","summary":"compact","tokensBefore":42}),
            serde_json::json!({"type":"branch_summary","id":"br","parentId":"co","summary":"branch","fromId":"c"}),
            serde_json::json!({"type":"future_entry","id":"f","parentId":"br","value":"visible"}),
            serde_json::json!({"type":"message","id":"b","parentId":"f","message":{"role":"assistant","content":[{"type":"bashExecution","command":"echo ok","output":"\u{1b}[32mok\u{1b}[0m","exitCode":0}]}}),
        ]);
        let snapshot = document.text_snapshot();
        for expected in [
            "custom",
            "compact",
            "branch",
            "future_entry",
            "tool:bash",
            "ansi:ok",
        ] {
            assert!(
                snapshot.contains(expected),
                "missing {expected}:\n{snapshot}"
            );
        }
    }

    #[test]
    fn conversation_document_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ConversationDocument>();
    }

    #[test]
    fn remote_markdown_images_are_not_auto_loaded() {
        let blocks = split_markdown(
            "before ![diagram](https://example.invalid/a.png) after",
            &mut Vec::new(),
            None,
        );
        assert!(matches!(
            &blocks[0],
            Block::Markdown(markdown)
                if markdown.source.contains("远程图片未自动加载")
                    && !markdown.source.contains("![diagram]")
        ));
    }

    #[test]
    fn current_leaf_path_excludes_sibling_branch() {
        let document = render_fixture(&[
            serde_json::json!({"type":"message","id":"root","parentId":null,"message":{"role":"user","content":"root"}}),
            serde_json::json!({"type":"message","id":"sibling","parentId":"root","message":{"role":"assistant","content":"not current"}}),
            serde_json::json!({"type":"message","id":"leaf","parentId":"root","message":{"role":"assistant","content":"current"}}),
        ]);
        let snapshot = document.text_snapshot();
        assert!(snapshot.contains("current"));
        assert!(!snapshot.contains("not current"));
    }
}
