//! pi 0.84.2 RPC wire protocol.
//!
//! pi 自身允许 provider、extension 与 session entry 携带任意 JSON，因此这些扩展点保留
//! `serde_json::Value`；稳定的 envelope、命令、响应数据和事件字段均使用强类型。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageContent {
    #[serde(rename = "type")]
    pub kind: ImageKind,
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageKind {
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    All,
    OneAtATime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// 32 个 v0.84.2 command。请求 id 由 [`RpcRequest`] envelope 单独承载。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Prompt {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
        #[serde(
            rename = "streamingBehavior",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        streaming_behavior: Option<StreamingBehavior>,
    },
    Steer {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    FollowUp {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        images: Option<Vec<ImageContent>>,
    },
    Abort,
    NewSession {
        #[serde(
            rename = "parentSession",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        parent_session: Option<String>,
    },
    GetState,
    SetModel {
        provider: String,
        #[serde(rename = "modelId")]
        model_id: String,
    },
    CycleModel,
    GetAvailableModels,
    SetThinkingLevel {
        level: ThinkingLevel,
    },
    CycleThinkingLevel,
    GetAvailableThinkingLevels,
    SetSteeringMode {
        mode: QueueMode,
    },
    SetFollowUpMode {
        mode: QueueMode,
    },
    Compact {
        #[serde(
            rename = "customInstructions",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        custom_instructions: Option<String>,
    },
    SetAutoCompaction {
        enabled: bool,
    },
    SetAutoRetry {
        enabled: bool,
    },
    AbortRetry,
    Bash {
        command: String,
        #[serde(
            rename = "excludeFromContext",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        exclude_from_context: Option<bool>,
    },
    AbortBash,
    GetSessionStats,
    ExportHtml {
        #[serde(
            rename = "outputPath",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        output_path: Option<String>,
    },
    SwitchSession {
        #[serde(rename = "sessionPath")]
        session_path: String,
    },
    Fork {
        #[serde(rename = "entryId")]
        entry_id: String,
    },
    Clone,
    GetForkMessages,
    GetEntries {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<String>,
    },
    GetTree,
    GetLastAssistantText,
    SetSessionName {
        name: String,
    },
    GetMessages,
    GetCommands,
}

impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Prompt { .. } => "prompt",
            Self::Steer { .. } => "steer",
            Self::FollowUp { .. } => "follow_up",
            Self::Abort => "abort",
            Self::NewSession { .. } => "new_session",
            Self::GetState => "get_state",
            Self::SetModel { .. } => "set_model",
            Self::CycleModel => "cycle_model",
            Self::GetAvailableModels => "get_available_models",
            Self::SetThinkingLevel { .. } => "set_thinking_level",
            Self::CycleThinkingLevel => "cycle_thinking_level",
            Self::GetAvailableThinkingLevels => "get_available_thinking_levels",
            Self::SetSteeringMode { .. } => "set_steering_mode",
            Self::SetFollowUpMode { .. } => "set_follow_up_mode",
            Self::Compact { .. } => "compact",
            Self::SetAutoCompaction { .. } => "set_auto_compaction",
            Self::SetAutoRetry { .. } => "set_auto_retry",
            Self::AbortRetry => "abort_retry",
            Self::Bash { .. } => "bash",
            Self::AbortBash => "abort_bash",
            Self::GetSessionStats => "get_session_stats",
            Self::ExportHtml { .. } => "export_html",
            Self::SwitchSession { .. } => "switch_session",
            Self::Fork { .. } => "fork",
            Self::Clone => "clone",
            Self::GetForkMessages => "get_fork_messages",
            Self::GetEntries { .. } => "get_entries",
            Self::GetTree => "get_tree",
            Self::GetLastAssistantText => "get_last_assistant_text",
            Self::SetSessionName { .. } => "set_session_name",
            Self::GetMessages => "get_messages",
            Self::GetCommands => "get_commands",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamingBehavior {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "followUp")]
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: ResponseKind,
    pub command: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RpcResponse {
    pub fn decode_data<T: for<'de> Deserialize<'de>>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.data.clone().unwrap_or(Value::Null))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseKind {
    Response,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSessionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub steering_mode: QueueMode,
    pub follow_up_mode: QueueMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    pub auto_compaction_enabled: bool,
    pub message_count: u64,
    pub pending_message_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
    pub api: String,
    pub provider: String,
    pub base_url: String,
    pub reasoning: bool,
    pub input: Vec<ModelInput>,
    pub cost: ModelCost,
    pub context_window: u64,
    pub max_tokens: u64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelInput {
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiers: Option<Vec<ModelCostTier>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCostTier {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub input_tokens_above: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelledData {
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleModelData {
    pub model: Model,
    pub thinking_level: ThinkingLevel,
    pub is_scoped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AvailableModelsData {
    pub models: Vec<Model>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingLevelData {
    pub level: ThinkingLevel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingLevelsData {
    pub levels: Vec<ThinkingLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportPathData {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkData {
    pub text: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkMessage {
    pub entry_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkMessagesData {
    pub messages: Vec<ForkMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntriesData {
    pub entries: Vec<SessionEntry>,
    pub leaf_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeData {
    pub tree: Vec<SessionTreeNode>,
    pub leaf_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastAssistantTextData {
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagesData {
    pub messages: Vec<AgentMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandsData {
    pub commands: Vec<RpcSlashCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BashResult {
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_output_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionResult {
    pub summary: String,
    pub first_kept_entry_id: String,
    pub tokens_before: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_tokens_after: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write1h: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<u64>,
    pub total_tokens: u64,
    pub cost: UsageCost,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_file: Option<String>,
    pub session_id: String,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub tokens: SessionTokens,
    pub cost: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_usage: Option<ContextUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub tokens: Option<u64>,
    pub context_window: u64,
    pub percent: Option<f64>,
}

/// AgentMessage 是可由 extension 扩展的开放 union，完整保留其 JSON。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentMessage(pub Value);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeNode {
    pub entry: SessionEntry,
    pub children: Vec<SessionTreeNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub source: String,
    pub scope: SourceScope,
    pub origin: SourceOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceScope {
    User,
    Project,
    Temporary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceOrigin {
    Package,
    TopLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlashCommandSource {
    Extension,
    Prompt,
    Skill,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcSlashCommand {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: SlashCommandSource,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcEvent {
    AgentStart,
    AgentEnd {
        messages: Vec<AgentMessage>,
        #[serde(rename = "willRetry")]
        will_retry: bool,
    },
    AgentSettled,
    TurnStart,
    TurnEnd {
        message: AgentMessage,
        #[serde(rename = "toolResults")]
        tool_results: Vec<AgentMessage>,
    },
    MessageStart {
        message: AgentMessage,
    },
    MessageUpdate {
        usage: Usage,
        #[serde(rename = "assistantMessageEvent")]
        assistant_message_event: AssistantMessageEvent,
    },
    MessageEnd {
        message: AgentMessage,
    },
    ToolExecutionStart {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        args: Value,
    },
    ToolExecutionUpdate {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        args: Value,
        #[serde(rename = "partialResult")]
        partial_result: Value,
    },
    ToolExecutionEnd {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        result: Value,
        #[serde(rename = "isError")]
        is_error: bool,
    },
    QueueUpdate {
        steering: Vec<String>,
        #[serde(rename = "followUp")]
        follow_up: Vec<String>,
    },
    CompactionStart {
        reason: CompactionReason,
    },
    CompactionEnd {
        reason: CompactionReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<CompactionResult>,
        aborted: bool,
        #[serde(rename = "willRetry")]
        will_retry: bool,
        #[serde(
            rename = "errorMessage",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        error_message: Option<String>,
    },
    AutoRetryStart {
        attempt: u32,
        #[serde(rename = "maxAttempts")]
        max_attempts: u32,
        #[serde(rename = "delayMs")]
        delay_ms: u64,
        #[serde(rename = "errorMessage")]
        error_message: String,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        #[serde(
            rename = "finalError",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        final_error: Option<String>,
    },
    SummarizationRetryScheduled {
        attempt: u32,
        #[serde(rename = "maxAttempts")]
        max_attempts: u32,
        #[serde(rename = "delayMs")]
        delay_ms: u64,
        #[serde(rename = "errorMessage")]
        error_message: String,
    },
    SummarizationRetryAttemptStart {
        source: SummarizationSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<CompactionReason>,
    },
    SummarizationRetryFinished,
    BashExecutionUpdate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        delta: String,
    },
    EntryAppended {
        entry: SessionEntry,
    },
    SessionInfoChanged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    ThinkingLevelChanged {
        level: ThinkingLevel,
    },
    ExtensionError {
        #[serde(rename = "extensionPath")]
        extension_path: String,
        event: String,
        error: String,
    },
    ExtensionUiRequest {
        id: String,
        #[serde(flatten)]
        request: ExtensionUiRequest,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompactionReason {
    Manual,
    Threshold,
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SummarizationSource {
    #[serde(rename = "branchSummary")]
    BranchSummary,
    #[serde(rename = "compaction")]
    Compaction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantMessageEvent {
    Start,
    TextStart {
        #[serde(rename = "contentIndex")]
        content_index: usize,
    },
    TextDelta {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        delta: String,
    },
    TextEnd {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        content: String,
    },
    ThinkingStart {
        #[serde(rename = "contentIndex")]
        content_index: usize,
    },
    ThinkingDelta {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        delta: String,
    },
    ThinkingEnd {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        content: String,
    },
    ToolcallStart {
        #[serde(rename = "contentIndex")]
        content_index: usize,
    },
    ToolcallDelta {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        delta: String,
    },
    ToolcallEnd {
        #[serde(rename = "contentIndex")]
        content_index: usize,
        #[serde(rename = "toolCall")]
        tool_call: Value,
    },
    Done {
        reason: DoneReason,
        message: AgentMessage,
    },
    Error {
        reason: ErrorReason,
        error: AgentMessage,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoneReason {
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "length")]
    Length,
    #[serde(rename = "toolUse")]
    ToolUse,
    #[serde(rename = "deferred")]
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorReason {
    Aborted,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ExtensionUiRequest {
    #[serde(rename = "select")]
    Select {
        title: String,
        options: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "confirm")]
    Confirm {
        title: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "input")]
    Input {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout: Option<u64>,
    },
    #[serde(rename = "editor")]
    Editor {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefill: Option<String>,
    },
    #[serde(rename = "notify")]
    Notify {
        message: String,
        #[serde(
            rename = "notifyType",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        notify_type: Option<NotifyType>,
    },
    #[serde(rename = "setStatus")]
    SetStatus {
        #[serde(rename = "statusKey")]
        status_key: String,
        #[serde(
            rename = "statusText",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        status_text: Option<String>,
    },
    #[serde(rename = "setWidget")]
    SetWidget {
        #[serde(rename = "widgetKey")]
        widget_key: String,
        #[serde(
            rename = "widgetLines",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        widget_lines: Option<Vec<String>>,
        #[serde(
            rename = "widgetPlacement",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        widget_placement: Option<WidgetPlacement>,
    },
    #[serde(rename = "setTitle")]
    SetTitle { title: String },
    #[serde(rename = "set_editor_text")]
    SetEditorText { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyType {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetPlacement {
    #[serde(rename = "aboveEditor")]
    AboveEditor,
    #[serde(rename = "belowEditor")]
    BelowEditor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionUiResponse {
    ExtensionUiResponse {
        id: String,
        #[serde(flatten)]
        response: ExtensionUiResponseValue,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExtensionUiResponseValue {
    Value { value: String },
    Confirmed { confirmed: bool },
    Cancelled { cancelled: bool },
}

impl ExtensionUiResponse {
    pub fn value(id: impl Into<String>, value: impl Into<String>) -> Self {
        Self::ExtensionUiResponse {
            id: id.into(),
            response: ExtensionUiResponseValue::Value {
                value: value.into(),
            },
        }
    }

    pub fn confirmed(id: impl Into<String>, confirmed: bool) -> Self {
        Self::ExtensionUiResponse {
            id: id.into(),
            response: ExtensionUiResponseValue::Confirmed { confirmed },
        }
    }

    pub fn cancelled(id: impl Into<String>) -> Self {
        Self::ExtensionUiResponse {
            id: id.into(),
            response: ExtensionUiResponseValue::Cancelled { cancelled: true },
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::ExtensionUiResponse { id, .. } => id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_all_32_commands() {
        let commands = vec![
            Command::Prompt {
                message: "x".into(),
                images: None,
                streaming_behavior: None,
            },
            Command::Steer {
                message: "x".into(),
                images: None,
            },
            Command::FollowUp {
                message: "x".into(),
                images: None,
            },
            Command::Abort,
            Command::NewSession {
                parent_session: None,
            },
            Command::GetState,
            Command::SetModel {
                provider: "p".into(),
                model_id: "m".into(),
            },
            Command::CycleModel,
            Command::GetAvailableModels,
            Command::SetThinkingLevel {
                level: ThinkingLevel::Off,
            },
            Command::CycleThinkingLevel,
            Command::GetAvailableThinkingLevels,
            Command::SetSteeringMode {
                mode: QueueMode::All,
            },
            Command::SetFollowUpMode {
                mode: QueueMode::OneAtATime,
            },
            Command::Compact {
                custom_instructions: None,
            },
            Command::SetAutoCompaction { enabled: false },
            Command::SetAutoRetry { enabled: false },
            Command::AbortRetry,
            Command::Bash {
                command: "echo ok".into(),
                exclude_from_context: Some(true),
            },
            Command::AbortBash,
            Command::GetSessionStats,
            Command::ExportHtml { output_path: None },
            Command::SwitchSession {
                session_path: "s".into(),
            },
            Command::Fork {
                entry_id: "e".into(),
            },
            Command::Clone,
            Command::GetForkMessages,
            Command::GetEntries { since: None },
            Command::GetTree,
            Command::GetLastAssistantText,
            Command::SetSessionName { name: "n".into() },
            Command::GetMessages,
            Command::GetCommands,
        ];
        assert_eq!(commands.len(), 32);
        for command in commands {
            let value = serde_json::to_value(&command).unwrap();
            assert_eq!(value["type"], command.name());
            assert_eq!(serde_json::from_value::<Command>(value).unwrap(), command);
        }
    }

    #[test]
    fn deserializes_documented_and_source_only_events() {
        let fixtures = [
            r#"{"type":"agent_start"}"#,
            r#"{"type":"agent_end","messages":[],"willRetry":false}"#,
            r#"{"type":"agent_settled"}"#,
            r#"{"type":"turn_start"}"#,
            r#"{"type":"turn_end","message":{"role":"user"},"toolResults":[]}"#,
            r#"{"type":"message_start","message":{"role":"user"}}"#,
            r#"{"type":"message_update","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"assistantMessageEvent":{"type":"start"}}"#,
            r#"{"type":"message_end","message":{"role":"user"}}"#,
            r#"{"type":"tool_execution_start","toolCallId":"t","toolName":"x","args":{}}"#,
            r#"{"type":"tool_execution_update","toolCallId":"t","toolName":"x","args":{},"partialResult":{}}"#,
            r#"{"type":"tool_execution_end","toolCallId":"t","toolName":"x","result":{},"isError":false}"#,
            r#"{"type":"queue_update","steering":[],"followUp":[]}"#,
            r#"{"type":"compaction_start","reason":"manual"}"#,
            r#"{"type":"compaction_end","reason":"manual","aborted":true,"willRetry":false}"#,
            r#"{"type":"auto_retry_start","attempt":1,"maxAttempts":3,"delayMs":1,"errorMessage":"x"}"#,
            r#"{"type":"auto_retry_end","success":true,"attempt":1}"#,
            r#"{"type":"summarization_retry_scheduled","attempt":1,"maxAttempts":3,"delayMs":1,"errorMessage":"x"}"#,
            r#"{"type":"summarization_retry_attempt_start","source":"branchSummary"}"#,
            r#"{"type":"summarization_retry_finished"}"#,
            r#"{"type":"bash_execution_update","id":"r","delta":"x"}"#,
            r#"{"type":"entry_appended","entry":{"type":"custom","id":"e","parentId":null,"timestamp":"now"}}"#,
            r#"{"type":"session_info_changed","name":"n"}"#,
            r#"{"type":"thinking_level_changed","level":"off"}"#,
            r#"{"type":"extension_error","extensionPath":"x","event":"e","error":"bad"}"#,
            r#"{"type":"extension_ui_request","id":"u","method":"confirm","title":"t","message":"m"}"#,
        ];
        for fixture in fixtures {
            serde_json::from_str::<RpcEvent>(fixture).unwrap();
        }
    }
}
