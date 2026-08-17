//! 共享会话目录上的显式、安全文件动作。

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use thiserror::Error;

use crate::SessionSummary;
use crate::config::write_bytes_atomic_if;
use crate::session::{load_session_bytes, normalize_session_name, session_revision};

#[derive(Debug, Error)]
pub enum SessionActionError {
    #[error("会话正在运行，不能执行此操作")]
    Running,
    #[error("会话仍有子会话，不能删除")]
    HasChildren,
    #[error("会话路径不是普通 JSONL 文件: {0}")]
    InvalidSession(PathBuf),
    #[error("导出目标与源会话相同")]
    SameExportPath,
    #[error("会话已被其他 pi 进程修改，请刷新后重试")]
    ConcurrentModification,
    #[error("文件操作失败: {0}")]
    Io(#[from] io::Error),
    #[error("会话格式错误: {0}")]
    Session(#[from] crate::SessionError),
    #[error("序列化 session_info 失败: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn rename_session(
    session: &SessionSummary,
    name: &str,
    running: bool,
) -> Result<(), SessionActionError> {
    if running {
        return Err(SessionActionError::Running);
    }
    ensure_session_file(&session.path)?;
    let source = read_unchanged_source(session)?;
    let parsed = load_session_bytes(&session.path, &source)?;
    if parsed.header.id != session.id {
        return Err(SessionActionError::InvalidSession(session.path.clone()));
    }
    let existing_ids: HashSet<&str> = parsed.entries.iter().filter_map(entry_id).collect();
    let entry = json!({
        "type": "session_info",
        "id": generated_entry_id(&existing_ids),
        "parentId": parsed.entries.iter().rev().find_map(entry_id),
        "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "name": normalize_session_name(name),
    });
    let mut bytes = source;
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        bytes.push(b'\n');
    }
    serde_json::to_writer(&mut bytes, &entry)?;
    bytes.push(b'\n');
    write_bytes_atomic_if(&session.path, &bytes, || verify_revision(session))?;
    Ok(())
}

pub fn delete_leaf_session(
    session: &SessionSummary,
    all_sessions: &[SessionSummary],
    running: bool,
) -> Result<(), SessionActionError> {
    if running {
        return Err(SessionActionError::Running);
    }
    if all_sessions
        .iter()
        .any(|candidate| candidate.parent_session_id.as_deref() == Some(&session.id))
    {
        return Err(SessionActionError::HasChildren);
    }
    ensure_session_file(&session.path)?;
    verify_revision(session)?;
    let tombstone = tombstone_path(&session.path);
    // rename 前的 revision 校验把窗口缩到单个文件系统调用；跨进程无锁仍有极小 TOCTOU。
    fs::rename(&session.path, &tombstone)?;
    match fs::remove_file(&tombstone) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::rename(&tombstone, &session.path);
            Err(SessionActionError::Io(error))
        }
    }
}

pub fn export_session_jsonl(
    session: &SessionSummary,
    destination: impl AsRef<Path>,
) -> Result<(), SessionActionError> {
    ensure_session_file(&session.path)?;
    let destination = destination.as_ref();
    if same_path(&session.path, destination) {
        return Err(SessionActionError::SameExportPath);
    }
    let bytes = read_unchanged_source(session)?;
    write_bytes_atomic_if(destination, &bytes, || Ok::<(), SessionActionError>(()))?;
    Ok(())
}

fn read_unchanged_source(session: &SessionSummary) -> Result<Vec<u8>, SessionActionError> {
    verify_revision(session)?;
    let bytes = fs::read(&session.path)?;
    let current = session_revision(&session.path, &bytes)?;
    if current != session.revision {
        return Err(SessionActionError::ConcurrentModification);
    }
    Ok(bytes)
}

fn verify_revision(session: &SessionSummary) -> Result<(), SessionActionError> {
    let metadata = fs::metadata(&session.path)?;
    if metadata.len() != session.revision.len
        || metadata.modified().unwrap_or(UNIX_EPOCH) != session.revision.modified
    {
        return Err(SessionActionError::ConcurrentModification);
    }
    let bytes = fs::read(&session.path)?;
    let current = session_revision(&session.path, &bytes)?;
    if current != session.revision {
        return Err(SessionActionError::ConcurrentModification);
    }
    Ok(())
}

fn ensure_session_file(path: &Path) -> Result<(), SessionActionError> {
    if !path.is_file() || path.extension().is_none_or(|ext| ext != "jsonl") {
        return Err(SessionActionError::InvalidSession(path.to_path_buf()));
    }
    Ok(())
}

fn entry_id(entry: &crate::SessionEntry) -> Option<&str> {
    match entry {
        crate::SessionEntry::Message { base, .. }
        | crate::SessionEntry::ModelChange { base, .. }
        | crate::SessionEntry::ThinkingLevelChange { base, .. }
        | crate::SessionEntry::Compaction { base, .. }
        | crate::SessionEntry::BranchSummary { base, .. }
        | crate::SessionEntry::Custom { base, .. }
        | crate::SessionEntry::CustomMessage { base, .. }
        | crate::SessionEntry::Label { base, .. }
        | crate::SessionEntry::SessionInfo { base, .. }
        | crate::SessionEntry::Unknown { base, .. } => base.id.as_deref(),
    }
}

fn generated_entry_id(existing_ids: &HashSet<&str>) -> String {
    for attempt in 0..100_u64 {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos() as u64)
            ^ u64::from(std::process::id())
            ^ attempt;
        let candidate = format!("{:08x}", seed as u32);
        if !existing_ids.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("有限 entry id 集合不可能占满所有 8 位 hex")
}

fn tombstone_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.jsonl");
    for attempt in 0..100_u64 {
        let candidate = parent.join(format!(
            ".{file_name}.{:016x}.deleted",
            next_nonce() ^ attempt
        ));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!(".{file_name}.deleted"))
}

fn next_nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64)
        ^ u64::from(std::process::id())
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = dunce::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = dunce::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}
