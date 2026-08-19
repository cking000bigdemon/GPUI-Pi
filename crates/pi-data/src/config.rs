//! `~/.pi/agent` 下 JSON 配置的保真读写。
//!
//! 配置会被多个 pi 客户端共享，因此不在这里做会丢未知字段的强类型重写；调用方
//! 修改 `serde_json::Value` 后，通过同目录临时文件原子替换。

use std::fs;
use std::io;
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
    crate::fs_util::write_bytes_atomic_if(path, bytes, verify_before_replace)
}

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
