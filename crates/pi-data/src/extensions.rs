//! 用户级 extension 目录扫描。
//!
//! `.ts.disabled` 是现有桌面端约定：pi 只会自动加载 `.ts` / `index.ts`，因此
//! disabled 文件仍要显示，但同一 id 两种状态并存时以 enabled 为准。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub kind: ExtensionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ExtensionScan {
    pub extensions: Vec<ExtensionInfo>,
    pub diagnostics: Vec<ExtensionDiagnostic>,
}

pub fn scan_extensions(root: impl AsRef<Path>) -> ExtensionScan {
    let root = root.as_ref();
    if !root.exists() {
        return ExtensionScan::default();
    }

    let entries = match root.read_dir() {
        Ok(entries) => entries,
        Err(error) => {
            return ExtensionScan {
                extensions: Vec::new(),
                diagnostics: vec![ExtensionDiagnostic {
                    path: root.to_path_buf(),
                    message: error.to_string(),
                }],
            };
        }
    };
    let mut by_id = BTreeMap::<String, ExtensionInfo>::new();
    let mut diagnostics = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(ExtensionDiagnostic {
                    path: root.to_path_buf(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            diagnostics.push(ExtensionDiagnostic {
                path,
                message: "文件名不是 UTF-8，已跳过".to_owned(),
            });
            continue;
        };
        if is_backup_directory(name) {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                diagnostics.push(ExtensionDiagnostic {
                    path,
                    message: error.to_string(),
                });
                continue;
            }
        };
        let candidate = if file_type.is_file() {
            file_candidate(&path, name)
        } else if file_type.is_dir() {
            directory_candidate(&path, name)
        } else {
            None
        };
        if let Some(info) = candidate {
            by_id
                .entry(info.id.clone())
                .and_modify(|old| {
                    if info.enabled && !old.enabled {
                        *old = info.clone();
                    }
                })
                .or_insert(info);
        }
    }

    ExtensionScan {
        extensions: by_id.into_values().collect(),
        diagnostics,
    }
}

fn file_candidate(path: &Path, name: &str) -> Option<ExtensionInfo> {
    let (id, enabled) = if let Some(id) = name.strip_suffix(".ts.disabled") {
        (id, false)
    } else {
        (name.strip_suffix(".ts")?, true)
    };
    (!id.is_empty()).then(|| ExtensionInfo {
        id: id.to_owned(),
        name: id.to_owned(),
        path: path.to_path_buf(),
        enabled,
        kind: ExtensionKind::File,
    })
}

fn directory_candidate(path: &Path, name: &str) -> Option<ExtensionInfo> {
    let enabled_path = path.join("index.ts");
    let disabled_path = path.join("index.ts.disabled");
    let (entry_path, enabled) = if enabled_path.is_file() {
        (enabled_path, true)
    } else if disabled_path.is_file() {
        (disabled_path, false)
    } else {
        return None;
    };
    Some(ExtensionInfo {
        id: name.to_owned(),
        name: name.to_owned(),
        path: entry_path,
        enabled,
        kind: ExtensionKind::Directory,
    })
}

fn is_backup_directory(name: &str) -> bool {
    name == "node_modules"
        || name.starts_with('.')
        || name.ends_with(".bak")
        || name.contains("backup")
        || name.starts_with("_backup")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn scans_files_directories_and_prefers_enabled() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alpha.ts.disabled"), "// old").unwrap();
        fs::write(dir.path().join("alpha.ts"), "// live").unwrap();
        let package = dir.path().join("bravo");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("index.ts.disabled"), "// off").unwrap();
        let ignored = dir.path().join("node_modules");
        fs::create_dir(&ignored).unwrap();
        fs::write(ignored.join("index.ts"), "// ignored").unwrap();

        let scan = scan_extensions(dir.path());
        assert_eq!(scan.extensions.len(), 2);
        assert_eq!(scan.extensions[0].id, "alpha");
        assert!(scan.extensions[0].enabled);
        assert_eq!(scan.extensions[1].kind, ExtensionKind::Directory);
        assert!(!scan.extensions[1].enabled);
    }
}
