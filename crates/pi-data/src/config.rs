//! `~/.pi/agent` 下 JSON 配置的保真读写。
//!
//! 配置会被多个 pi 客户端共享，因此不在这里做会丢未知字段的强类型重写；调用方
//! 修改 `serde_json::Value` 后，通过同目录临时文件原子替换。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("读取 JSON 配置 {path} 失败: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("解析 JSON 配置 {path} 失败: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("序列化 JSON 配置 {path} 失败: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("写入 JSON 配置 {path} 失败: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn models_path(agent_dir: impl AsRef<Path>) -> PathBuf {
    agent_dir.as_ref().join("models.json")
}

pub fn settings_path(agent_dir: impl AsRef<Path>) -> PathBuf {
    agent_dir.as_ref().join("settings.json")
}

pub fn trust_path(agent_dir: impl AsRef<Path>) -> PathBuf {
    agent_dir.as_ref().join("trust.json")
}

/// 缺失配置按空对象处理，与 pi 首次启动前的状态一致。
pub fn read_json(path: impl AsRef<Path>) -> Result<Value, ConfigError> {
    let path = path.as_ref();
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Value::Object(Default::default()));
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_json::from_slice(&bytes).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

pub fn write_json_atomic(path: impl AsRef<Path>, value: &Value) -> Result<(), ConfigError> {
    let path = path.as_ref();
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| ConfigError::Serialize {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn read_models(agent_dir: impl AsRef<Path>) -> Result<Value, ConfigError> {
    read_json(models_path(agent_dir))
}

pub fn write_models(agent_dir: impl AsRef<Path>, value: &Value) -> Result<(), ConfigError> {
    write_json_atomic(models_path(agent_dir), value)
}

pub fn read_settings(agent_dir: impl AsRef<Path>) -> Result<Value, ConfigError> {
    read_json(settings_path(agent_dir))
}

pub fn write_settings(agent_dir: impl AsRef<Path>, value: &Value) -> Result<(), ConfigError> {
    write_json_atomic(settings_path(agent_dir), value)
}

pub fn read_trust(agent_dir: impl AsRef<Path>) -> Result<Value, ConfigError> {
    read_json(trust_path(agent_dir))
}

pub fn write_trust(agent_dir: impl AsRef<Path>, value: &Value) -> Result<(), ConfigError> {
    write_json_atomic(trust_path(agent_dir), value)
}

pub(crate) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomic_if(path, bytes, || Ok(()))
}

pub(crate) fn write_bytes_atomic_if<E>(
    path: &Path,
    bytes: &[u8],
    verify_before_replace: impl FnOnce() -> Result<(), E>,
) -> Result<(), E>
where
    E: From<io::Error>,
{
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(E::from)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.json");
    let mut last_collision = None;
    let mut verify_before_replace = Some(verify_before_replace);

    for _ in 0..100 {
        let temp_path = parent.join(format!(".{file_name}-{:016x}.tmp", next_temp_nonce()));
        let mut file = match open_private_temp(&temp_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                last_collision = Some(error);
                continue;
            }
            Err(error) => return Err(E::from(error)),
        };
        let result = (|| {
            file.write_all(bytes).map_err(E::from)?;
            file.flush().map_err(E::from)?;
            file.sync_all().map_err(E::from)?;
            drop(file);
            verify_before_replace
                .take()
                .expect("revision verifier is called once")()?;
            replace_file(&temp_path, path).map_err(E::from)?;
            sync_directory(parent);
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        return result;
    }
    Err(E::from(last_collision.unwrap_or_else(|| {
        io::Error::other("无法创建唯一临时文件")
    })))
}

fn open_private_temp(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn next_temp_nonce() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos() as u64);
    now ^ u64::from(std::process::id()) ^ COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: 两个 UTF-16 缓冲都以 NUL 结尾，并在调用期间保持有效。
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        // SAFETY: 紧跟失败的 Win32 调用读取线程局部错误码。
        let code = unsafe { GetLastError() };
        Err(io::Error::from_raw_os_error(code as i32))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(path: &Path) {
    if let Ok(directory) = File::open(path) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) {}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn missing_file_is_empty_object() {
        let dir = tempdir().unwrap();
        assert_eq!(
            read_json(dir.path().join("missing.json")).unwrap(),
            json!({})
        );
    }

    #[test]
    fn atomic_roundtrip_preserves_unknown_fields_and_cleans_temp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let value = json!({"future": {"nested": [1, true, null]}});
        write_json_atomic(&path, &value).unwrap();
        assert_eq!(read_json(&path).unwrap(), value);
        assert_eq!(dir.path().read_dir().unwrap().count(), 1);
    }

    #[test]
    fn failed_replace_keeps_existing_directory_and_cleans_temp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::create_dir(&path).unwrap();
        assert!(write_json_atomic(&path, &json!({"new": true})).is_err());
        assert!(path.is_dir());
        assert_eq!(dir.path().read_dir().unwrap().count(), 1);
    }
}
