//! 模型与认证配置的保真、安全读写。
//!
//! `models.json` 与 `auth.json` 会被官方 pi 和其他客户端共享。这里仅修改调用方明确
//! 选择的字段，未知字段原样保留；认证视图永远不暴露凭据正文。

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::config::{ConfigError, write_bytes_atomic_if};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelApi {
    OpenAiCompletions,
    OpenAiResponses,
    AnthropicMessages,
    GoogleGenerativeAi,
}

impl ModelApi {
    pub const ALL: [Self; 4] = [
        Self::OpenAiCompletions,
        Self::OpenAiResponses,
        Self::AnthropicMessages,
        Self::GoogleGenerativeAi,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompletions => "openai-completions",
            Self::OpenAiResponses => "openai-responses",
            Self::AnthropicMessages => "anthropic-messages",
            Self::GoogleGenerativeAi => "google-generative-ai",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|api| api.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthCapability {
    ApiKey,
    Login,
    ApiKeyOrLogin,
}

impl AuthCapability {
    pub const fn accepts_api_key(self) -> bool {
        matches!(self, Self::ApiKey | Self::ApiKeyOrLogin)
    }

    pub const fn accepts_login(self) -> bool {
        matches!(self, Self::Login | Self::ApiKeyOrLogin)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: String,
    pub display_name: String,
    pub auth: AuthCapability,
    pub built_in: bool,
}

/// 常用内置 provider。配置文件和 `pi --list-models` 中出现的其他 provider 会动态合并。
pub fn built_in_providers() -> Vec<ProviderDescriptor> {
    [
        ("anthropic", "Anthropic", AuthCapability::ApiKeyOrLogin),
        ("openai", "OpenAI", AuthCapability::ApiKey),
        ("openai-codex", "OpenAI Codex", AuthCapability::Login),
        ("google", "Google", AuthCapability::ApiKey),
        (
            "google-gemini-cli",
            "Google Gemini CLI",
            AuthCapability::Login,
        ),
        (
            "google-antigravity",
            "Google Antigravity",
            AuthCapability::Login,
        ),
        ("github-copilot", "GitHub Copilot", AuthCapability::Login),
        ("openrouter", "OpenRouter", AuthCapability::ApiKeyOrLogin),
        ("xai", "xAI", AuthCapability::ApiKey),
        ("groq", "Groq", AuthCapability::ApiKey),
        ("mistral", "Mistral", AuthCapability::ApiKey),
        ("cerebras", "Cerebras", AuthCapability::ApiKey),
        ("radius", "Radius", AuthCapability::Login),
    ]
    .into_iter()
    .map(|(id, display_name, auth)| ProviderDescriptor {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        auth,
        built_in: true,
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelEntry {
    pub id: String,
    pub name: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderConfig {
    pub id: String,
    pub base_url: Option<String>,
    pub api: Option<ModelApi>,
    pub api_raw: Option<String>,
    pub models: Vec<ModelEntry>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAuthStatus {
    Ready { auth_type: Option<String> },
    NotReady { reason: Option<String> },
    Invalid { reason: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDraft {
    pub id: String,
    pub base_url: String,
    /// `None` 表示用户未修改 API 类型，保存时必须保留原始字段。
    pub api: Option<ModelApi>,
    pub model_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    ApiKey,
    OAuth,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSummary {
    pub provider_id: String,
    pub kind: AuthKind,
    pub configured: bool,
    pub has_key: bool,
    pub external_reference: bool,
    pub external_env: bool,
    pub masked: &'static str,
}

/// 密钥的不可打印容器。没有 `Debug` / `Display`，避免意外进入日志或通知。
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Result<Self, ModelConfigError> {
        validate_secret(&value)?;
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // Rust 的 String 没有稳定的安全清零 API；尽早清空至少避免继续被正常代码读取。
        self.0.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileRevision {
    hash: u64,
}

pub struct ModelConfigDocument {
    path: PathBuf,
    root: Value,
    revision: Option<FileRevision>,
    has_rewrite_trivia: bool,
}

#[derive(Debug, Error)]
pub enum ModelConfigError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("读取配置 {path} 失败: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("解析配置 {path} 失败: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("配置根节点必须是对象")]
    RootNotObject,
    #[error("providers 必须是对象")]
    ProvidersNotObject,
    #[error("provider `{0}` 必须是对象")]
    ProviderNotObject(String),
    #[error("provider id 只能包含字母、数字、点、下划线和连字符")]
    InvalidProviderId,
    #[error("base URL 必须是无凭据、无查询参数的 http(s) URL")]
    InvalidBaseUrl,
    #[error("至少需要一个非空模型 id")]
    MissingModels,
    #[error("模型 id 不能为空或重复")]
    InvalidModelId,
    #[error("配置已被其他进程修改，请刷新后重试")]
    ConcurrentModification,
    #[error("provider `{0}` 已保存非 API Key 凭据，不能覆盖")]
    CredentialWouldBeOverwritten(String),
    #[error("API Key 不能为空，且不能包含换行或 NUL")]
    InvalidSecret,
    #[error("新 provider 必须显式选择受支持的 API 类型")]
    MissingApi,
    #[error("API Key 使用命令引用；客户端不会执行命令，无法发现或测试模型")]
    CommandCredentialUnsupported,
    #[error("API Key 使用复杂环境变量模板；客户端仅支持单一 $NAME 或 ${{NAME}} 引用")]
    ComplexCredentialUnsupported,
    #[error("API Key 引用的环境变量 `{0}` 未设置")]
    MissingCredentialEnvironment(String),
    #[error("原子写入配置失败: {0}")]
    AtomicWrite(#[from] io::Error),
}

impl ModelConfigDocument {
    pub fn load(agent_dir: impl AsRef<Path>) -> Result<Self, ModelConfigError> {
        let path = crate::models_path(agent_dir);
        let (root, revision) = read_relaxed_json_with_revision(&path)?;
        validate_root(&root)?;
        let has_rewrite_trivia = source_has_rewrite_trivia(&path)?;
        Ok(Self {
            revision,
            path,
            root,
            has_rewrite_trivia,
        })
    }

    pub fn providers(&self) -> Result<Vec<ProviderConfig>, ModelConfigError> {
        let Some(providers) = self.root.get("providers") else {
            return Ok(Vec::new());
        };
        let providers = providers
            .as_object()
            .ok_or(ModelConfigError::ProvidersNotObject)?;
        providers
            .iter()
            .map(|(id, raw)| provider_view(id, raw))
            .collect()
    }

    pub fn upsert_provider(&mut self, draft: &ProviderDraft) -> Result<(), ModelConfigError> {
        validate_draft(draft)?;
        let root = self
            .root
            .as_object_mut()
            .ok_or(ModelConfigError::RootNotObject)?;
        let providers = root
            .entry("providers")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or(ModelConfigError::ProvidersNotObject)?;
        let provider = providers
            .entry(draft.id.clone())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| ModelConfigError::ProviderNotObject(draft.id.clone()))?;
        provider.insert("baseUrl".into(), Value::String(draft.base_url.clone()));
        if let Some(api) = draft.api {
            provider.insert("api".into(), Value::String(api.as_str().into()));
        } else if !provider.contains_key("api") {
            return Err(ModelConfigError::MissingApi);
        }

        let existing = provider
            .remove("models")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        let mut by_id = BTreeMap::new();
        let mut opaque_models = Vec::new();
        for model in existing {
            if let Some(id) = model.get("id").and_then(Value::as_str) {
                by_id.insert(id.to_owned(), model);
            } else {
                // 新版或扩展可能使用非字符串 id；无法识别时必须保留，不能因本客户端保存而丢失。
                opaque_models.push(model);
            }
        }
        let mut models = draft
            .model_ids
            .iter()
            .map(|id| {
                let mut model = by_id
                    .remove(id)
                    .and_then(|value| value.as_object().cloned())
                    .unwrap_or_default();
                model.insert("id".into(), Value::String(id.clone()));
                Value::Object(model)
            })
            .collect::<Vec<_>>();
        models.extend(opaque_models);
        provider.insert("models".into(), Value::Array(models));
        Ok(())
    }

    pub fn remove_provider(&mut self, provider_id: &str) -> Result<bool, ModelConfigError> {
        validate_provider_id(provider_id)?;
        let Some(providers) = self.root.get_mut("providers") else {
            return Ok(false);
        };
        let providers = providers
            .as_object_mut()
            .ok_or(ModelConfigError::ProvidersNotObject)?;
        Ok(providers.remove(provider_id).is_some())
    }

    pub fn save(&mut self) -> Result<(), ModelConfigError> {
        validate_root(&self.root)?;
        let bytes = serialize_json(&self.path, &self.root)?;
        write_bytes_atomic_if(&self.path, &bytes, || {
            verify_revision(&self.path, &self.revision)
        })?;
        self.revision = Some(revision_bytes(&bytes));
        Ok(())
    }

    pub fn value(&self) -> &Value {
        &self.root
    }

    pub fn has_rewrite_trivia(&self) -> bool {
        self.has_rewrite_trivia
    }
}

pub fn auth_path(agent_dir: impl AsRef<Path>) -> PathBuf {
    agent_dir.as_ref().join("auth.json")
}

pub fn read_auth_summaries(
    agent_dir: impl AsRef<Path>,
) -> Result<Vec<AuthSummary>, ModelConfigError> {
    let path = auth_path(agent_dir);
    let root = read_relaxed_json(&path)?;
    let object = root.as_object().ok_or(ModelConfigError::RootNotObject)?;
    Ok(object
        .iter()
        .map(|(provider_id, credential)| {
            let kind = match credential.get("type").and_then(Value::as_str) {
                Some("api_key") => AuthKind::ApiKey,
                Some("oauth") => AuthKind::OAuth,
                _ => AuthKind::Unknown,
            };
            let key = credential.get("key").and_then(Value::as_str);
            let has_key = key.is_some_and(|key| !key.is_empty());
            let external_reference =
                matches!(key, Some(key) if external_reference_kind(key).is_some());
            let external_env = credential
                .get("env")
                .and_then(Value::as_object)
                .is_some_and(|env| !env.is_empty());
            let configured = match kind {
                AuthKind::ApiKey => has_key || external_env,
                AuthKind::OAuth => credential
                    .get("access")
                    .and_then(Value::as_str)
                    .is_some_and(|token| !token.is_empty()),
                AuthKind::Unknown => false,
            };
            AuthSummary {
                provider_id: provider_id.clone(),
                kind,
                configured,
                has_key,
                external_reference,
                external_env,
                masked: if has_key && !external_reference {
                    "••••••••"
                } else {
                    ""
                },
            }
        })
        .collect())
}

pub fn read_api_key(
    agent_dir: impl AsRef<Path>,
    provider_id: &str,
) -> Result<Option<SecretString>, ModelConfigError> {
    validate_provider_id(provider_id)?;
    let root = read_relaxed_json(&auth_path(agent_dir))?;
    let Some(credential) = root.get(provider_id) else {
        return Ok(None);
    };
    if credential.get("type").and_then(Value::as_str) != Some("api_key") {
        return Ok(None);
    }
    let Some(key) = credential.get("key").and_then(Value::as_str) else {
        return Ok(None);
    };
    let env = credential.get("env").and_then(Value::as_object);
    resolve_stored_secret(key, env).map(Some)
}

pub fn write_api_key(
    agent_dir: impl AsRef<Path>,
    provider_id: &str,
    key: SecretString,
) -> Result<(), ModelConfigError> {
    validate_provider_id(provider_id)?;
    if key.is_empty() {
        return Err(ModelConfigError::InvalidSecret);
    }
    let path = auth_path(agent_dir);
    let (mut root, expected_revision) = read_relaxed_json_with_revision(&path)?;
    let object = root
        .as_object_mut()
        .ok_or(ModelConfigError::RootNotObject)?;
    let credential = object
        .entry(provider_id.to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| ModelConfigError::CredentialWouldBeOverwritten(provider_id.to_owned()))?;
    match credential.get("type").and_then(Value::as_str) {
        None | Some("api_key") => {}
        Some(_) => {
            return Err(ModelConfigError::CredentialWouldBeOverwritten(
                provider_id.to_owned(),
            ));
        }
    }
    credential.insert("type".into(), Value::String("api_key".into()));
    credential.insert(
        "key".into(),
        Value::String(encode_literal_secret(key.expose())),
    );
    let bytes = serialize_json(&path, &root)?;
    write_bytes_atomic_if(&path, &bytes, || verify_revision(&path, &expected_revision))?;
    Ok(())
}

/// 只移除 API Key；OAuth 或未知凭据保持不变。
pub fn remove_api_key(
    agent_dir: impl AsRef<Path>,
    provider_id: &str,
) -> Result<bool, ModelConfigError> {
    validate_provider_id(provider_id)?;
    let path = auth_path(agent_dir);
    let (mut root, expected_revision) = read_relaxed_json_with_revision(&path)?;
    let object = root
        .as_object_mut()
        .ok_or(ModelConfigError::RootNotObject)?;
    let remove_entry = {
        let Some(credential) = object.get_mut(provider_id) else {
            return Ok(false);
        };
        let Some(credential) = credential.as_object_mut() else {
            return Ok(false);
        };
        if credential.get("type").and_then(Value::as_str) != Some("api_key")
            || credential.remove("key").is_none()
        {
            return Ok(false);
        }
        credential.len() == 1 && credential.get("type").and_then(Value::as_str) == Some("api_key")
    };
    if remove_entry {
        object.remove(provider_id);
    }
    let bytes = serialize_json(&path, &root)?;
    write_bytes_atomic_if(&path, &bytes, || verify_revision(&path, &expected_revision))?;
    Ok(true)
}

pub fn merge_provider_directory(
    configured: &[ProviderConfig],
    model_providers: impl IntoIterator<Item = String>,
) -> Vec<ProviderDescriptor> {
    let mut providers = built_in_providers()
        .into_iter()
        .map(|provider| (provider.id.clone(), provider))
        .collect::<BTreeMap<_, _>>();
    for id in configured
        .iter()
        .map(|provider| provider.id.clone())
        .chain(model_providers)
    {
        providers.entry(id.clone()).or_insert(ProviderDescriptor {
            display_name: id.clone(),
            id,
            auth: AuthCapability::ApiKey,
            built_in: false,
        });
    }
    providers.into_values().collect()
}

pub fn parse_cli_auth_status(bytes: &[u8]) -> Result<CliAuthStatus, ModelConfigError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|error| ModelConfigError::Parse {
        path: PathBuf::from("<pi-auth-check>"),
        message: error.to_string(),
    })?;
    let status =
        root.get("status")
            .and_then(Value::as_str)
            .ok_or_else(|| ModelConfigError::Parse {
                path: PathBuf::from("<pi-auth-check>"),
                message: "缺少 status".into(),
            })?;
    let auth_type = root
        .get("authType")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let reason = root
        .get("reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    match status {
        "ready" => Ok(CliAuthStatus::Ready { auth_type }),
        "not_ready" => Ok(CliAuthStatus::NotReady { reason }),
        "invalid" => Ok(CliAuthStatus::Invalid { reason }),
        _ => Err(ModelConfigError::Parse {
            path: PathBuf::from("<pi-auth-check>"),
            message: "未知 status".into(),
        }),
    }
}

pub fn connectivity_request_body(api: ModelApi, model: &str) -> Vec<u8> {
    let body = match api {
        ModelApi::OpenAiCompletions => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 1,
            "stream": false
        }),
        ModelApi::OpenAiResponses => {
            json!({"model": model, "input": "Reply with OK.", "max_output_tokens": 1})
        }
        ModelApi::AnthropicMessages => json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with OK."}],
            "max_tokens": 1
        }),
        ModelApi::GoogleGenerativeAi => json!({
            "contents": [{"parts": [{"text": "Reply with OK."}]}],
            "generationConfig": {"maxOutputTokens": 1}
        }),
    };
    serde_json::to_vec(&body).expect("静态连通性请求必须可序列化")
}

pub fn parse_discovered_models(
    api: ModelApi,
    bytes: &[u8],
) -> Result<Vec<String>, ModelConfigError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|error| ModelConfigError::Parse {
        path: PathBuf::from("<provider-response>"),
        message: error.to_string(),
    })?;
    let entries = match api {
        ModelApi::GoogleGenerativeAi => root.get("models"),
        _ => root.get("data"),
    }
    .and_then(Value::as_array)
    .ok_or_else(|| ModelConfigError::Parse {
        path: PathBuf::from("<provider-response>"),
        message: "缺少模型数组".into(),
    })?;
    let mut ids = BTreeSet::new();
    for entry in entries {
        let Some(id) = entry
            .get(if api == ModelApi::GoogleGenerativeAi {
                "name"
            } else {
                "id"
            })
            .and_then(Value::as_str)
        else {
            continue;
        };
        let id = id.strip_prefix("models/").unwrap_or(id).trim();
        if !id.is_empty() {
            ids.insert(id.to_owned());
        }
    }
    Ok(ids.into_iter().collect())
}

pub fn validate_base_url(value: &str) -> Result<(), ModelConfigError> {
    let value = value.trim();
    let scheme_ok = value.starts_with("http://") || value.starts_with("https://");
    let remainder = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or_default();
    if !scheme_ok
        || remainder.is_empty()
        || remainder.starts_with('/')
        || value.chars().any(char::is_whitespace)
        || remainder.contains('@')
        || value.contains('?')
        || value.contains('#')
    {
        return Err(ModelConfigError::InvalidBaseUrl);
    }
    Ok(())
}

fn read_relaxed_json(path: &Path) -> Result<Value, ModelConfigError> {
    read_relaxed_json_with_revision(path).map(|(value, _)| value)
}

fn read_relaxed_json_with_revision(
    path: &Path,
) -> Result<(Value, Option<FileRevision>), ModelConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((json!({}), None)),
        Err(source) => {
            return Err(ModelConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let revision = Some(revision_bytes(&bytes));
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok((json!({}), revision));
    }
    let relaxed = strip_jsonc(bytes).map_err(|message| ModelConfigError::Parse {
        path: path.to_path_buf(),
        message,
    })?;
    let value = serde_json::from_slice(&relaxed).map_err(|error| ModelConfigError::Parse {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok((value, revision))
}

fn source_has_rewrite_trivia(path: &Path) -> Result<bool, ModelConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(ModelConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    Ok(detect_rewrite_trivia(&bytes))
}

fn detect_rewrite_trivia(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
        } else if byte == b'/' && matches!(bytes.get(index + 1), Some(b'/' | b'*')) {
            return true;
        } else if byte == b',' {
            let mut next = index + 1;
            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if matches!(bytes.get(next), Some(b'}' | b']')) {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn serialize_json(path: &Path, value: &Value) -> Result<Vec<u8>, ModelConfigError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| {
        ModelConfigError::Config(ConfigError::Serialize {
            path: path.to_path_buf(),
            source,
        })
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn verify_revision(path: &Path, expected: &Option<FileRevision>) -> Result<(), ModelConfigError> {
    if &revision(path)? == expected {
        Ok(())
    } else {
        Err(ModelConfigError::ConcurrentModification)
    }
}

fn revision(path: &Path) -> Result<Option<FileRevision>, ModelConfigError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(revision_bytes(&bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ModelConfigError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn revision_bytes(bytes: &[u8]) -> FileRevision {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    FileRevision {
        hash: hasher.finish(),
    }
}

/// 上游允许 JSONC 注释与尾逗号。编辑模型仍落为标准 JSON，但所有数据字段保留。
fn strip_jsonc(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            output.push(byte);
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && !matches!(bytes[index], b'\r' | b'\n') {
                index += 1;
            }
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            if index + 1 >= bytes.len() {
                return Err("块注释未闭合".into());
            }
            index += 2;
            continue;
        }
        if byte == b',' {
            let mut lookahead = index + 1;
            loop {
                while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                    lookahead += 1;
                }
                if bytes.get(lookahead) == Some(&b'/') && bytes.get(lookahead + 1) == Some(&b'/') {
                    lookahead += 2;
                    while lookahead < bytes.len() && !matches!(bytes[lookahead], b'\r' | b'\n') {
                        lookahead += 1;
                    }
                    continue;
                }
                if bytes.get(lookahead) == Some(&b'/') && bytes.get(lookahead + 1) == Some(&b'*') {
                    lookahead += 2;
                    while lookahead + 1 < bytes.len()
                        && !(bytes[lookahead] == b'*' && bytes[lookahead + 1] == b'/')
                    {
                        lookahead += 1;
                    }
                    if lookahead + 1 >= bytes.len() {
                        return Err("块注释未闭合".into());
                    }
                    lookahead += 2;
                    continue;
                }
                break;
            }
            if matches!(bytes.get(lookahead), Some(b'}' | b']')) {
                index += 1;
                continue;
            }
        }
        output.push(byte);
        index += 1;
    }
    if in_string {
        return Err("字符串未闭合".into());
    }
    Ok(output)
}

fn provider_view(id: &str, raw: &Value) -> Result<ProviderConfig, ModelConfigError> {
    let object = raw
        .as_object()
        .ok_or_else(|| ModelConfigError::ProviderNotObject(id.to_owned()))?;
    let api_raw = object.get("api").and_then(Value::as_str).map(str::to_owned);
    let api = api_raw.as_deref().and_then(ModelApi::parse);
    let models = object
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|raw| {
                    let id = raw.get("id")?.as_str()?.to_owned();
                    Some(ModelEntry {
                        id,
                        name: raw.get("name").and_then(Value::as_str).map(str::to_owned),
                        raw: raw.clone(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ProviderConfig {
        id: id.to_owned(),
        base_url: object
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(str::to_owned),
        api,
        api_raw,
        models,
        raw: raw.clone(),
    })
}

fn validate_root(root: &Value) -> Result<(), ModelConfigError> {
    let object = root.as_object().ok_or(ModelConfigError::RootNotObject)?;
    if let Some(providers) = object.get("providers") {
        let providers = providers
            .as_object()
            .ok_or(ModelConfigError::ProvidersNotObject)?;
        for (id, provider) in providers {
            if id.is_empty() || id.contains(['\r', '\n', '\0']) {
                return Err(ModelConfigError::InvalidProviderId);
            }
            if !provider.is_object() {
                return Err(ModelConfigError::ProviderNotObject(id.clone()));
            }
        }
    }
    Ok(())
}

fn validate_draft(draft: &ProviderDraft) -> Result<(), ModelConfigError> {
    validate_provider_id(&draft.id)?;
    validate_base_url(&draft.base_url)?;
    if draft.model_ids.is_empty() {
        return Err(ModelConfigError::MissingModels);
    }
    let mut ids = BTreeSet::new();
    for id in &draft.model_ids {
        let trimmed = id.trim();
        if trimmed.is_empty() || trimmed != id || !ids.insert(id) {
            return Err(ModelConfigError::InvalidModelId);
        }
    }
    Ok(())
}

fn validate_provider_id(provider_id: &str) -> Result<(), ModelConfigError> {
    if provider_id.is_empty()
        || !provider_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ModelConfigError::InvalidProviderId);
    }
    Ok(())
}

fn encode_literal_secret(secret: &str) -> String {
    let mut encoded = String::with_capacity(secret.len() + secret.matches('$').count() + 1);
    if secret.starts_with('!') {
        encoded.push('$');
    }
    for character in secret.chars() {
        if character == '$' {
            encoded.push('$');
        }
        encoded.push(character);
    }
    encoded
}

fn decode_literal_secret(encoded: &str) -> String {
    let mut decoded = String::with_capacity(encoded.len());
    let mut characters = encoded.chars();
    while let Some(character) = characters.next() {
        if character == '$' {
            if let Some(next @ ('$' | '!')) = characters.next() {
                decoded.push(next);
            } else {
                decoded.push('$');
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExternalReferenceKind {
    Environment,
    Command,
    Complex,
}

fn external_reference_kind(key: &str) -> Option<ExternalReferenceKind> {
    if key.starts_with('!') {
        return Some(ExternalReferenceKind::Command);
    }
    if simple_env_reference(key).is_some() {
        return Some(ExternalReferenceKind::Environment);
    }
    let mut characters = key.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '$' {
            continue;
        }
        match characters.peek() {
            Some('$' | '!') => {
                characters.next();
            }
            Some(_) => return Some(ExternalReferenceKind::Complex),
            None => {}
        }
    }
    None
}

fn simple_env_reference(key: &str) -> Option<&str> {
    let name = if let Some(name) = key
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    {
        name
    } else {
        key.strip_prefix('$')?
    };
    (!name.is_empty()
        && name.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        }))
    .then_some(name)
}

fn resolve_stored_secret(
    key: &str,
    env: Option<&Map<String, Value>>,
) -> Result<SecretString, ModelConfigError> {
    match external_reference_kind(key) {
        Some(ExternalReferenceKind::Command) => Err(ModelConfigError::CommandCredentialUnsupported),
        Some(ExternalReferenceKind::Complex) => Err(ModelConfigError::ComplexCredentialUnsupported),
        Some(ExternalReferenceKind::Environment) => {
            let name = simple_env_reference(key).expect("environment reference has a name");
            let value = env
                .and_then(|env| env.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| std::env::var(name).ok())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ModelConfigError::MissingCredentialEnvironment(name.to_owned()))?;
            SecretString::new(value)
        }
        None => SecretString::new(decode_literal_secret(key)),
    }
}

fn validate_secret(secret: &str) -> Result<(), ModelConfigError> {
    if secret.is_empty() || secret.contains(['\r', '\n', '\0']) {
        return Err(ModelConfigError::InvalidSecret);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn blank_and_missing_models_are_empty_documents() {
        let dir = tempdir().unwrap();
        let missing = ModelConfigDocument::load(dir.path()).unwrap();
        assert!(missing.providers().unwrap().is_empty());
        fs::write(dir.path().join("models.json"), "  \n").unwrap();
        assert!(
            ModelConfigDocument::load(dir.path())
                .unwrap()
                .providers()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn provider_roundtrip_preserves_unknown_fields_and_partial_cost() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("models.json"),
            serde_json::to_vec_pretty(&json!({
                "futureRoot": {"keep": true},
                "providers": {
                    "local": {
                        "baseUrl": "http://old.invalid/v1",
                        "api": "openai-completions",
                        "futureProvider": [1, 2],
                        "models": [{
                            "id": "model-a",
                            "name": "A",
                            "cost": {"input": 1.5},
                            "futureModel": {"keep": true}
                        }]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let mut document = ModelConfigDocument::load(dir.path()).unwrap();
        document
            .upsert_provider(&ProviderDraft {
                id: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
                api: Some(ModelApi::OpenAiCompletions),
                model_ids: vec!["model-a".into(), "model-b".into()],
            })
            .unwrap();
        document.save().unwrap();
        let saved = ModelConfigDocument::load(dir.path()).unwrap();
        assert_eq!(saved.value()["futureRoot"]["keep"], true);
        assert_eq!(
            saved.value()["providers"]["local"]["futureProvider"],
            json!([1, 2])
        );
        assert_eq!(
            saved.value()["providers"]["local"]["models"][0]["cost"],
            json!({"input": 1.5})
        );
        assert_eq!(
            saved.value()["providers"]["local"]["models"][0]["futureModel"]["keep"],
            true
        );
    }

    #[test]
    fn jsonc_comments_trailing_commas_and_utf8_bom_are_accepted() {
        let dir = tempdir().unwrap();
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice(
            br#"{
                // provider catalog
                "providers": {
                    "local": {
                        "baseUrl": "http://localhost:11434/v1", /* local */
                        "api": "openai-completions",
                        "models": [{"id": "a",}],
                    },
                },
            }"#,
        );
        fs::write(dir.path().join("models.json"), bytes).unwrap();
        let providers = ModelConfigDocument::load(dir.path())
            .unwrap()
            .providers()
            .unwrap();
        assert_eq!(providers[0].models[0].id, "a");
    }

    #[test]
    fn invalid_shapes_are_rejected() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("models.json"), "[]").unwrap();
        assert!(matches!(
            ModelConfigDocument::load(dir.path()),
            Err(ModelConfigError::RootNotObject)
        ));
        fs::write(dir.path().join("models.json"), r#"{"providers": []}"#).unwrap();
        assert!(matches!(
            ModelConfigDocument::load(dir.path()),
            Err(ModelConfigError::ProvidersNotObject)
        ));
    }

    #[test]
    fn concurrent_replacement_is_detected_before_save() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("models.json");
        fs::write(&path, "{}\n").unwrap();
        let mut document = ModelConfigDocument::load(dir.path()).unwrap();
        document
            .upsert_provider(&ProviderDraft {
                id: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
                api: Some(ModelApi::OpenAiCompletions),
                model_ids: vec!["a".into()],
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&path, "{\"external\":true}\n").unwrap();
        assert!(matches!(
            document.save(),
            Err(ModelConfigError::ConcurrentModification)
        ));
        assert_eq!(read_relaxed_json(&path).unwrap(), json!({"external": true}));
    }

    #[test]
    fn load_accepts_structural_provider_ids_but_writes_remain_strict() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("models.json"),
            r#"{"providers":{"future/provider":{"models":[]}}}"#,
        )
        .unwrap();
        let document = ModelConfigDocument::load(dir.path()).unwrap();
        assert_eq!(document.providers().unwrap()[0].id, "future/provider");
        let mut document = ModelConfigDocument::load(dir.path()).unwrap();
        assert!(matches!(
            document.remove_provider("future/provider"),
            Err(ModelConfigError::InvalidProviderId)
        ));
    }

    #[test]
    fn provider_save_preserves_models_with_unrecognized_ids() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("models.json"),
            serde_json::to_vec(&json!({
                "providers": {
                    "local": {
                        "models": [
                            {"id": "known", "name": "Known"},
                            {"id": 42, "future": true},
                            {"name": "identifier supplied by extension"}
                        ]
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let mut document = ModelConfigDocument::load(dir.path()).unwrap();
        document
            .upsert_provider(&ProviderDraft {
                id: "local".into(),
                base_url: "http://localhost:11434/v1".into(),
                api: Some(ModelApi::OpenAiCompletions),
                model_ids: vec!["known".into()],
            })
            .unwrap();
        document.save().unwrap();
        let models = document.value()["providers"]["local"]["models"]
            .as_array()
            .unwrap();
        assert_eq!(models.len(), 3);
        assert!(models.iter().any(|model| model["id"] == 42));
        assert!(
            models
                .iter()
                .any(|model| model["name"] == "identifier supplied by extension")
        );
    }

    #[test]
    fn auth_summary_never_contains_secret_and_api_key_mutations_preserve_oauth() {
        let dir = tempdir().unwrap();
        let secret = "top-secret-value";
        fs::write(
            auth_path(dir.path()),
            serde_json::to_vec(&json!({
                "openai": {"type": "api_key", "key": secret},
                "openai-codex": {"type": "oauth", "access": secret, "refresh": "r", "expires": 42}
            }))
            .unwrap(),
        )
        .unwrap();
        let summaries = read_auth_summaries(dir.path()).unwrap();
        let rendered = format!("{summaries:?}");
        assert!(!rendered.contains(secret));
        assert!(remove_api_key(dir.path(), "openai").unwrap());
        assert!(!remove_api_key(dir.path(), "openai-codex").unwrap());
        assert!(matches!(
            write_api_key(
                dir.path(),
                "openai-codex",
                SecretString::new("replacement".into()).unwrap()
            ),
            Err(ModelConfigError::CredentialWouldBeOverwritten(_))
        ));
        let auth = read_relaxed_json(&auth_path(dir.path())).unwrap();
        assert_eq!(auth["openai-codex"]["type"], "oauth");
        assert_eq!(auth["openai-codex"]["access"], secret);
    }

    #[test]
    fn api_key_update_preserves_env_and_unknown_fields_and_rejects_unknown_type() {
        let dir = tempdir().unwrap();
        let path = auth_path(dir.path());
        fs::write(
            &path,
            r#"{"openai":{"type":"api_key","key":"old","env":{"PROFILE":"work"},"future":{"keep":true}},"other":{"type":"future_auth","token":"keep"}}"#,
        )
        .unwrap();
        write_api_key(
            dir.path(),
            "openai",
            SecretString::new("new".into()).unwrap(),
        )
        .unwrap();
        let root = read_relaxed_json(&path).unwrap();
        assert_eq!(root["openai"]["key"], "new");
        assert_eq!(root["openai"]["env"]["PROFILE"], "work");
        assert_eq!(root["openai"]["future"]["keep"], true);
        let before = fs::read(&path).unwrap();
        assert!(matches!(
            write_api_key(
                dir.path(),
                "other",
                SecretString::new("blocked".into()).unwrap()
            ),
            Err(ModelConfigError::CredentialWouldBeOverwritten(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn remove_api_key_preserves_non_key_auth_fields() {
        for (name, credential, expected, removed) in [
            (
                "env-only",
                json!({"type":"api_key","env":{"AWS_PROFILE":"work"}}),
                json!({"type":"api_key","env":{"AWS_PROFILE":"work"}}),
                false,
            ),
            (
                "env-key",
                json!({"type":"api_key","key":"secret","env":{"AWS_PROFILE":"work"}}),
                json!({"type":"api_key","env":{"AWS_PROFILE":"work"}}),
                true,
            ),
            (
                "unknown-key",
                json!({"type":"api_key","key":"secret","future":true}),
                json!({"type":"api_key","future":true}),
                true,
            ),
            (
                "pure-key",
                json!({"type":"api_key","key":"secret"}),
                Value::Null,
                true,
            ),
        ] {
            let dir = tempdir().unwrap();
            let path = auth_path(dir.path());
            fs::write(
                &path,
                serde_json::to_vec(&json!({name: credential})).unwrap(),
            )
            .unwrap();
            assert_eq!(remove_api_key(dir.path(), name).unwrap(), removed);
            let root = read_relaxed_json(&path).unwrap();
            if expected.is_null() {
                assert!(root.get(name).is_none());
            } else {
                assert_eq!(root[name], expected);
            }
        }
    }

    #[test]
    fn auth_summary_distinguishes_env_only_from_removable_key() {
        let dir = tempdir().unwrap();
        fs::write(
            auth_path(dir.path()),
            r#"{"bedrock":{"type":"api_key","env":{"AWS_PROFILE":"work"}},"openai":{"type":"api_key","key":"literal"}}"#,
        )
        .unwrap();
        let summaries = read_auth_summaries(dir.path()).unwrap();
        let env_only = summaries
            .iter()
            .find(|item| item.provider_id == "bedrock")
            .unwrap();
        assert!(env_only.configured);
        assert!(env_only.external_env);
        assert!(!env_only.has_key);
        let key = summaries
            .iter()
            .find(|item| item.provider_id == "openai")
            .unwrap();
        assert!(key.has_key);
        assert!(!key.external_env);
    }

    #[test]
    fn api_key_write_replaces_only_api_key_atomically() {
        let dir = tempdir().unwrap();
        write_api_key(
            dir.path(),
            "openai",
            SecretString::new("first".into()).unwrap(),
        )
        .unwrap();
        write_api_key(
            dir.path(),
            "openai",
            SecretString::new("second".into()).unwrap(),
        )
        .unwrap();
        let stored = read_api_key(dir.path(), "openai").unwrap().unwrap();
        assert_eq!(stored.expose(), "second");
        assert_eq!(dir.path().read_dir().unwrap().count(), 1);
    }

    #[test]
    fn auth_mutation_revision_conflict_preserves_external_update() {
        let dir = tempdir().unwrap();
        let path = auth_path(dir.path());
        fs::write(&path, r#"{"openai":{"type":"api_key","key":"old"}}"#).unwrap();
        let (_, expected_revision) = read_relaxed_json_with_revision(&path).unwrap();
        fs::write(&path, r#"{"external":{"type":"oauth","access":"token"}}"#).unwrap();
        let mut root = json!({"openai": {"type": "api_key", "key": "new"}});
        let bytes = serialize_json(&path, &root).unwrap();
        let result =
            write_bytes_atomic_if(&path, &bytes, || verify_revision(&path, &expected_revision));
        assert!(matches!(
            result,
            Err(ModelConfigError::ConcurrentModification)
        ));
        root = read_relaxed_json(&path).unwrap();
        assert_eq!(root["external"]["type"], "oauth");
    }

    #[test]
    fn unknown_api_roundtrip_is_preserved_until_user_explicitly_changes_it() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("models.json"),
            r#"{"providers":{"vertex":{"baseUrl":"https://example.test","api":"google-vertex","models":[{"id":"gemini"}]}}}"#,
        )
        .unwrap();
        let mut document = ModelConfigDocument::load(dir.path()).unwrap();
        let provider = document.providers().unwrap().remove(0);
        assert_eq!(provider.api, None);
        assert_eq!(provider.api_raw.as_deref(), Some("google-vertex"));
        document
            .upsert_provider(&ProviderDraft {
                id: "vertex".into(),
                base_url: "https://example.test".into(),
                api: None,
                model_ids: vec!["gemini".into()],
            })
            .unwrap();
        document.save().unwrap();
        assert_eq!(
            ModelConfigDocument::load(dir.path()).unwrap().value()["providers"]["vertex"]["api"],
            "google-vertex"
        );
    }

    #[test]
    fn literal_secret_encoding_matches_upstream_resolution_rules() {
        for secret in [
            "$TOKEN",
            "${TOKEN}",
            "!command",
            "$!already-escaped$$combo",
            "prefix$$${TOKEN}!suffix",
        ] {
            let encoded = encode_literal_secret(secret);
            assert_eq!(decode_literal_secret(&encoded), secret);
            assert_ne!(
                external_reference_kind(&encoded),
                Some(ExternalReferenceKind::Command)
            );
            assert!(simple_env_reference(&encoded).is_none());
        }
    }

    #[test]
    fn api_key_roundtrip_escapes_literals_and_resolves_safe_external_references() {
        let dir = tempdir().unwrap();
        let secret = "!literal-$TOKEN-${TOKEN}-$$-$!";
        write_api_key(
            dir.path(),
            "openai",
            SecretString::new(secret.into()).unwrap(),
        )
        .unwrap();
        let raw = read_relaxed_json(&auth_path(dir.path())).unwrap();
        let stored = raw["openai"]["key"].as_str().unwrap();
        assert_eq!(decode_literal_secret(stored), secret);
        assert_eq!(
            read_api_key(dir.path(), "openai")
                .unwrap()
                .unwrap()
                .expose(),
            secret
        );

        fs::write(
            auth_path(dir.path()),
            r#"{"openai":{"type":"api_key","key":"$R16_TEST_KEY","env":{"R16_TEST_KEY":"from-map"}}}"#,
        )
        .unwrap();
        assert_eq!(
            read_api_key(dir.path(), "openai")
                .unwrap()
                .unwrap()
                .expose(),
            "from-map"
        );
        fs::write(
            auth_path(dir.path()),
            r#"{"openai":{"type":"api_key","key":"!echo forbidden"}}"#,
        )
        .unwrap();
        assert!(matches!(
            read_api_key(dir.path(), "openai"),
            Err(ModelConfigError::CommandCredentialUnsupported)
        ));
    }

    #[test]
    fn rewrite_trivia_detection_covers_comments_and_trailing_commas() {
        assert!(detect_rewrite_trivia(br#"{"a":1,// note\n}"#));
        assert!(detect_rewrite_trivia(br#"{"a":[1,]}"#));
        assert!(!detect_rewrite_trivia(
            br#"{"text":"// not a comment","a":1}"#
        ));
    }

    #[test]
    fn discovery_parsers_deduplicate_all_supported_shapes() {
        let openai = br#"{"data":[{"id":"b"},{"id":"a"},{"id":"a"}]}"#;
        assert_eq!(
            parse_discovered_models(ModelApi::OpenAiResponses, openai).unwrap(),
            vec!["a", "b"]
        );
        let anthropic = br#"{"data":[{"id":"claude-b"},{"id":"claude-a"}]}"#;
        assert_eq!(
            parse_discovered_models(ModelApi::AnthropicMessages, anthropic).unwrap(),
            vec!["claude-a", "claude-b"]
        );
        let google = br#"{"models":[{"name":"models/gemini-b"},{"name":"models/gemini-a"}]}"#;
        assert_eq!(
            parse_discovered_models(ModelApi::GoogleGenerativeAi, google).unwrap(),
            vec!["gemini-a", "gemini-b"]
        );
    }
}
