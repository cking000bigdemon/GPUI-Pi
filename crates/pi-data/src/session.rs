//! pi 会话 JSONL 的只读解析。
//!
//! 上游会话是可追加且可扩展的。这里对稳定 envelope 强类型，对消息正文和未知
//! entry 保留原始 JSON，避免上游新增字段时让历史会话整体不可读。

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
pub struct SessionSummary {
    pub path: PathBuf,
    pub id: String,
    pub cwd: PathBuf,
    pub name: Option<String>,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub message_count: usize,
    pub first_message: String,
    pub parent_session_path: Option<PathBuf>,
    pub parent_session_id: Option<String>,
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

pub fn read_session_summary(path: impl AsRef<Path>) -> Result<SessionSummary, SessionError> {
    let path = path.as_ref();
    let session = load_session(path)?;
    Ok(summary_from_session(&session, file_modified(path)))
}

pub fn list_sessions(root: impl AsRef<Path>) -> SessionList {
    let root = root.as_ref();
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    collect_jsonl(root, root, &mut files, &mut diagnostics);

    let mut summaries = Vec::new();
    for path in files {
        match read_session_summary(&path) {
            Ok(summary) => summaries.push(summary),
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

fn summary_from_session(session: &SessionFile, file_mtime: SystemTime) -> SessionSummary {
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
                    .and_then(non_empty)
                    .map(str::to_owned)
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

    let created = parse_iso_time(&session.header.timestamp).unwrap_or(file_mtime);
    SessionSummary {
        path: session.path.clone(),
        id: session.header.id.clone(),
        cwd: PathBuf::from(&session.header.cwd),
        name,
        created,
        modified: last_activity.unwrap_or(created),
        message_count,
        first_message: first_message.unwrap_or_else(|| NO_MESSAGES.to_owned()),
        parent_session_path: session.header.parent_session.clone(),
        parent_session_id: None,
    }
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

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
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

fn file_modified(path: &Path) -> SystemTime {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(UNIX_EPOCH)
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
            "{\"type\":\"session_info\",\"id\":\"n\",\"parentId\":\"u\",\"timestamp\":\"2026-01-01T00:00:02Z\",\"name\":\" title \"}\n"
        ));
        let summary = summary_from_session(&session, UNIX_EPOCH);
        assert_eq!(summary.first_message, "hello");
        assert_eq!(summary.name.as_deref(), Some("title"));
        assert_eq!(summary.message_count, 1);
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
