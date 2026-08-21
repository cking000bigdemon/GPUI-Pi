//! Skills 与配置 package 的保守扫描。
//!
//! 本模块只读取用户目录与已信任项目目录。出于桌面客户端安全边界，扫描不跟随
//! skill 根或目录内 symlink；这与 pinned pi 会跟随 symlink 的行为有意不同，所有跳过
//! 都进入 diagnostics。Ignore 规则只匹配 canonical scan root 下的相对路径；无法
//! canonicalize 或脱离 root 的路径直接诊断并跳过，不回退到绝对路径匹配。Skill 启停
//! 只修改 `disable-model-invocation`，并在原子替换前
//! 复验允许根、真实路径和 revision。

use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde_json::Value;
use thiserror::Error;

use crate::{project_identity_key, read_project_trust_status};

const SKILL_FILE: &str = "SKILL.md";
const MAX_SKILL_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceScope {
    User,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRevision {
    pub len: u64,
    pub modified_nanos: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub scope: ResourceScope,
    pub disable_model_invocation: bool,
    pub revision: FileRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDiagnostic {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillScan {
    pub skills: Vec<SkillInfo>,
    pub diagnostics: Vec<ResourceDiagnostic>,
    pub project_resources_loaded: bool,
    pub trust_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFilters {
    pub autoload: Option<bool>,
    pub extensions: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
    pub prompts: Option<Vec<String>>,
    pub themes: Option<Vec<String>>,
}

impl PackageFilters {
    pub fn filtered(&self) -> bool {
        self.autoload.is_some()
            || self.extensions.is_some()
            || self.skills.is_some()
            || self.prompts.is_some()
            || self.themes.is_some()
    }

    pub fn disabled(&self) -> bool {
        self.autoload == Some(false)
            && [
                self.extensions.as_ref(),
                self.skills.as_ref(),
                self.prompts.as_ref(),
                self.themes.as_ref(),
            ]
            .into_iter()
            .all(|items| items.is_none_or(Vec::is_empty))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPackageInfo {
    pub source: String,
    pub scope: ResourceScope,
    pub filters: PackageFilters,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginScan {
    pub packages: Vec<PluginPackageInfo>,
    pub diagnostics: Vec<ResourceDiagnostic>,
    pub project_resources_loaded: bool,
    pub trust_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum SkillWriteError {
    #[error("skill 路径不在允许根内：{0}")]
    OutsideAllowedRoot(PathBuf),
    #[error("skill 路径不是普通文件：{0}")]
    NotRegularFile(PathBuf),
    #[error("skill 文件 revision 已变化：{0}")]
    RevisionConflict(PathBuf),
    #[error("skill frontmatter 缺少 closing ---：{0}")]
    MalformedFrontmatter(PathBuf),
    #[error("读取 skill 文件 {path} 失败：{source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("写入 skill 文件 {path} 失败：{source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn scan_skills(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    home: Option<&Path>,
) -> SkillScan {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let trust = read_project_trust_status(agent_dir, cwd, home);
    let project_resources_loaded = trust.as_ref().is_ok_and(|status| status.trusted);
    let trust_error = trust.err().map(|error| error.to_string());
    let roots = skill_scan_roots(agent_dir, cwd, home, project_resources_loaded);

    let mut by_name = BTreeMap::<String, SkillInfo>::new();
    let mut real_paths = HashSet::new();
    let mut diagnostics = Vec::new();
    let mut visited_roots = HashSet::new();
    for (root, scope) in roots {
        let root_key = project_identity_key(&root);
        if !visited_roots.insert(root_key) {
            continue;
        }
        let mut root_skills = Vec::new();
        scan_skill_root(&root, scope, &mut root_skills, &mut diagnostics);
        for skill in root_skills {
            let real_key = project_identity_key(&skill.path);
            if !real_paths.insert(real_key) {
                continue;
            }
            if let Some(winner) = by_name.get(&skill.name) {
                diagnostics.push(ResourceDiagnostic {
                    path: skill.path.clone(),
                    message: format!(
                        "skill name {:?} 冲突；先到者 {} 胜出，当前路径已忽略",
                        skill.name,
                        winner.path.display()
                    ),
                });
            } else {
                by_name.insert(skill.name.clone(), skill);
            }
        }
    }
    let mut skills = by_name.into_values().collect::<Vec<_>>();
    skills.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.path.cmp(&right.path))
    });
    SkillScan {
        skills,
        diagnostics,
        project_resources_loaded,
        trust_error,
    }
}

pub fn skill_allowed_roots(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    home: Option<&Path>,
) -> Result<Vec<PathBuf>, crate::TrustError> {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let project_resources_loaded = read_project_trust_status(agent_dir, cwd, home)?.trusted;
    Ok(
        skill_scan_roots(agent_dir, cwd, home, project_resources_loaded)
            .into_iter()
            .map(|(root, _)| root)
            .collect(),
    )
}

fn skill_scan_roots(
    agent_dir: &Path,
    cwd: &Path,
    home: Option<&Path>,
    project_resources_loaded: bool,
) -> Vec<(PathBuf, ResourceScope)> {
    // pinned pi 顺序：project .pi、cwd→git-root 的 project .agents、user .pi、user .agents。
    let mut roots = Vec::new();
    if project_resources_loaded {
        roots.push((cwd.join(".pi").join("skills"), ResourceScope::Project));
        for ancestor in project_skill_ancestors(cwd) {
            roots.push((
                ancestor.join(".agents").join("skills"),
                ResourceScope::Project,
            ));
        }
    }
    roots.push((agent_dir.join("skills"), ResourceScope::User));
    if let Some(home) = home {
        roots.push((home.join(".agents").join("skills"), ResourceScope::User));
    }
    roots
}

fn scan_skill_root(
    root: &Path,
    scope: ResourceScope,
    skills: &mut Vec<SkillInfo>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    if !root.exists() {
        return;
    }
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: root.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        diagnostics.push(ResourceDiagnostic {
            path: root.to_path_buf(),
            message: "skill 根目录是链接，已跳过".to_owned(),
        });
        return;
    }
    if root.is_file() {
        if root.extension().is_some_and(|extension| extension == "md") {
            add_skill(root, scope, skills, diagnostics);
        }
        return;
    }
    let canonical_root = match dunce::canonicalize(root) {
        Ok(path) => path,
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: root.to_path_buf(),
                message: format!("无法规范化 skill 根目录：{error}"),
            });
            return;
        }
    };
    let mut ignores = Vec::new();
    scan_skill_directory(
        &canonical_root,
        &canonical_root,
        true,
        scope,
        skills,
        diagnostics,
        &mut ignores,
    );
}

fn scan_skill_directory(
    directory: &Path,
    root: &Path,
    include_root_files: bool,
    scope: ResourceScope,
    skills: &mut Vec<SkillInfo>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
    ignores: &mut Vec<IgnoreRule>,
) {
    let canonical_directory = match dunce::canonicalize(directory) {
        Ok(path) => path,
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: directory.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    if !canonical_directory.starts_with(root) {
        diagnostics.push(ResourceDiagnostic {
            path: canonical_directory,
            message: "skill 目录不在规范化根目录内，已跳过".to_owned(),
        });
        return;
    }
    add_ignore_rules(&canonical_directory, root, ignores, diagnostics);
    let skill_file = canonical_directory.join(SKILL_FILE);
    if skill_file.is_file() {
        add_skill(&skill_file, scope, skills, diagnostics);
        return;
    }
    let entries = match fs::read_dir(&canonical_directory) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: canonical_directory,
                message: error.to_string(),
            });
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(ResourceDiagnostic {
                    path: canonical_directory.clone(),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                diagnostics.push(ResourceDiagnostic {
                    path,
                    message: error.to_string(),
                });
                continue;
            }
        };
        let relative = match path.strip_prefix(root) {
            Ok(relative) => relative,
            Err(error) => {
                diagnostics.push(ResourceDiagnostic {
                    path,
                    message: format!("无法计算相对 skill 路径：{error}"),
                });
                continue;
            }
        };
        if is_ignored(relative, metadata.is_dir(), ignores) {
            continue;
        }
        if metadata.file_type().is_symlink() {
            diagnostics.push(ResourceDiagnostic {
                path,
                message: "skill 扫描不跟随链接".to_owned(),
            });
        } else if metadata.is_dir() {
            scan_skill_directory(&path, root, false, scope, skills, diagnostics, ignores);
        } else if include_root_files
            && metadata.is_file()
            && path.extension().is_some_and(|extension| extension == "md")
        {
            add_skill(&path, scope, skills, diagnostics);
        }
    }
}

