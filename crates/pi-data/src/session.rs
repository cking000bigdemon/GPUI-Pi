//! pi 会话 JSONL 的只读解析。
//!
//! 上游会话是可追加且可扩展的。这里对稳定 envelope 强类型，对消息正文和未知
//! entry 保留原始 JSON，避免上游新增字段时让历史会话整体不可读。

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const NO_MESSAGES: &str = "(no messages)";

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("读取会话 {path} 失败: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("会话 {path} 没有有效的 session header")]
    MissingHeader { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDiagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    #[serde(default)]
    pub version: Option<u32>,
    pub id: String,
    pub timestamp: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub parent_session: Option<PathBuf>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryBase {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEntry {
    Message {
        base: EntryBase,
        message: Value,
    },
    ModelChange {
        base: EntryBase,
        provider: Option<String>,
        model_id: Option<String>,
        raw: Value,
    },
    ThinkingLevelChange {
        base: EntryBase,
        thinking_level: Option<String>,
        raw: Value,
    },
    Compaction {
        base: EntryBase,
        summary: Option<String>,
        tokens_before: Option<u64>,
        raw: Value,
    },
    BranchSummary {
        base: EntryBase,
        summary: Option<String>,
        from_id: Option<String>,
        raw: Value,
    },
    Custom {
        base: EntryBase,
        custom_type: Option<String>,
        raw: Value,
    },
    CustomMessage {
        base: EntryBase,
        custom_type: Option<String>,
        content: Value,
        display: Option<bool>,
        raw: Value,
    },
    Label {
        base: EntryBase,
        target_id: Option<String>,
        label: Option<String>,
        raw: Value,
    },
    SessionInfo {
        base: EntryBase,
        name: Option<String>,
        raw: Value,
    },
    Unknown {
        entry_type: String,
        base: EntryBase,
        raw: Value,
    },
}

impl SessionEntry {
    pub fn base(&self) -> &EntryBase {
        match self {
            Self::Message { base, .. }
            | Self::ModelChange { base, .. }
            | Self::ThinkingLevelChange { base, .. }
            | Self::Compaction { base, .. }
            | Self::BranchSummary { base, .. }
            | Self::Custom { base, .. }
            | Self::CustomMessage { base, .. }
            | Self::Label { base, .. }
            | Self::SessionInfo { base, .. }
            | Self::Unknown { base, .. } => base,
        }
    }

    pub fn entry_type(&self) -> &str {
        match self {
            Self::Message { .. } => "message",
            Self::ModelChange { .. } => "model_change",
            Self::ThinkingLevelChange { .. } => "thinking_level_change",
            Self::Compaction { .. } => "compaction",
            Self::BranchSummary { .. } => "branch_summary",
            Self::Custom { .. } => "custom",
            Self::CustomMessage { .. } => "custom_message",
            Self::Label { .. } => "label",
            Self::SessionInfo { .. } => "session_info",
            Self::Unknown { entry_type, .. } => entry_type,
        }
    }

    pub fn raw(&self) -> Option<&Value> {
        match self {
            Self::Message { .. } => None,
            Self::ModelChange { raw, .. }
            | Self::ThinkingLevelChange { raw, .. }
            | Self::Compaction { raw, .. }
            | Self::BranchSummary { raw, .. }
            | Self::Custom { raw, .. }
            | Self::CustomMessage { raw, .. }
            | Self::Label { raw, .. }
            | Self::SessionInfo { raw, .. }
            | Self::Unknown { raw, .. } => Some(raw),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionFile {
    pub path: PathBuf,
    pub header: SessionHeader,
    pub entries: Vec<SessionEntry>,
    pub diagnostics: Vec<SessionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBranchNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub children: Vec<String>,
    pub depth: usize,
    pub entry_type: String,
    pub preview: String,
    pub role: Option<String>,
    pub label: Option<String>,
    pub forkable_user_message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionBranchTree {
    /// 按稳定的深度优先顺序展开，UI 无需递归即可渲染深树。
    pub nodes: Vec<SessionBranchNode>,
    pub active_leaf_id: Option<String>,
    pub active_path: HashSet<String>,
    pub diagnostics: Vec<String>,
}

impl SessionFile {
    /// 构建只读分支投影。损坏 parentId、重复 id、自环与环都只产生诊断，绝不递归。
    pub fn branch_tree(&self) -> SessionBranchTree {
        self.branch_tree_at_leaf(None)
    }

    /// RPC `get_tree.leafId` 是活会话的权威 leaf；缺失或无效时才回退到 JSONL 末项。
    pub fn branch_tree_at_leaf(&self, authoritative_leaf_id: Option<&str>) -> SessionBranchTree {
        let mut diagnostics = Vec::new();
        let mut by_id = HashMap::<String, usize>::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(id) = entry.base().id.as_ref() else {
                continue;
            };
            if by_id.insert(id.clone(), index).is_some() {
                diagnostics.push(format!("重复 entry id {id}；使用最后一条"));
            }
        }
        let fallback_leaf_id = self
            .entries
            .iter()
            .rev()
            .find_map(|entry| entry.base().id.clone());
        let active_leaf_id = authoritative_leaf_id
            .filter(|leaf_id| by_id.contains_key(*leaf_id))
            .map(str::to_owned)
            .or_else(|| {
                if let Some(leaf_id) = authoritative_leaf_id {
                    diagnostics.push(format!(
                        "RPC 权威 leafId {leaf_id} 不在本地 JSONL；回退到末项"
                    ));
                }
                fallback_leaf_id
            });
        let active_path = branch_path_ids(
            &self.entries,
            &by_id,
            active_leaf_id.as_deref(),
            &mut diagnostics,
        );

        let mut children = HashMap::<String, Vec<String>>::new();
        let mut roots = Vec::new();
        for entry in &self.entries {
            let Some(id) = entry.base().id.as_ref() else {
                continue;
            };
            match entry.base().parent_id.as_ref() {
                Some(parent) if parent != id && by_id.contains_key(parent) => {
                    children.entry(parent.clone()).or_default().push(id.clone());
                }
                Some(parent) if parent == id => {
                    diagnostics.push(format!("entry {id} 自己指向自己；按 root 展示"));
                    roots.push(id.clone());
                }
                Some(parent) => {
                    diagnostics.push(format!("entry {id} 找不到 parentId {parent}；按 root 展示"));
                    roots.push(id.clone());
                }
                None => roots.push(id.clone()),
            }
        }

        let mut labels = HashMap::<String, String>::new();
        for entry in &self.entries {
            if let SessionEntry::Label {
                target_id: Some(target),
                label: Some(label),
                ..
            } = entry
            {
                labels.insert(target.clone(), normalize_preview(label, 80));
            }
        }

        let mut ordered = Vec::new();
        let mut visited = HashSet::new();
        let mut stack = roots
            .iter()
            .rev()
            .map(|id| (id.clone(), 0_usize))
            .collect::<Vec<_>>();
        // 没有 root 通常意味着环；仍须把所有条目作为安全的顶层候选展示。
        for id in by_id.keys() {
            if !roots.contains(id) {
                stack.push((id.clone(), 0));
            }
        }
        while let Some((id, depth)) = stack.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            let Some(index) = by_id.get(&id).copied() else {
                continue;
            };
            let entry = &self.entries[index];
            let child_ids = children.get(&id).cloned().unwrap_or_default();
            for child in child_ids.iter().rev() {
                stack.push((child.clone(), depth.saturating_add(1)));
            }
            let (preview, role, forkable_user_message) = entry_preview(entry);
            ordered.push(SessionBranchNode {
                id: id.clone(),
                parent_id: entry.base().parent_id.clone(),
                children: child_ids,
                depth,
                entry_type: entry.entry_type().to_owned(),
                preview,
                role,
                label: labels.get(&id).cloned(),
                forkable_user_message,
            });
        }
        if visited.len() != by_id.len() {
            diagnostics.push("部分环形条目无法从 root 到达；已作为顶层安全展示".to_owned());
        }
        SessionBranchTree {
            nodes: ordered,
            active_leaf_id,
            active_path,
            diagnostics,
        }
    }

    /// 返回目标 leaf 的只读路径副本；原 JSONL 与当前 leaf 均不修改。
    pub fn project_to_leaf(&self, leaf_id: &str) -> Option<Self> {
        let by_id = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.base().id.as_ref().map(|id| (id.clone(), index)))
            .collect::<HashMap<_, _>>();
        let mut diagnostics = Vec::new();
        let path = branch_path_indexes(&self.entries, &by_id, Some(leaf_id), &mut diagnostics)?;
        let entries = path
            .into_iter()
            .map(|index| self.entries[index].clone())
            .collect();
        let mut projected = self.clone();
        projected.entries = entries;
        projected.diagnostics.extend(
            diagnostics
                .into_iter()
                .map(|message| SessionDiagnostic { line: 0, message }),
        );
        Some(projected)
    }
}

fn branch_path_ids(
    entries: &[SessionEntry],
    by_id: &HashMap<String, usize>,
    leaf_id: Option<&str>,
    diagnostics: &mut Vec<String>,
) -> HashSet<String> {
    branch_path_indexes(entries, by_id, leaf_id, diagnostics)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|index| entries[index].base().id.clone())
        .collect()
}

fn branch_path_indexes(
    entries: &[SessionEntry],
    by_id: &HashMap<String, usize>,
    leaf_id: Option<&str>,
    diagnostics: &mut Vec<String>,
) -> Option<Vec<usize>> {
    let leaf_id = leaf_id?;
    let mut index = *by_id.get(leaf_id)?;
    let mut path = Vec::new();
    let mut visited = HashSet::new();
    loop {
        let entry = &entries[index];
        let id = entry.base().id.as_ref()?;
        if !visited.insert(id.clone()) {
            diagnostics.push(format!("parentId 在 {id} 形成循环；目标分支不可投影"));
            return None;
        }
        path.push(index);
        let Some(parent_id) = entry.base().parent_id.as_ref() else {
            break;
        };
        let Some(parent) = by_id.get(parent_id).copied() else {
            diagnostics.push(format!(
                "目标分支 {id} 找不到 parentId {parent_id}；保留可达路径"
            ));
            break;
        };
        index = parent;
    }
    path.reverse();
    Some(path)
}

fn entry_preview(entry: &SessionEntry) -> (String, Option<String>, Option<String>) {
    if let SessionEntry::Message { message, .. } = entry {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let text = message_text(message).unwrap_or_default();
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let forkable =
            (role.as_deref() == Some("user") && !normalized.is_empty()).then(|| normalized.clone());
        let fallback = role.as_deref().map_or("message", |role| role);
        return (
            if text.trim().is_empty() {
                format!("[{fallback}]")
            } else {
                normalize_preview(&text, 80)
            },
            role,
            forkable,
        );
    }
    (entry.entry_type().replace('_', " "), None, None)
}

fn normalize_preview(value: &str, limit: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}…")
    } else {
        preview
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionMetrics {
    /// 所有可识别 usage 的累计 token。坏字段被跳过，不影响其余会话。
    pub cumulative_tokens: u64,
    pub cumulative_cost: f64,
    /// 静态历史可确定的最近 assistant usage.totalTokens。
    pub recent_context_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRevision {
    pub len: u64,
    pub modified: SystemTime,
    pub fingerprint: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub path: PathBuf,
    pub revision: SessionRevision,
    pub id: String,
    pub cwd: PathBuf,
    pub name: Option<String>,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub message_count: usize,
    pub first_message: String,
    pub parent_session_path: Option<PathBuf>,
    pub parent_session_id: Option<String>,
    pub metrics: SessionMetrics,
}

#[derive(Debug, Default)]
pub struct SessionList {
    pub sessions: Vec<SessionSummary>,
    pub diagnostics: Vec<SessionListDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

pub fn load_session(path: impl AsRef<Path>) -> Result<SessionFile, SessionError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_session_reader(path, BufReader::new(file))
}

pub(crate) fn load_session_bytes(path: &Path, bytes: &[u8]) -> Result<SessionFile, SessionError> {
    parse_session_reader(path, io::Cursor::new(bytes))
}

pub fn read_session_summary(path: impl AsRef<Path>) -> Result<SessionSummary, SessionError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let revision = session_revision(path, &bytes).map_err(|source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let session = load_session_bytes(path, &bytes)?;
    Ok(summary_from_session(&session, revision))
}

pub fn list_sessions(root: impl AsRef<Path>) -> SessionList {
    let root = root.as_ref();
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    collect_jsonl(root, root, &mut files, &mut diagnostics);

    let mut summaries = Vec::new();
    for path in files {
        match read_session_summary_with_diagnostics(&path) {
            Ok((summary, session_diagnostics)) => {
                diagnostics.extend(session_diagnostics.into_iter().map(|diagnostic| {
                    SessionListDiagnostic {
                        path: path.clone(),
                        message: format!("第 {} 行：{}", diagnostic.line, diagnostic.message),
                    }
                }));
                summaries.push(summary);
            }
            Err(error) => diagnostics.push(SessionListDiagnostic {
                path,
                message: error.to_string(),
            }),
        }
    }
    let id_by_path: std::collections::HashMap<String, String> = summaries
        .iter()
        .map(|summary| (session_path_key(&summary.path), summary.id.clone()))
        .collect();
    for summary in &mut summaries {
        summary.parent_session_id = summary
            .parent_session_path
            .as_deref()
            .and_then(|path| id_by_path.get(&session_path_key(path)).cloned());
    }
    summaries.sort_by_key(|summary| std::cmp::Reverse(summary.modified));
    SessionList {
        sessions: summaries,
        diagnostics,
    }
}

fn read_session_summary_with_diagnostics(
    path: &Path,
) -> Result<(SessionSummary, Vec<SessionDiagnostic>), SessionError> {
    let bytes = std::fs::read(path).map_err(|source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let revision = session_revision(path, &bytes).map_err(|source| SessionError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let session = load_session_bytes(path, &bytes)?;
    let diagnostics = session.diagnostics.clone();
    Ok((summary_from_session(&session, revision), diagnostics))
}

fn parse_session_reader<R: BufRead>(path: &Path, reader: R) -> Result<SessionFile, SessionError> {
    let mut header = None;
    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|source| SessionError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let line = line.strip_suffix('\r').unwrap_or(&line);
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                diagnostics.push(SessionDiagnostic {
                    line: line_number,
                    message: error.to_string(),
                });
                continue;
            }
        };
        let entry_type = value.get("type").and_then(Value::as_str);
        if header.is_none() {
            if entry_type != Some("session") {
                diagnostics.push(SessionDiagnostic {
                    line: line_number,
                    message: "首个有效 JSON 不是 session header".to_owned(),
                });
                continue;
            }
            match serde_json::from_value(value) {
                Ok(parsed) => header = Some(parsed),
                Err(error) => diagnostics.push(SessionDiagnostic {
                    line: line_number,
                    message: error.to_string(),
                }),
            }
            continue;
        }
        entries.push(parse_entry(value));
    }

    let header = header.ok_or_else(|| SessionError::MissingHeader {
        path: path.to_path_buf(),
    })?;
    Ok(SessionFile {
        path: path.to_path_buf(),
        header,
        entries,
        diagnostics,
    })
}

fn parse_entry(value: Value) -> SessionEntry {
    let entry_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let base = parse_base(&value);
    match entry_type.as_str() {
        "message" => SessionEntry::Message {
            base,
            message: value.get("message").cloned().unwrap_or(Value::Null),
        },
        "model_change" => SessionEntry::ModelChange {
            base,
            provider: string_field(&value, "provider"),
            model_id: string_field(&value, "modelId"),
            raw: value,
        },
        "thinking_level_change" => SessionEntry::ThinkingLevelChange {
            base,
            thinking_level: string_field(&value, "thinkingLevel"),
            raw: value,
        },
        "compaction" => SessionEntry::Compaction {
            base,
            summary: string_field(&value, "summary"),
            tokens_before: value.get("tokensBefore").and_then(Value::as_u64),
            raw: value,
        },
        "branch_summary" => SessionEntry::BranchSummary {
            base,
            summary: string_field(&value, "summary"),
            from_id: string_field(&value, "fromId"),
            raw: value,
        },
        "custom" => SessionEntry::Custom {
            base,
            custom_type: string_field(&value, "customType"),
            raw: value,
        },
        "custom_message" => SessionEntry::CustomMessage {
            base,
            custom_type: string_field(&value, "customType"),
            content: value.get("content").cloned().unwrap_or(Value::Null),
            display: value.get("display").and_then(Value::as_bool),
            raw: value,
        },
        "label" => SessionEntry::Label {
            base,
            target_id: string_field(&value, "targetId"),
            label: string_field(&value, "label"),
            raw: value,
        },
        "session_info" => SessionEntry::SessionInfo {
            base,
            name: string_field(&value, "name"),
            raw: value,
        },
        _ => SessionEntry::Unknown {
            entry_type,
            base,
            raw: value,
        },
    }
}

fn parse_base(value: &Value) -> EntryBase {
    serde_json::from_value(value.clone()).unwrap_or(EntryBase {
        id: None,
        parent_id: None,
        timestamp: None,
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn summary_from_session(session: &SessionFile, revision: SessionRevision) -> SessionSummary {
    let mut name = None;
    let mut message_count = 0;
    let mut first_message = None;
    let mut last_activity = None;

    for entry in &session.entries {
        match entry {
            SessionEntry::SessionInfo {
                name: current_name, ..
            } => {
                name = current_name
                    .as_deref()
                    .map(normalize_session_name)
                    .filter(|name| !name.is_empty())
            }
            SessionEntry::Message { base, message } => {
                message_count += 1;
                let role = message.get("role").and_then(Value::as_str);
                if matches!(role, Some("user" | "assistant")) {
                    let activity = message
                        .get("timestamp")
                        .and_then(Value::as_u64)
                        .map(system_time_from_millis)
                        .or_else(|| base.timestamp.as_deref().and_then(parse_iso_time));
                    if let Some(activity) = activity {
                        last_activity = Some(
                            last_activity.map_or(activity, |old: SystemTime| old.max(activity)),
                        );
                    }
                }
                if first_message.is_none() && role == Some("user") {
                    first_message = message_text(message).and_then(|text| {
                        let text = text.trim();
                        (!text.is_empty()).then(|| text.to_owned())
                    });
                }
            }
            _ => {}
        }
    }

    let created = parse_iso_time(&session.header.timestamp).unwrap_or(revision.modified);
    SessionSummary {
        path: session.path.clone(),
        revision,
        id: session.header.id.clone(),
        cwd: PathBuf::from(&session.header.cwd),
        name,
        created,
        modified: last_activity.unwrap_or(created),
        message_count,
        first_message: first_message.unwrap_or_else(|| NO_MESSAGES.to_owned()),
        parent_session_path: session.header.parent_session.clone(),
        parent_session_id: None,
        metrics: session_metrics(session),
    }
}

pub fn session_metrics(session: &SessionFile) -> SessionMetrics {
    let mut metrics = SessionMetrics::default();
    for entry in &session.entries {
        let usage = match entry {
            SessionEntry::Message { message, .. } => {
                match message.get("role").and_then(Value::as_str) {
                    Some("assistant") => {
                        let usage = message.get("usage");
                        if let Some(tokens) = usage
                            .and_then(|value| value.get("totalTokens"))
                            .and_then(Value::as_u64)
                        {
                            metrics.recent_context_tokens = Some(tokens);
                        }
                        usage
                    }
                    Some("toolResult") => message.get("usage"),
                    _ => None,
                }
            }
            SessionEntry::Compaction { raw, .. } | SessionEntry::BranchSummary { raw, .. } => {
                raw.get("usage")
            }
            _ => None,
        };
        let Some(usage) = usage else { continue };
        if let Some(tokens) = usage_tokens(usage) {
            metrics.cumulative_tokens = metrics.cumulative_tokens.saturating_add(tokens);
        }
        if let Some(cost) = usage
            .get("cost")
            .and_then(|cost| cost.get("total"))
            .and_then(Value::as_f64)
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
        {
            metrics.cumulative_cost += cost;
        }
    }
    metrics
}

fn session_path_key(path: &Path) -> String {
    let text = path.as_os_str().to_string_lossy();
    if cfg!(windows) {
        text.replace('/', "\\").to_lowercase()
    } else {
        text.into_owned()
    }
}

fn message_text(message: &Value) -> Option<String> {
    match message.get("content")? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let parts: Vec<&str> = blocks
                .iter()
                .filter_map(|block| {
                    (block.get("type").and_then(Value::as_str) == Some("text"))
                        .then(|| block.get("text").and_then(Value::as_str))
                        .flatten()
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        _ => None,
    }
}

pub(crate) fn normalize_session_name(value: &str) -> String {
    value.replace(['\r', '\n'], " ").trim().to_owned()
}

fn usage_tokens(usage: &Value) -> Option<u64> {
    if let Some(total) = usage.get("totalTokens").and_then(Value::as_u64) {
        return Some(total);
    }
    let mut found = false;
    let total = ["input", "output", "cacheRead", "cacheWrite"]
        .into_iter()
        .filter_map(|key| usage.get(key).and_then(Value::as_u64))
        .fold(0_u64, |total, value| {
            found = true;
            total.saturating_add(value)
        });
    found.then_some(total)
}

pub(crate) fn session_revision(path: &Path, bytes: &[u8]) -> io::Result<SessionRevision> {
    let metadata = path.metadata()?;
    Ok(SessionRevision {
        len: metadata.len(),
        modified: metadata.modified().unwrap_or(UNIX_EPOCH),
        fingerprint: fingerprint(bytes),
    })
}

fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a 只用于并发修改检测，不用于安全边界；固定算法让 revision 可稳定比较。
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn parse_iso_time(value: &str) -> Option<SystemTime> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|date| {
            system_time_from_timestamp(date.timestamp(), date.timestamp_subsec_nanos())
        })
}

fn system_time_from_timestamp(seconds: i64, nanos: u32) -> Option<SystemTime> {
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::new(seconds as u64, nanos))
    } else {
        UNIX_EPOCH.checked_sub(Duration::new(seconds.unsigned_abs(), nanos))
    }
}

fn system_time_from_millis(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis)
}

fn collect_jsonl(
    scan_root: &Path,
    directory: &Path,
    files: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<SessionListDiagnostic>,
) {
    let entries = match directory.read_dir() {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(SessionListDiagnostic {
                path: directory.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(SessionListDiagnostic {
                    path: directory.to_path_buf(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                diagnostics.push(SessionListDiagnostic {
                    path,
                    message: error.to_string(),
                });
                continue;
            }
        };
        if file_type.is_dir() {
            collect_jsonl(scan_root, &path, files, diagnostics);
        } else if file_type.is_file()
            && path.extension().is_some_and(|ext| ext == "jsonl")
            && is_session_storage_path(scan_root, &path)
        {
            files.push(path);
        }
    }
}

fn is_session_storage_path(scan_root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(scan_root).unwrap_or(path);
    let components: Vec<_> = relative.components().collect();
    // pi 的默认目录是 `<project-dir>/<timestamp>_<uuid>.jsonl`。子代理会在该文件
    // 旁边再建同名目录，里面的 run-N/session.jsonl 不是顶层历史会话，不能重复列出。
    components.len() == 2 && path.file_name().is_some_and(|name| name != "session.jsonl")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn parse(text: &str) -> SessionFile {
        parse_session_reader(Path::new("fixture.jsonl"), Cursor::new(text)).unwrap()
    }

    #[test]
    fn parses_crlf_unknown_and_malformed_lines() {
        let session = parse(concat!(
            "{bad}\r\n",
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\r\n",
            "{\"type\":\"future\",\"id\":\"x\",\"parentId\":null}\r\n",
            "{bad again}\n"
        ));
        assert_eq!(session.header.id, "s");
        assert_eq!(session.entries[0].entry_type(), "future");
        assert_eq!(session.diagnostics.len(), 2);
    }

    #[test]
    fn v1_entries_may_omit_tree_fields() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"id\":\"old\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"message\",\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}"
        ));
        assert!(matches!(
            &session.entries[0],
            SessionEntry::Message {
                base: EntryBase { id: None, .. },
                ..
            }
        ));
    }

    #[test]
    fn summary_uses_latest_name_and_array_text() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"message\",\"id\":\"u\",\"parentId\":null,\"timestamp\":\"2026-01-01T00:00:01Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"image\"},{\"type\":\"text\",\"text\":\"hello\"}]}}\n",
            "{\"type\":\"session_info\",\"id\":\"n\",\"parentId\":\"u\",\"timestamp\":\"2026-01-01T00:00:02Z\",\"name\":\" title\\r\\nnext \"}\n"
        ));
        let summary = summary_from_session(
            &session,
            SessionRevision {
                len: 0,
                modified: UNIX_EPOCH,
                fingerprint: 0,
            },
        );
        assert_eq!(summary.first_message, "hello");
        assert_eq!(summary.name.as_deref(), Some("title  next"));
        assert_eq!(summary.message_count, 1);
    }

    #[test]
    fn metrics_accumulate_supported_usage_and_keep_latest_context() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"usage\":{\"totalTokens\":10,\"cost\":{\"total\":0.1}}}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"usage\":{\"input\":2,\"output\":3,\"cacheRead\":\"bad\",\"cost\":{\"total\":0.2}}}}\n",
            "{\"type\":\"compaction\",\"usage\":{\"totalTokens\":20,\"cost\":{\"total\":0.3}}}\n",
            "{\"type\":\"branch_summary\",\"usage\":{\"totalTokens\":30,\"cost\":{\"total\":0.4}}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"usage\":{\"totalTokens\":99,\"cost\":{\"total\":\"bad\"}}}}"
        ));
        let metrics = session_metrics(&session);
        assert_eq!(metrics.cumulative_tokens, 164);
        assert!((metrics.cumulative_cost - 1.0).abs() < f64::EPSILON);
        assert_eq!(metrics.recent_context_tokens, Some(99));
    }

    #[test]
    fn branch_tree_projects_active_path_labels_previews_and_forkable_users() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"message\",\"id\":\"root\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"  root   prompt  \"}}\n",
            "{\"type\":\"message\",\"id\":\"old\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"old branch\"}}\n",
            "{\"type\":\"message\",\"id\":\"new\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"new branch\"}}\n",
            "{\"type\":\"label\",\"id\":\"label\",\"parentId\":\"new\",\"targetId\":\"new\",\"label\":\"Chosen branch\"}\n"
        ));
        let tree = session.branch_tree();
        assert_eq!(tree.active_leaf_id.as_deref(), Some("label"));
        assert!(tree.active_path.contains("root"));
        assert!(tree.active_path.contains("new"));
        assert!(!tree.active_path.contains("old"));
        let root = tree.nodes.iter().find(|node| node.id == "root").unwrap();
        assert_eq!(root.children, ["old", "new"]);
        assert_eq!(root.preview, "root prompt");
        assert_eq!(root.forkable_user_message.as_deref(), Some("root prompt"));
        let selected = tree.nodes.iter().find(|node| node.id == "new").unwrap();
        assert_eq!(selected.label.as_deref(), Some("Chosen branch"));
    }

    #[test]
    fn rpc_leaf_overrides_append_last_active_path() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"message\",\"id\":\"root\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"root\"}}\n",
            "{\"type\":\"message\",\"id\":\"authoritative\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"active\"}}\n",
            "{\"type\":\"message\",\"id\":\"appended-sibling\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"later on disk\"}}\n"
        ));
        assert_eq!(
            session.branch_tree().active_leaf_id.as_deref(),
            Some("appended-sibling")
        );
        let tree = session.branch_tree_at_leaf(Some("authoritative"));
        assert_eq!(tree.active_leaf_id.as_deref(), Some("authoritative"));
        assert!(tree.active_path.contains("authoritative"));
        assert!(!tree.active_path.contains("appended-sibling"));
    }

    #[test]
    fn projection_is_read_only_and_rejects_cycles_without_recursing() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"message\",\"id\":\"root\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"root\"}}\n",
            "{\"type\":\"message\",\"id\":\"left\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"left\"}}\n",
            "{\"type\":\"message\",\"id\":\"right\",\"parentId\":\"root\",\"message\":{\"role\":\"assistant\",\"content\":\"right\"}}\n",
            "{\"type\":\"future\",\"id\":\"orphan\",\"parentId\":\"missing\"}\n",
            "{\"type\":\"future\",\"id\":\"cycle-a\",\"parentId\":\"cycle-b\"}\n",
            "{\"type\":\"future\",\"id\":\"cycle-b\",\"parentId\":\"cycle-a\"}\n"
        ));
        let projected = session.project_to_leaf("left").unwrap();
        assert_eq!(projected.entries.len(), 2);
        assert_eq!(projected.entries[1].base().id.as_deref(), Some("left"));
        assert_eq!(session.entries.len(), 6, "源会话不可修改");
        assert!(session.project_to_leaf("cycle-a").is_none());
        let tree = session.branch_tree();
        assert_eq!(tree.nodes.len(), 6);
        assert!(tree.diagnostics.iter().any(|line| line.contains("missing")));
    }

    #[test]
    fn deep_linear_tree_is_iterative() {
        let mut text = "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n".to_owned();
        for index in 0..10_000 {
            let parent = if index == 0 {
                "null".to_owned()
            } else {
                format!("\"n{}\"", index - 1)
            };
            text.push_str(&format!(
                "{{\"type\":\"future\",\"id\":\"n{index}\",\"parentId\":{parent}}}\n"
            ));
        }
        let session = parse(&text);
        let tree = session.branch_tree();
        assert_eq!(tree.nodes.len(), 10_000);
        assert_eq!(tree.active_path.len(), 10_000);
    }

    #[test]
    fn parses_every_documented_entry_kind() {
        let session = parse(concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"/tmp\"}\n",
            "{\"type\":\"model_change\",\"id\":\"1\",\"parentId\":null,\"provider\":\"p\",\"modelId\":\"m\"}\n",
            "{\"type\":\"thinking_level_change\",\"id\":\"2\",\"parentId\":\"1\",\"thinkingLevel\":\"high\"}\n",
            "{\"type\":\"compaction\",\"id\":\"3\",\"parentId\":\"2\",\"summary\":\"s\",\"tokensBefore\":1}\n",
            "{\"type\":\"branch_summary\",\"id\":\"4\",\"parentId\":\"3\",\"fromId\":\"2\",\"summary\":\"s\"}\n",
            "{\"type\":\"custom\",\"id\":\"5\",\"parentId\":\"4\",\"customType\":\"x\"}\n",
            "{\"type\":\"custom_message\",\"id\":\"6\",\"parentId\":\"5\",\"customType\":\"x\",\"content\":\"c\",\"display\":true}\n",
            "{\"type\":\"label\",\"id\":\"7\",\"parentId\":\"6\",\"targetId\":\"1\",\"label\":\"l\"}\n",
            "{\"type\":\"session_info\",\"id\":\"8\",\"parentId\":\"7\",\"name\":\"n\"}\n"
        ));
        assert_eq!(
            session
                .entries
                .iter()
                .map(SessionEntry::entry_type)
                .collect::<Vec<_>>(),
            [
                "model_change",
                "thinking_level_change",
                "compaction",
                "branch_summary",
                "custom",
                "custom_message",
                "label",
                "session_info",
            ]
        );
    }
}
