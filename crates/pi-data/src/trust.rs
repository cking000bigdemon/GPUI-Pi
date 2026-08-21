//! 项目资源检测与共享 trust store 的原子更新。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::{Map, Value};
use thiserror::Error;

use crate::config::{read_json, trust_path, write_json_atomic};
use crate::project_identity_key;

const PI_RESOURCES: &[&str] = &[
    "settings.json",
    "extensions",
    "skills",
    "prompts",
    "themes",
    "SYSTEM.md",
    "APPEND_SYSTEM.md",
];
const LOCK_ATTEMPTS: usize = 10;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);
const LOCK_STALE_AFTER: Duration = Duration::from_secs(10);
const READONLY_ATTEMPTS: usize = 3;
const READONLY_RETRY_DELAY: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTrustStatus {
    pub requires_trust: bool,
    pub trusted: bool,
    pub decision_path: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum TrustError {
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
    #[error("trust.json 必须是 JSON object")]
    InvalidStore,
    #[error("trust.json 中 {key:?} 的值必须是 bool 或 null")]
    InvalidDecision { key: String },
    #[error("无法规范化项目路径 {path}: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法获取 trust store 锁 {path}: {source}")]
    Lock {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn has_trust_resources(cwd: impl AsRef<Path>, home: Option<&Path>) -> bool {
    let cwd = cwd.as_ref();
    let pi_dir = cwd.join(".pi");
    if PI_RESOURCES.iter().any(|name| pi_dir.join(name).exists()) {
        return true;
    }
    let home = home.map(Path::to_path_buf).or_else(dirs::home_dir);
    let user_skills = home.map(|home| canonical_or_self(&home.join(".agents").join("skills")));
    let mut current = Some(canonical_or_self(cwd));
    while let Some(directory) = current {
        let skills = directory.join(".agents").join("skills");
        if skills.exists()
            && user_skills
                .as_ref()
                .is_none_or(|user| project_identity_key(&skills) != project_identity_key(user))
        {
            return true;
        }
        current = directory.parent().map(Path::to_path_buf);
    }
    false
}

/// 只读解析共享 trust store；不会创建锁、目录或写文件。
///
/// 上游写入 `trust.json` 不是原子替换，短暂的半写 JSON 会做有限重试。
pub fn read_project_trust_status(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    home: Option<&Path>,
) -> Result<ProjectTrustStatus, TrustError> {
    let cwd = cwd.as_ref();
    if !has_trust_resources(cwd, home) {
        return Ok(ProjectTrustStatus {
            requires_trust: false,
            trusted: true,
            decision_path: None,
        });
    }
    let path = trust_path(agent_dir);
    let object = read_store_readonly_with_retry(&path, READONLY_ATTEMPTS, |_| {
        thread::sleep(READONLY_RETRY_DELAY)
    })?;
    let mut current = canonical_project_path(cwd)?;
    loop {
        let key = trust_key(&current);
        if let Some(decision) = object.get(&key).and_then(Value::as_bool) {
            return Ok(ProjectTrustStatus {
                requires_trust: true,
                trusted: decision,
                decision_path: Some(current),
            });
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent.to_path_buf();
    }
    Ok(ProjectTrustStatus {
        requires_trust: true,
        trusted: false,
        decision_path: None,
    })
}

pub fn trust_project(agent_dir: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Result<(), TrustError> {
    let path = trust_path(agent_dir);
    let cwd = canonical_project_path(cwd.as_ref())?;
    with_trust_lock(&path, || {
        let mut object = read_store(&path)?;
        object.insert(trust_key(&cwd), Value::Bool(true));
        write_json_atomic(&path, &Value::Object(object))?;
        Ok(())
    })
}

fn read_store(path: &Path) -> Result<Map<String, Value>, TrustError> {
    let value = read_json(path)?;
    let object = value.as_object().ok_or(TrustError::InvalidStore)?;
    for (key, value) in object {
        if !matches!(value, Value::Bool(_) | Value::Null) {
            return Err(TrustError::InvalidDecision { key: key.clone() });
        }
    }
    Ok(object.clone())
}

fn read_store_readonly_with_retry(
    path: &Path,
    attempts: usize,
    mut wait: impl FnMut(usize),
) -> Result<Map<String, Value>, TrustError> {
    let attempts = attempts.max(1);
    for attempt in 0..attempts {
        match read_store(path) {
            Ok(store) => return Ok(store),
            Err(error) if attempt + 1 < attempts && readonly_error_is_retryable(&error) => {
                wait(attempt);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("只读 trust 重试循环总会返回")
}

fn readonly_error_is_retryable(error: &TrustError) -> bool {
    match error {
        TrustError::Config(crate::ConfigError::Parse { .. }) => true,
        TrustError::Config(crate::ConfigError::Read { source, .. }) => matches!(
            source.kind(),
            io::ErrorKind::Interrupted
                | io::ErrorKind::WouldBlock
                | io::ErrorKind::TimedOut
                | io::ErrorKind::UnexpectedEof
        ),
        _ => false,
    }
}

fn canonical_project_path(path: &Path) -> Result<PathBuf, TrustError> {
    dunce::canonicalize(path).map_err(|source| TrustError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

fn trust_key(path: &Path) -> String {
    path.as_os_str().to_string_lossy().into_owned()
}

fn canonical_or_self(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn with_trust_lock<T>(
    trust_file: &Path,
    operation: impl FnOnce() -> Result<T, TrustError>,
) -> Result<T, TrustError> {
    let _lock = TrustLock::acquire(trust_file)?;
    operation()
}

struct TrustLock {
    path: PathBuf,
}

impl TrustLock {
    fn acquire(trust_file: &Path) -> Result<Self, TrustError> {
        let parent = trust_file.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| TrustError::Lock {
            path: parent.to_path_buf(),
            source,
        })?;
        let lock_path = PathBuf::from(format!("{}.lock", trust_file.as_os_str().to_string_lossy()));
        for attempt in 0..LOCK_ATTEMPTS {
            match fs::create_dir(&lock_path) {
                Ok(()) => return Ok(Self { path: lock_path }),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&lock_path) {
                        match fs::remove_dir(&lock_path) {
                            Ok(()) => continue,
                            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                            Err(_) => {}
                        }
                    }
                    if attempt + 1 == LOCK_ATTEMPTS {
                        return Err(TrustError::Lock {
                            path: lock_path,
                            source,
                        });
                    }
                    thread::sleep(LOCK_RETRY_DELAY);
                }
                Err(source) => {
                    return Err(TrustError::Lock {
                        path: lock_path,
                        source,
                    });
                }
            }
        }
        unreachable!("锁重试循环总会返回")
    }
}

fn lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| modified.elapsed().map_err(io::Error::other))
        .is_ok_and(|elapsed| elapsed > LOCK_STALE_AFTER)
}

impl Drop for TrustLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn detects_only_trust_requiring_resources() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".pi")).unwrap();
        assert!(!has_trust_resources(dir.path(), None));
        fs::create_dir_all(dir.path().join(".agents/skills")).unwrap();
        assert!(has_trust_resources(dir.path(), None));
    }

    #[test]
    fn readonly_retry_is_bounded_and_reports_permanent_parse_error() {
        let agent = tempdir().unwrap();
        let path = agent.path().join("trust.json");
        fs::write(&path, "{").unwrap();
        let mut waits = Vec::new();
        let error =
            read_store_readonly_with_retry(&path, 3, |attempt| waits.push(attempt)).unwrap_err();
        assert!(matches!(
            error,
            TrustError::Config(crate::ConfigError::Parse { .. })
        ));
        assert_eq!(waits, [0, 1]);
    }

    #[test]
    fn readonly_retry_recovers_from_transient_parse_error_without_locking() {
        let agent = tempdir().unwrap();
        let path = agent.path().join("trust.json");
        fs::write(&path, "{").unwrap();
        let replacement = path.clone();
        let store = read_store_readonly_with_retry(&path, 2, move |_| {
            fs::write(&replacement, "{}").unwrap();
        })
        .unwrap();
        assert!(store.is_empty());
        assert!(!agent.path().join("trust.json.lock").exists());
    }

    #[test]
    fn readonly_retry_reports_permanent_io_error_without_waiting() {
        let agent = tempdir().unwrap();
        let path = agent.path().join("trust.json");
        fs::create_dir(&path).unwrap();
        let mut waits = 0;
        assert!(matches!(
            read_store_readonly_with_retry(&path, 2, |_| waits += 1),
            Err(TrustError::Config(crate::ConfigError::Read { .. }))
        ));
        assert_eq!(waits, 0);
    }

    #[test]
    fn readonly_retry_rejects_invalid_store_and_decision_without_waiting() {
        let agent = tempdir().unwrap();
        let path = agent.path().join("trust.json");
        for content in ["[]", r#"{"project":"yes"}"#] {
            fs::write(&path, content).unwrap();
            let mut waits = 0;
            let error = read_store_readonly_with_retry(&path, 3, |_| waits += 1).unwrap_err();
            assert!(matches!(
                error,
                TrustError::InvalidStore | TrustError::InvalidDecision { .. }
            ));
            assert_eq!(waits, 0);
        }
    }

    #[test]
    fn readonly_retry_classifies_only_transient_io_kinds() {
        let retryable = |kind| {
            readonly_error_is_retryable(&TrustError::Config(crate::ConfigError::Read {
                path: PathBuf::from("trust.json"),
                source: io::Error::from(kind),
            }))
        };
        assert!(retryable(io::ErrorKind::WouldBlock));
        assert!(retryable(io::ErrorKind::Interrupted));
        assert!(!retryable(io::ErrorKind::PermissionDenied));
        assert!(!retryable(io::ErrorKind::InvalidData));
    }

    #[test]
    fn fresh_proper_lock_directory_blocks_write() {
        let agent = tempdir().unwrap();
        let project = tempdir().unwrap();
        fs::create_dir(agent.path().join("trust.json.lock")).unwrap();
        assert!(matches!(
            trust_project(agent.path(), project.path()),
            Err(TrustError::Lock { .. })
        ));
        assert!(!agent.path().join("trust.json").exists());
    }
}