#[derive(Debug, Clone)]
struct IgnoreRule {
    base: PathBuf,
    pattern: String,
    negated: bool,
}

fn add_ignore_rules(
    directory: &Path,
    root: &Path,
    rules: &mut Vec<IgnoreRule>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    const IGNORE_FILES: [&str; 3] = [".gitignore", ".ignore", ".fdignore"];
    let base = match directory.strip_prefix(root) {
        Ok(base) => base.to_path_buf(),
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: directory.to_path_buf(),
                message: format!("无法计算 ignore 规则相对目录：{error}"),
            });
            return;
        }
    };
    for name in IGNORE_FILES {
        let Ok(content) = fs::read_to_string(directory.join(name)) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negated, pattern) = line
                .strip_prefix('!')
                .map_or((false, line), |pattern| (true, pattern));
            let pattern = pattern.trim_start_matches('/').replace('\\', "/");
            if !pattern.is_empty() {
                rules.push(IgnoreRule {
                    base: base.clone(),
                    pattern,
                    negated,
                });
            }
        }
    }
}

fn is_ignored(relative: &Path, is_directory: bool, rules: &[IgnoreRule]) -> bool {
    let relative = relative.to_string_lossy().replace('\\', "/");
    let candidate = if is_directory {
        format!("{relative}/")
    } else {
        relative
    };
    let mut ignored = false;
    for rule in rules {
        let base = rule.base.to_string_lossy().replace('\\', "/");
        let scoped = if base.is_empty() {
            candidate.as_str()
        } else if let Some(scoped) = candidate.strip_prefix(&format!("{base}/")) {
            scoped
        } else {
            continue;
        };
        if ignore_pattern_matches(&rule.pattern, scoped) {
            ignored = !rule.negated;
        }
    }
    ignored
}

