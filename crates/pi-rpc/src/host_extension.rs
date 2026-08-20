//! GPUI-Pi 随包携带的项目命令环境扩展。

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const HOST_EXTENSION_SOURCE: &str = include_str!("../assets/project-command-environment.ts");
const HOST_EXTENSION_VERSION: &str = env!("CARGO_PKG_VERSION");
const HOST_EXTENSION_FILE: &str = "project-command-environment.ts";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 将内嵌扩展落到内容寻址的临时目录，供 `pi -e` 加载。
pub fn materialize_host_extension() -> io::Result<PathBuf> {
    materialize_host_extension_in(&std::env::temp_dir())
}

fn materialize_host_extension_in(temp_root: &Path) -> io::Result<PathBuf> {
    let digest = content_digest(HOST_EXTENSION_SOURCE.as_bytes());
    let directory = temp_root
        .join("gpui-pi")
        .join("host-extensions")
        .join(format!("v{HOST_EXTENSION_VERSION}-{digest:016x}"));
    fs::create_dir_all(&directory)?;
    let target = directory.join(HOST_EXTENSION_FILE);

    match fs::read(&target) {
        Ok(existing) if existing == HOST_EXTENSION_SOURCE.as_bytes() => return Ok(target),
        Ok(_) => {
            return existing_fallback(&directory)?
                .map_or_else(|| write_unique_fallback(&directory), Ok);
        }
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }

    match write_atomic(&directory, &target) {
        Ok(()) => Ok(target),
        Err(error) if target.is_file() => {
            let existing = fs::read(&target)?;
            if existing == HOST_EXTENSION_SOURCE.as_bytes() {
                Ok(target)
            } else {
                existing_fallback(&directory)?.map_or_else(|| write_unique_fallback(&directory), Ok).map_err(|fallback_error| {
                    io::Error::new(
                        fallback_error.kind(),
                        format!(
                            "host extension 并发落盘后内容不一致（原错误：{error}；fallback 错误：{fallback_error}）：{}",
                            target.display()
                        ),
                    )
                })
            }
        }
        Err(error) => Err(error),
    }
}

fn existing_fallback(directory: &Path) -> io::Result<Option<PathBuf>> {
    let mut candidates = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("project-command-environment.") && name.ends_with(".ts")
                })
        })
        .collect::<Vec<_>>();
    candidates.sort();
    for candidate in candidates {
        let Ok(content) = fs::read(&candidate) else {
            continue;
        };
        if content == HOST_EXTENSION_SOURCE.as_bytes() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn write_unique_fallback(directory: &Path) -> io::Result<PathBuf> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let target = directory.join(format!(
            "project-command-environment.{}.{}.ts",
            std::process::id(),
            sequence
        ));
        match write_atomic(directory, &target) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "无法分配唯一 host extension fallback 文件",
    ))
}

fn write_atomic(directory: &Path, target: &Path) -> io::Result<()> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "host extension 文件名不是 UTF-8",
            )
        })?;
    let temporary = directory.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(HOST_EXTENSION_SOURCE.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, target)?;
        if fs::read(target)? != HOST_EXTENSION_SOURCE.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("host extension 落盘校验失败：{}", target.display()),
            ));
        }
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn content_digest(content: &[u8]) -> u64 {
    // 固定 FNV-1a，避免 DefaultHasher 实现变化导致缓存路径无谓漂移。
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in content {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_embedded_source_intact_and_stably() {
        let temp = tempfile::tempdir().unwrap();
        let first = materialize_host_extension_in(temp.path()).unwrap();
        let second = materialize_host_extension_in(temp.path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(fs::read_to_string(first).unwrap(), HOST_EXTENSION_SOURCE);
    }

    #[test]
    fn corrupted_content_addressed_target_uses_a_verified_unique_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let target = materialize_host_extension_in(temp.path()).unwrap();
        fs::write(&target, "corrupt").unwrap();
        let fallback = materialize_host_extension_in(temp.path()).unwrap();
        let reused = materialize_host_extension_in(temp.path()).unwrap();
        assert_ne!(fallback, target);
        assert_eq!(fallback, reused);
        assert_eq!(fs::read_to_string(&target).unwrap(), "corrupt");
        assert_eq!(fs::read_to_string(fallback).unwrap(), HOST_EXTENSION_SOURCE);
    }

    #[test]
    fn fallback_scan_skips_an_unreadable_shape_and_reuses_a_valid_candidate() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("extensions");
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("project-command-environment.0.ts")).unwrap();
        let valid = directory.join("project-command-environment.1.ts");
        fs::write(&valid, HOST_EXTENSION_SOURCE).unwrap();
        assert_eq!(existing_fallback(&directory).unwrap(), Some(valid));
    }

    #[test]
    fn source_contains_required_host_environment_contract() {
        for needle in [
            "session_start",
            "resources_discover",
            "user_bash",
            "<builtin:bash>",
            "HOST_EXTENSION_PATH",
            "hostRegistered",
            "PORT",
            "NODE_ENV",
            "NEXT_",
            "getShellPath",
            "getShellCommandPrefix",
        ] {
            assert!(HOST_EXTENSION_SOURCE.contains(needle), "missing {needle}");
        }
    }
}