fn ignore_pattern_matches(pattern: &str, candidate: &str) -> bool {
    let directory_only = pattern.ends_with('/');
    let pattern = pattern.trim_end_matches('/');
    if pattern.is_empty() {
        return false;
    }
    if !pattern.contains('/') {
        return candidate
            .split('/')
            .any(|segment| wildcard_match(pattern, segment));
    }
    wildcard_match(pattern, candidate)
        || (directory_only && candidate.starts_with(&format!("{pattern}/")))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star, mut star_value) = (None, 0);
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            star_value = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_value += 1;
            value_index = star_value;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn project_skill_ancestors(cwd: &Path) -> Vec<PathBuf> {
    let git_root = find_git_repository_root(cwd);
    let mut ancestors = Vec::new();
    for ancestor in cwd.ancestors() {
        ancestors.push(ancestor.to_path_buf());
        if git_root
            .as_ref()
            .is_some_and(|root| project_identity_key(root) == project_identity_key(ancestor))
        {
            break;
        }
    }
    ancestors
}

fn find_git_repository_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors().find_map(|ancestor| {
        let git = ancestor.join(".git");
        git.exists().then(|| ancestor.to_path_buf())
    })
}

fn add_skill(
    path: &Path,
    scope: ResourceScope,
    skills: &mut Vec<SkillInfo>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    match read_skill(path, scope) {
        Ok(skill) => skills.push(skill),
        Err(message) => diagnostics.push(ResourceDiagnostic {
            path: path.to_path_buf(),
            message,
        }),
    }
}

fn read_skill(path: &Path, scope: ResourceScope) -> Result<SkillInfo, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("不是普通文件".to_owned());
    }
    if metadata.len() > MAX_SKILL_BYTES {
        return Err("skill 文件超过 1 MiB 限制".to_owned());
    }
    let path = dunce::canonicalize(path).map_err(|error| error.to_string())?;
    let content = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let frontmatter = parse_frontmatter(&content)?;
    let description = frontmatter
        .get("description")
        .map(String::as_str)
        .filter(|description| !description.trim().is_empty())
        .ok_or_else(|| "缺少非空 description frontmatter".to_owned())?
        .to_owned();
    let name = frontmatter
        .get("name")
        .map(String::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "skill".to_owned());
    Ok(SkillInfo {
        name,
        description,
        path,
        scope,
        disable_model_invocation: frontmatter
            .get("disable-model-invocation")
            .is_some_and(|value| value.eq_ignore_ascii_case("true")),
        revision: revision_for_metadata(&metadata),
    })
}

fn parse_frontmatter(content: &str) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Ok(fields);
    }
    let mut closed = false;
    for line in lines {
        if line.trim_end_matches('\r') == "---" {
            closed = true;
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        fields.insert(
            key.trim().to_owned(),
            value
                .trim()
                .trim_matches(|character| character == '\'' || character == '"')
                .to_owned(),
        );
    }
    if !closed {
        return Err("skill frontmatter 缺少 closing ---".to_owned());
    }
    Ok(fields)
}

pub fn set_skill_disable_model_invocation(
    path: impl AsRef<Path>,
    allowed_roots: &[PathBuf],
    expected_revision: &FileRevision,
    disabled: bool,
) -> Result<FileRevision, SkillWriteError> {
    let path = path.as_ref().to_path_buf();
    let canonical_path = validate_skill_target(&path, allowed_roots)?;
    let metadata = fs::metadata(&canonical_path).map_err(|source| SkillWriteError::Read {
        path: canonical_path.clone(),
        source,
    })?;
    let actual_revision = revision_for_metadata(&metadata);
    if &actual_revision != expected_revision {
        return Err(SkillWriteError::RevisionConflict(canonical_path));
    }
    let content = fs::read_to_string(&canonical_path).map_err(|source| SkillWriteError::Read {
        path: canonical_path.clone(),
        source,
    })?;
    let updated = update_disable_model_invocation(&content, disabled)
        .ok_or_else(|| SkillWriteError::MalformedFrontmatter(canonical_path.clone()))?;
    if updated == content {
        return Ok(actual_revision);
    }
    let verify_path = canonical_path.clone();
    let verify_roots = allowed_roots.to_vec();
    let verify_revision = expected_revision.clone();
    crate::fs_util::write_atomic_with(
        &canonical_path,
        |file, temp_path| {
            use std::io::Write as _;
            file.write_all(updated.as_bytes())
                .map_err(|source| SkillWriteError::Write {
                    path: temp_path.to_path_buf(),
                    source,
                })
        },
        move || {
            let current = validate_skill_target(&verify_path, &verify_roots)?;
            if current != verify_path {
                return Err(SkillWriteError::OutsideAllowedRoot(current));
            }
            let metadata = fs::metadata(&verify_path).map_err(|source| SkillWriteError::Read {
                path: verify_path.clone(),
                source,
            })?;
            if revision_for_metadata(&metadata) != verify_revision {
                return Err(SkillWriteError::RevisionConflict(verify_path.clone()));
            }
            Ok(())
        },
        |_, io_path, source| SkillWriteError::Write {
            path: io_path.to_path_buf(),
            source,
        },
    )?;
    let metadata = fs::metadata(&canonical_path).map_err(|source| SkillWriteError::Read {
        path: canonical_path.clone(),
        source,
    })?;
    Ok(revision_for_metadata(&metadata))
}

fn validate_skill_target(
    path: &Path,
    allowed_roots: &[PathBuf],
) -> Result<PathBuf, SkillWriteError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| SkillWriteError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SkillWriteError::NotRegularFile(path.to_path_buf()));
    }
    let canonical_path = dunce::canonicalize(path).map_err(|source| SkillWriteError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let allowed = allowed_roots.iter().any(|root| {
        dunce::canonicalize(root)
            .ok()
            .is_some_and(|root| canonical_path.starts_with(root))
    });
    if !allowed {
        return Err(SkillWriteError::OutsideAllowedRoot(canonical_path));
    }
    Ok(canonical_path)
}

fn revision_for_metadata(metadata: &fs::Metadata) -> FileRevision {
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    FileRevision {
        len: metadata.len(),
        modified_nanos,
    }
}

fn update_disable_model_invocation(content: &str, disabled: bool) -> Option<String> {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines = content.split_inclusive(['\n']).collect::<Vec<_>>();
    let has_frontmatter = lines.first().is_some_and(|line| line.trim_end() == "---");
    if !has_frontmatter {
        return Some(if disabled {
            format!("---{newline}disable-model-invocation: true{newline}---{newline}{content}")
        } else {
            content.to_owned()
        });
    }
    let close_index = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (line.trim_end() == "---").then_some(index));
    let close_index = close_index?;
    let key_index = lines[1..close_index]
        .iter()
        .position(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "disable-model-invocation")
        })
        .map(|index| index + 1);
    let mut result = String::with_capacity(content.len() + 40);
    for (index, line) in lines.iter().enumerate() {
        if Some(index) == key_index {
            if disabled {
                result.push_str("disable-model-invocation: true");
                result.push_str(newline);
            }
            continue;
        }
        result.push_str(line);
        if disabled && key_index.is_none() && index == 0 {
            result.push_str("disable-model-invocation: true");
            result.push_str(newline);
        }
    }
    Some(result)
}

pub fn scan_plugin_packages(
    agent_dir: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    home: Option<&Path>,
) -> PluginScan {
    let agent_dir = agent_dir.as_ref();
    let cwd = cwd.as_ref();
    let trust = read_project_trust_status(agent_dir, cwd, home);
    let project_resources_loaded = trust.as_ref().is_ok_and(|status| status.trusted);
    let trust_error = trust.err().map(|error| error.to_string());
    let mut packages = Vec::new();
    let mut diagnostics = Vec::new();
    read_packages_from_settings(
        &agent_dir.join("settings.json"),
        ResourceScope::User,
        &mut packages,
        &mut diagnostics,
    );
    if project_resources_loaded {
        read_packages_from_settings(
            &cwd.join(".pi").join("settings.json"),
            ResourceScope::Project,
            &mut packages,
            &mut diagnostics,
        );
    }
    packages.sort_by(|left, right| {
        left.scope
            .cmp(&right.scope)
            .then_with(|| left.source.cmp(&right.source))
    });
    PluginScan {
        packages,
        diagnostics,
        project_resources_loaded,
        trust_error,
    }
}

fn read_packages_from_settings(
    path: &Path,
    scope: ResourceScope,
    packages: &mut Vec<PluginPackageInfo>,
    diagnostics: &mut Vec<ResourceDiagnostic>,
) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    let settings: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(ResourceDiagnostic {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
            return;
        }
    };
    let Some(entries) = settings.get("packages").and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        match parse_package(entry, scope) {
            Ok(package) => packages.push(package),
            Err(message) => diagnostics.push(ResourceDiagnostic {
                path: path.to_path_buf(),
                message,
            }),
        }
    }
}

fn parse_package(entry: &Value, scope: ResourceScope) -> Result<PluginPackageInfo, String> {
    if let Some(source) = entry.as_str() {
        return Ok(PluginPackageInfo {
            source: source.to_owned(),
            scope,
            filters: PackageFilters {
                autoload: None,
                extensions: None,
                skills: None,
                prompts: None,
                themes: None,
            },
        });
    }
    let object = entry
        .as_object()
        .ok_or_else(|| "package 配置必须是 string 或 object".to_owned())?;
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .filter(|source| !source.is_empty())
        .ok_or_else(|| "package object 缺少 source".to_owned())?;
    let string_array = |key: &str| -> Result<Option<Vec<String>>, String> {
        let Some(value) = object.get(key) else {
            return Ok(None);
        };
        let array = value
            .as_array()
            .ok_or_else(|| format!("package {key} 必须是 string array"))?;
        array
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("package {key} 必须是 string array"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    };
    Ok(PluginPackageInfo {
        source: source.to_owned(),
        scope,
        filters: PackageFilters {
            autoload: object.get("autoload").and_then(Value::as_bool),
            extensions: string_array("extensions")?,
            skills: string_array("skills")?,
            prompts: string_array("prompts")?,
            themes: string_array("themes")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn scans_user_project_and_untrusted_skills() {
        let agent = tempdir().unwrap();
        let project = tempdir().unwrap();
        let home = tempdir().unwrap();
        let user_skill = agent.path().join("skills/user/SKILL.md");
        fs::create_dir_all(user_skill.parent().unwrap()).unwrap();
        fs::write(
            &user_skill,
            "---\nname: user\ndescription: User skill\n---\nbody\n",
        )
        .unwrap();
        let project_skill = project.path().join(".pi/skills/project/SKILL.md");
        fs::create_dir_all(project_skill.parent().unwrap()).unwrap();
        fs::write(
            &project_skill,
            "---\nname: project\ndescription: Project skill\n---\nbody\n",
        )
        .unwrap();
        let untrusted = scan_skills(agent.path(), project.path(), Some(home.path()));
        assert_eq!(untrusted.skills.len(), 1);
        assert!(!untrusted.project_resources_loaded);

        crate::trust_project(agent.path(), project.path()).unwrap();
        let trusted = scan_skills(agent.path(), project.path(), Some(home.path()));
        assert_eq!(trusted.skills.len(), 2);
        assert!(trusted.project_resources_loaded);
    }

    #[test]
    fn skill_discovery_matches_pinned_pi_root_and_recursion_rules() {
        let root = tempdir().unwrap();
        fs::write(
            root.path().join("root.md"),
            "---\nname: root\ndescription: Root doc\n---\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("nested/skill/docs")).unwrap();
        fs::write(
            root.path().join("nested/skill/SKILL.md"),
            "---\nname: nested\ndescription: Nested skill\n---\n",
        )
        .unwrap();
        fs::write(
            root.path().join("nested/skill/docs/not-a-skill.md"),
            "---\nname: hidden\ndescription: Hidden doc\n---\n",
        )
        .unwrap();
        fs::write(
            root.path().join("nested/not-a-skill.md"),
            "---\nname: nested-doc\ndescription: Nested doc\n---\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join(".hidden/skill")).unwrap();
        fs::write(
            root.path().join(".hidden/skill/SKILL.md"),
            "---\nname: hidden-dir\ndescription: Hidden dir\n---\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("node_modules/package")).unwrap();
        fs::write(
            root.path().join("node_modules/package/SKILL.md"),
            "---\nname: dependency\ndescription: Dependency\n---\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("ignored/skill")).unwrap();
        fs::write(root.path().join(".ignore"), "ignored/\n").unwrap();
        fs::write(
            root.path().join("ignored/skill/SKILL.md"),
            "---\nname: ignored\ndescription: Ignored\n---\n",
        )
        .unwrap();

        let mut skills = Vec::new();
        let mut diagnostics = Vec::new();
        scan_skill_root(
            root.path(),
            ResourceScope::User,
            &mut skills,
            &mut diagnostics,
        );
        let names = skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"root"));
        assert!(names.contains(&"nested"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn duplicate_skill_name_keeps_first_pinned_root_and_reports_loser() {
        let agent = tempdir().unwrap();
        let project = tempdir().unwrap();
        let project_skill = project.path().join(".pi/skills/project/SKILL.md");
        let user_skill = agent.path().join("skills/user/SKILL.md");
        fs::create_dir_all(project_skill.parent().unwrap()).unwrap();
        fs::create_dir_all(user_skill.parent().unwrap()).unwrap();
        fs::write(
            &project_skill,
            "---\nname: duplicate\ndescription: Project winner\n---\n",
        )
        .unwrap();
        fs::write(
            &user_skill,
            "---\nname: duplicate\ndescription: User loser\n---\n",
        )
        .unwrap();
        crate::trust_project(agent.path(), project.path()).unwrap();
        let scan = scan_skills(agent.path(), project.path(), None);
        assert_eq!(scan.skills.len(), 1);
        assert_eq!(scan.skills[0].scope, ResourceScope::Project);
        assert_eq!(scan.skills[0].description, "Project winner");
        assert!(scan.diagnostics.iter().any(|diagnostic| {
            diagnostic.path == dunce::canonicalize(&user_skill).unwrap()
                && diagnostic.message.contains("冲突")
        }));
    }

    #[test]
    fn ignore_rules_use_canonical_root_relative_paths() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("nested/ignored/skill")).unwrap();
        fs::write(root.path().join("nested/.ignore"), "ignored/\n").unwrap();
        fs::write(
            root.path().join("nested/ignored/skill/SKILL.md"),
            "---\nname: ignored\ndescription: Ignored\n---\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("nested/kept/skill")).unwrap();
        fs::write(
            root.path().join("nested/kept/skill/SKILL.md"),
            "---\nname: kept\ndescription: Kept\n---\n",
        )
        .unwrap();
        let mut skills = Vec::new();
        let mut diagnostics = Vec::new();
        scan_skill_root(
            root.path(),
            ResourceScope::User,
            &mut skills,
            &mut diagnostics,
        );
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "kept");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignore_rule_base_rejects_directory_outside_root_with_diagnostic() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let mut rules = Vec::new();
        let mut diagnostics = Vec::new();
        add_ignore_rules(outside.path(), root.path(), &mut rules, &mut diagnostics);
        assert!(rules.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("相对目录"));
    }

    #[cfg(windows)]
    #[test]
    fn canonical_root_normalizes_windows_case_before_ignore_matching() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("ignored/skill")).unwrap();
        fs::write(root.path().join(".ignore"), "ignored/\n").unwrap();
        fs::write(
            root.path().join("ignored/skill/SKILL.md"),
            "---\nname: ignored\ndescription: Ignored\n---\n",
        )
        .unwrap();
        let differently_cased = PathBuf::from(root.path().display().to_string().to_uppercase());
        let mut skills = Vec::new();
        let mut diagnostics = Vec::new();
        scan_skill_root(
            &differently_cased,
            ResourceScope::User,
            &mut skills,
            &mut diagnostics,
        );
        assert!(skills.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn skill_allowed_roots_follow_readonly_trust() {
        let agent = tempdir().unwrap();
        let project = tempdir().unwrap();
        fs::create_dir_all(project.path().join(".pi/skills")).unwrap();
        let untrusted = skill_allowed_roots(agent.path(), project.path(), None).unwrap();
        assert!(!untrusted.contains(&project.path().join(".pi/skills")));
        crate::trust_project(agent.path(), project.path()).unwrap();
        let trusted = skill_allowed_roots(agent.path(), project.path(), None).unwrap();
        assert!(trusted.contains(&project.path().join(".pi/skills")));
    }

    #[test]
    fn project_agents_ancestors_stop_at_git_repository_root() {
        let root = tempdir().unwrap();
        let repository = root.path().join("repo");
        let cwd = repository.join("nested/project");
        fs::create_dir_all(repository.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let ancestors = project_skill_ancestors(&cwd);
        assert_eq!(ancestors.last(), Some(&repository));
        assert!(!ancestors.contains(&root.path().to_path_buf()));
    }

    #[test]
    fn toggles_frontmatter_surgically_and_rejects_revision_conflicts() {
        let root = tempdir().unwrap();
        let path = root.path().join("skill/SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "---\nname: fixture\nunknown: [keep, this]\ndescription: Fixture\n---\nbody\n",
        )
        .unwrap();
        let skill = read_skill(&path, ResourceScope::User).unwrap();
        let revision = set_skill_disable_model_invocation(
            &path,
            &[root.path().to_path_buf()],
            &skill.revision,
            true,
        )
        .unwrap();
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("unknown: [keep, this]"));
        assert!(updated.contains("disable-model-invocation: true"));
        fs::write(&path, format!("{updated}changed\n")).unwrap();
        assert!(matches!(
            set_skill_disable_model_invocation(
                &path,
                &[root.path().to_path_buf()],
                &revision,
                false
            ),
            Err(SkillWriteError::RevisionConflict(_))
        ));

        let malformed = root.path().join("malformed/SKILL.md");
        fs::create_dir_all(malformed.parent().unwrap()).unwrap();
        let malformed_content = "---\nname: malformed\ndescription: Missing close\nbody\n";
        fs::write(&malformed, malformed_content).unwrap();
        let metadata = fs::metadata(&malformed).unwrap();
        assert!(matches!(
            set_skill_disable_model_invocation(
                &malformed,
                &[root.path().to_path_buf()],
                &revision_for_metadata(&metadata),
                true
            ),
            Err(SkillWriteError::MalformedFrontmatter(_))
        ));
        assert_eq!(fs::read_to_string(&malformed).unwrap(), malformed_content);
    }

    #[test]
    fn parses_package_string_object_disabled_and_filters() {
        let agent = tempdir().unwrap();
        let project = tempdir().unwrap();
        fs::write(
            agent.path().join("settings.json"),
            r#"{"packages":["npm:plain",{"source":"git:filtered","autoload":false,"extensions":[],"skills":["+skill/**"],"prompts":[],"themes":[]}]}"#,
        )
        .unwrap();
        let scan = scan_plugin_packages(agent.path(), project.path(), None);
        assert_eq!(scan.packages.len(), 2);
        let plain = scan
            .packages
            .iter()
            .find(|package| package.source == "npm:plain")
            .unwrap();
        let filtered = scan
            .packages
            .iter()
            .find(|package| package.source == "git:filtered")
            .unwrap();
        assert!(!plain.filters.filtered());
        assert!(filtered.filters.filtered());
        assert!(!filtered.filters.disabled());
        let disabled = parse_package(
            &serde_json::json!({"source":"npm:disabled","autoload":false,"extensions":[],"skills":[],"prompts":[],"themes":[]}),
            ResourceScope::User,
        )
        .unwrap();
        assert!(disabled.filters.disabled());
    }
}
