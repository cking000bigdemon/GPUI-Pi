use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use pi_render::{
    ConversationDocument, LiveAssistantUpdate, LiveBlockKind, LiveEvent, LivePhase,
    LiveSessionReducer,
};

use pi_rpc::{
    AssistantMessageEvent, AvailableModelsData, Client, ClientConfig, ClientEvent, CloneData,
    Command, CommandsData, CompactionResult, ExportPathData, ExtensionUiRequest,
    ExtensionUiResponse, ForkData, ImageContent, ImageKind, Model, NotifyType, RpcEvent,
    RpcSessionState, RpcSlashCommand, StreamingBehavior, ThinkingLevel, ThinkingLevelsData,
    TreeData, WidgetPlacement,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Extension slash command handlers may synchronously wait for several human-operated dialogs
/// before the official RPC emits the prompt response. Keep this bounded, but do not apply the
/// metadata/control timeout to interactive submissions.
const INTERACTIVE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const PUMP_FRAME: Duration = Duration::from_millis(20);
const MAX_EVENTS_PER_BATCH: usize = 512;
const EXTENSION_TEXT_LIMIT: usize = 4096;
const EXTENSION_EDITABLE_BYTES_LIMIT: usize = 1024 * 1024;
const EXTENSION_SELECT_RAW_BYTES_LIMIT: usize = 1024 * 1024;
const EXTENSION_KEY_LIMIT: usize = 64;
const EXTENSION_STATUS_LIMIT: usize = 256;
const EXTENSION_STATUS_COUNT_LIMIT: usize = 16;
const EXTENSION_WIDGET_COUNT_LIMIT: usize = 8;
const EXTENSION_WIDGET_LINE_LIMIT: usize = 256;
const EXTENSION_WIDGET_LINES_LIMIT: usize = 8;
const EXTENSION_DIALOG_TITLE_LIMIT: usize = 128;
const EXTENSION_DIALOG_MESSAGE_LIMIT: usize = 2048;
const EXTENSION_DIALOG_OPTION_LIMIT: usize = 256;
const EXTENSION_DIALOG_OPTIONS_LIMIT: usize = 50;
const EXTENSION_DIALOG_QUEUE_LIMIT: usize = 32;
const EXTENSION_NOTIFICATION_QUEUE_LIMIT: usize = 64;

pub const UNSUPPORTED_BY_PINNED_RPC: &str = "UNSUPPORTED_BY_PINNED_RPC";

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionDialogRequest {
    pub id: String,
    pub request: ExtensionUiRequest,
    pub select_options: Option<Vec<ExtensionSelectOption>>,
    pub deadline: Option<Instant>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSelectOption {
    pub raw: String,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionNotification {
    pub message: String,
    pub notify_type: NotifyType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionWidget {
    pub raw_key: String,
    pub display_key: String,
    pub lines: Vec<String>,
    pub placement: WidgetPlacement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionStatus {
    pub raw_key: String,
    pub display_key: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtensionUiState {
    dialogs: VecDeque<ExtensionDialogRequest>,
    next_dialog_sequence: u64,
    pending_ids: HashSet<String>,
    statuses: BTreeMap<String, ExtensionStatus>,
    widgets: BTreeMap<String, ExtensionWidget>,
    notifications: VecDeque<ExtensionNotification>,
    diagnostics: VecDeque<String>,
    title: Option<String>,
    editor_text: Option<String>,
    has_seen_extension_ui: bool,
}

impl ExtensionUiState {
    /// 应用请求；无法入队的交互请求必须由调用方立即回传取消，避免扩展永久等待。
    pub fn apply(
        &mut self,
        id: String,
        request: ExtensionUiRequest,
    ) -> Option<ExtensionUiResponse> {
        self.has_seen_extension_ui = true;
        if self.pending_ids.contains(&id) {
            return None;
        }
        let request = sanitize_extension_request(request);
        match request {
            ExtensionUiRequest::Select {
                title,
                options,
                timeout,
            } => {
                if options.is_empty() {
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                if options.len() > EXTENSION_DIALOG_OPTIONS_LIMIT {
                    self.push_diagnostic(format!(
                        "Extension UI Select {id} 有 {} 个选项，超过上限 {EXTENSION_DIALOG_OPTIONS_LIMIT}，已取消",
                        options.len()
                    ));
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                let raw_bytes = options.iter().map(String::len).sum::<usize>();
                if raw_bytes > EXTENSION_SELECT_RAW_BYTES_LIMIT {
                    self.push_diagnostic(format!(
                        "Extension UI Select {id} 原始选项总量超过 1 MiB，已取消"
                    ));
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                if self.dialogs.len() >= EXTENSION_DIALOG_QUEUE_LIMIT {
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                let select_options = options
                    .into_iter()
                    .map(|raw| ExtensionSelectOption {
                        display: sanitize_extension_single_line(
                            &raw,
                            EXTENSION_DIALOG_OPTION_LIMIT,
                        ),
                        raw,
                    })
                    .collect();
                self.pending_ids.insert(id.clone());
                let sequence = self.next_dialog_sequence;
                self.next_dialog_sequence = self.next_dialog_sequence.wrapping_add(1);
                self.dialogs.push_back(ExtensionDialogRequest {
                    id,
                    request: ExtensionUiRequest::Select {
                        title,
                        options: Vec::new(),
                        timeout,
                    },
                    select_options: Some(select_options),
                    deadline: timeout
                        .map(|timeout| Instant::now() + Duration::from_millis(timeout.max(1))),
                    sequence,
                });
            }
            ExtensionUiRequest::Confirm { timeout, .. }
            | ExtensionUiRequest::Input { timeout, .. } => {
                if self.dialogs.len() >= EXTENSION_DIALOG_QUEUE_LIMIT {
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                self.pending_ids.insert(id.clone());
                let sequence = self.next_dialog_sequence;
                self.next_dialog_sequence = self.next_dialog_sequence.wrapping_add(1);
                self.dialogs.push_back(ExtensionDialogRequest {
                    id,
                    request,
                    select_options: None,
                    deadline: timeout
                        .map(|timeout| Instant::now() + Duration::from_millis(timeout.max(1))),
                    sequence,
                });
            }
            ExtensionUiRequest::Editor { title, prefill } => {
                if prefill
                    .as_ref()
                    .is_some_and(|prefill| prefill.len() > EXTENSION_EDITABLE_BYTES_LIMIT)
                {
                    self.push_diagnostic(format!(
                        "Extension UI Editor {id} prefill 超过 1 MiB，已取消"
                    ));
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                let prefill = prefill.map(|prefill| {
                    let sanitized = sanitize_extension_editable(&prefill);
                    if sanitized != prefill {
                        self.push_diagnostic(format!(
                            "Extension UI Editor {id} prefill 含控制/Cf 字符，已过滤但未截断"
                        ));
                    }
                    sanitized
                });
                if self.dialogs.len() >= EXTENSION_DIALOG_QUEUE_LIMIT {
                    return Some(ExtensionUiResponse::cancelled(id));
                }
                self.pending_ids.insert(id.clone());
                let sequence = self.next_dialog_sequence;
                self.next_dialog_sequence = self.next_dialog_sequence.wrapping_add(1);
                self.dialogs.push_back(ExtensionDialogRequest {
                    id,
                    request: ExtensionUiRequest::Editor { title, prefill },
                    select_options: None,
                    deadline: None,
                    sequence,
                });
            }
            ExtensionUiRequest::Notify {
                message,
                notify_type,
            } => {
                if self.notifications.len() >= EXTENSION_NOTIFICATION_QUEUE_LIMIT {
                    self.notifications.pop_front();
                }
                self.notifications.push_back(ExtensionNotification {
                    message: sanitize_extension_text(&message, EXTENSION_TEXT_LIMIT),
                    notify_type: notify_type.unwrap_or(NotifyType::Info),
                });
            }
            ExtensionUiRequest::SetStatus {
                status_key,
                status_text,
            } => {
                if let Some(text) = status_text {
                    if self.statuses.contains_key(&status_key)
                        || self.statuses.len() < EXTENSION_STATUS_COUNT_LIMIT
                    {
                        self.statuses.insert(
                            status_key.clone(),
                            ExtensionStatus {
                                display_key: sanitize_extension_single_line(
                                    &status_key,
                                    EXTENSION_KEY_LIMIT,
                                ),
                                raw_key: status_key,
                                text: sanitize_extension_single_line(&text, EXTENSION_STATUS_LIMIT),
                            },
                        );
                    }
                } else {
                    self.statuses.remove(&status_key);
                }
            }
            ExtensionUiRequest::SetWidget {
                widget_key,
                widget_lines,
                widget_placement,
            } => {
                if let Some(lines) = widget_lines {
                    if self.widgets.contains_key(&widget_key)
                        || self.widgets.len() < EXTENSION_WIDGET_COUNT_LIMIT
                    {
                        let lines = lines
                            .into_iter()
                            .take(EXTENSION_WIDGET_LINES_LIMIT)
                            .map(|line| {
                                sanitize_extension_single_line(&line, EXTENSION_WIDGET_LINE_LIMIT)
                            })
                            .collect();
                        self.widgets.insert(
                            widget_key.clone(),
                            ExtensionWidget {
                                display_key: sanitize_extension_single_line(
                                    &widget_key,
                                    EXTENSION_KEY_LIMIT,
                                ),
                                raw_key: widget_key,
                                lines,
                                placement: widget_placement.unwrap_or(WidgetPlacement::AboveEditor),
                            },
                        );
                    }
                } else {
                    self.widgets.remove(&widget_key);
                }
            }
            ExtensionUiRequest::SetTitle { title } => {
                self.title = Some(sanitize_extension_single_line(&title, EXTENSION_TEXT_LIMIT));
            }
            ExtensionUiRequest::SetEditorText { text } => {
                if text.len() > EXTENSION_EDITABLE_BYTES_LIMIT {
                    self.push_diagnostic(
                        "Extension UI set_editor_text 超过 1 MiB，未替换编辑器内容".to_owned(),
                    );
                } else {
                    let sanitized = sanitize_extension_editable(&text);
                    if sanitized != text {
                        self.push_diagnostic(
                            "Extension UI set_editor_text 含控制/Cf 字符，已过滤但未截断"
                                .to_owned(),
                        );
                    }
                    self.editor_text = Some(sanitized);
                }
            }
        }
        None
    }

    pub fn active_dialog(&self) -> Option<&ExtensionDialogRequest> {
        self.dialogs.front()
    }

    pub fn is_dialog_pending(&self, id: &str) -> bool {
        self.pending_ids.contains(id)
    }

    pub fn finish_dialog(&mut self, id: &str) -> bool {
        if self.dialogs.front().is_none_or(|dialog| dialog.id != id) {
            return false;
        }
        self.dialogs.pop_front();
        self.pending_ids.remove(id)
    }

    pub fn active_dialog_expired(&self, now: Instant) -> bool {
        self.dialogs
            .front()
            .and_then(|dialog| dialog.deadline)
            .is_some_and(|deadline| now >= deadline)
    }

    pub fn statuses(&self) -> impl Iterator<Item = &ExtensionStatus> {
        self.statuses.values()
    }

    pub fn widgets(&self, placement: WidgetPlacement) -> impl Iterator<Item = &ExtensionWidget> {
        self.widgets
            .values()
            .filter(move |widget| widget.placement == placement)
    }

    pub fn take_notification(&mut self) -> Option<ExtensionNotification> {
        self.notifications.pop_front()
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn take_editor_text(&mut self) -> Option<String> {
        self.editor_text.take()
    }

    pub fn take_diagnostic(&mut self) -> Option<String> {
        self.diagnostics.pop_front()
    }

    pub const fn has_seen_extension_ui(&self) -> bool {
        self.has_seen_extension_ui
    }

    fn push_diagnostic(&mut self, diagnostic: String) {
        const DIAGNOSTIC_LIMIT: usize = 16;
        if self.diagnostics.len() >= DIAGNOSTIC_LIMIT {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic);
    }

    pub fn drain_cancelled_dialogs(&mut self) -> Vec<ExtensionUiResponse> {
        let responses = self
            .dialogs
            .drain(..)
            .map(|dialog| ExtensionUiResponse::cancelled(dialog.id))
            .collect();
        self.pending_ids.clear();
        responses
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub const fn custom_ui_capability(&self) -> &'static str {
        "自定义扩展界面受限"
    }
}

fn sanitize_extension_request(request: ExtensionUiRequest) -> ExtensionUiRequest {
    match request {
        ExtensionUiRequest::Select {
            title,
            options,
            timeout,
        } => ExtensionUiRequest::Select {
            title: sanitize_extension_single_line(&title, EXTENSION_DIALOG_TITLE_LIMIT),
            options,
            timeout,
        },
        ExtensionUiRequest::Confirm {
            title,
            message,
            timeout,
        } => ExtensionUiRequest::Confirm {
            title: sanitize_extension_single_line(&title, EXTENSION_DIALOG_TITLE_LIMIT),
            message: sanitize_extension_text(&message, EXTENSION_DIALOG_MESSAGE_LIMIT),
            timeout,
        },
        ExtensionUiRequest::Input {
            title,
            placeholder,
            timeout,
        } => ExtensionUiRequest::Input {
            title: sanitize_extension_single_line(&title, EXTENSION_DIALOG_TITLE_LIMIT),
            placeholder: placeholder.map(|placeholder| {
                sanitize_extension_single_line(&placeholder, EXTENSION_DIALOG_OPTION_LIMIT)
            }),
            timeout,
        },
        ExtensionUiRequest::Editor { title, prefill } => ExtensionUiRequest::Editor {
            title: sanitize_extension_single_line(&title, EXTENSION_DIALOG_TITLE_LIMIT),
            prefill,
        },
        request => request,
    }
}

fn sanitize_extension_editable(text: &str) -> String {
    text.chars()
        .filter(|character| {
            (*character == '\n' || *character == '\t' || !character.is_control())
                && !is_unicode_format_character(*character)
        })
        .collect()
}

pub fn sanitize_extension_text(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|character| {
            (*character == '\n' || *character == '\t' || !character.is_control())
                && !is_unicode_format_character(*character)
        })
        .take(limit)
        .collect()
}

fn sanitize_extension_single_line(text: &str, limit: usize) -> String {
    let mut result = String::new();
    let mut length = 0;
    let mut pending_space = false;
    for character in text.chars() {
        if character == '\r' || character == '\n' || character == '\t' {
            pending_space = !result.is_empty();
            continue;
        }
        if character.is_control() || is_unicode_format_character(character) {
            continue;
        }
        if pending_space && !character.is_whitespace() && length < limit {
            result.push(' ');
            length += 1;
        }
        pending_space = false;
        if length >= limit {
            break;
        }
        result.push(character);
        length += 1;
    }
    result
}

/// Unicode General_Category=Cf；扩展文本不应能用双向控制或零宽字符伪装 UI。
fn is_unicode_format_character(character: char) -> bool {
    matches!(
        character as u32,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x13455
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerMode {
    Steer,
    FollowUp,
}

impl ComposerMode {
    pub const fn streaming_behavior(self) -> StreamingBehavior {
        match self {
            Self::Steer => StreamingBehavior::Steer,
            Self::FollowUp => StreamingBehavior::FollowUp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcIntent {
    Prompt,
    Steer,
    FollowUp,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolPreset {
    #[default]
    Inherit,
    None,
    ReadOnly,
    Default,
    Full,
}

impl ToolPreset {
    pub const ALL: [Self; 5] = [
        Self::Inherit,
        Self::None,
        Self::ReadOnly,
        Self::Default,
        Self::Full,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Inherit => "跟随 pi",
            Self::None => "关闭",
            Self::ReadOnly => "只读",
            Self::Default => "默认",
            Self::Full => "完整",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Inherit => "沿用 settings.json 的 defaultTools 与扩展工具",
            Self::None => "不启用任何工具（扩展工具也不生效）",
            Self::ReadOnly => "内建 read、grep、find、ls（扩展工具不生效）",
            Self::Default => "内建四件套 read、bash、edit、write（扩展工具不生效）",
            Self::Full => "全部 7 个内建工具（扩展工具不生效）",
        }
    }

    pub const fn tool_names(self) -> &'static [&'static str] {
        match self {
            Self::Inherit | Self::None => &[],
            Self::ReadOnly => &["read", "grep", "find", "ls"],
            Self::Default => &["read", "bash", "edit", "write"],
            Self::Full => &["bash", "read", "edit", "write", "grep", "find", "ls"],
        }
    }

    pub fn append_args(self, args: &mut Vec<std::ffi::OsString>) {
        if self == Self::Inherit {
            return;
        }
        args.push("--tools".into());
        args.push(self.tool_names().join(",").into());
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionControls {
    pub model: Option<Model>,
    pub thinking_level: ThinkingLevel,
    pub models: Vec<Model>,
    pub thinking_levels: Vec<ThinkingLevel>,
    pub session_file: Option<PathBuf>,
    pub session_id: String,
    pub tree: TreeData,
    pub auto_compaction_enabled: bool,
    pub auto_retry_enabled: bool,
    pub is_compacting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOperation {
    Model,
    Thinking,
    Tools,
    Compact,
    AutoCompaction,
    AutoRetry,
    AbortRetry,
    Fork,
    Clone,
    SwitchSession,
    ExportHtml,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlRequest {
    SetModel { provider: String, model_id: String },
    CycleModel,
    SetThinking(ThinkingLevel),
    Compact,
    SetAutoCompaction(bool),
    SetAutoRetry(bool),
    AbortRetry,
    Fork { entry_id: String },
    Clone,
    SwitchSession { path: PathBuf },
    ExportHtml { output_path: PathBuf },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlOutcome {
    Controls(SessionControls),
    Compacted(CompactionResult),
    Forked {
        data: ForkData,
        controls: SessionControls,
    },
    ForkCancelled(ForkData),
    Cloned {
        data: CloneData,
        controls: SessionControls,
    },
    CloneCancelled,
    Switched(SessionControls),
    SwitchCancelled,
    RebindCalibrationFailed {
        operation: ControlOperation,
        message: String,
        fork_data: Option<ForkData>,
    },
    Exported(ExportPathData),
    RetryAborted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRuntimeEvent {
    CompactionStarted,
    CompactionEnded {
        error: Option<String>,
    },
    RetryStarted {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error: String,
    },
    RetryEnded {
        success: bool,
        attempt: u32,
        error: Option<String>,
    },
    AgentEnded {
        will_retry: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComposerSubmission {
    pub message: String,
    pub images: Vec<pi_data::DraftImage>,
}

impl ComposerSubmission {
    fn rpc_images(&self) -> Option<Vec<ImageContent>> {
        (!self.images.is_empty()).then(|| {
            self.images
                .iter()
                .map(|image| ImageContent {
                    kind: ImageKind::Image,
                    data: image.data.clone(),
                    mime_type: image.mime_type.clone(),
                })
                .collect()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestFailureKind {
    Rejected,
    Ambiguous,
}

pub enum PumpMessage {
    Events {
        generation: u64,
        events: Vec<LiveEvent>,
        runtime_events: Vec<SessionRuntimeEvent>,
    },
    ExtensionUiBatch {
        generation: u64,
        requests: Vec<(String, ExtensionUiRequest)>,
    },
    ExtensionUiReset {
        generation: u64,
    },
    RequestFinished {
        generation: u64,
        intent: RpcIntent,
        submission: Option<ComposerSubmission>,
        pending_activity_generation: Option<u64>,
        result: Result<(), (RequestFailureKind, String)>,
    },
    CommandsLoaded {
        generation: u64,
        result: Result<Vec<RpcSlashCommand>, String>,
    },
    ControlsLoaded {
        generation: u64,
        result: Result<SessionControls, String>,
    },
    ControlFinished {
        generation: u64,
        operation: ControlOperation,
        result: Result<ControlOutcome, String>,
    },
    ToolRestartFinished {
        generation: u64,
        preset: ToolPreset,
        result: Result<Box<ActiveSession>, String>,
    },
    Calibrated {
        generation: u64,
        calibration: u64,
        result: Result<ConversationDocument, String>,
    },
    Diagnostic {
        generation: u64,
        message: String,
    },
    Stopped {
        generation: u64,
        error: Option<String>,
    },
}

/// 可注入的响应写回边界。生产实现包装 ActiveSession 的 RPC Client，测试实现不启动子进程。
pub trait ExtensionResponseSender: Send + Sync {
    fn send(&self, response: ExtensionUiResponse) -> Result<(), String>;
}

#[derive(Clone)]
pub struct ClientExtensionResponseSender(Client);

impl ClientExtensionResponseSender {
    pub fn new(client: Client) -> Self {
        Self(client)
    }
}

impl ExtensionResponseSender for ClientExtensionResponseSender {
    fn send(&self, response: ExtensionUiResponse) -> Result<(), String> {
        self.0
            .send_extension_ui_response(&response)
            .map_err(|error| error.to_string())
    }
}

pub struct ActiveSession {
    generation: u64,
    client: Client,
    reducer: LiveSessionReducer,
    pump: UnboundedSender<PumpMessage>,
    calibration_path: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    startup_diagnostic: Option<String>,
    agent_dir: Option<PathBuf>,
}

struct ActiveSessionSpawn {
    generation: u64,
    binary: PathBuf,
    session_path: Option<PathBuf>,
    cwd: PathBuf,
    history: ConversationDocument,
    tool_preset: ToolPreset,
}

impl ActiveSession {
    pub fn spawn_fresh(
        generation: u64,
        binary: PathBuf,
        cwd: PathBuf,
        history: ConversationDocument,
        tool_preset: ToolPreset,
    ) -> Result<(Self, UnboundedReceiver<PumpMessage>), String> {
        let (pump, receiver) = mpsc::unbounded();
        let active = Self::spawn_with_pump(
            ActiveSessionSpawn {
                generation,
                binary,
                session_path: None,
                cwd,
                history,
                tool_preset,
            },
            pump,
            true,
            pi_data::agent_dir(),
        )?;
        if let Some(message) = active.startup_diagnostic.clone() {
            let _ = active.pump.unbounded_send(PumpMessage::Diagnostic {
                generation,
                message,
            });
        }
        Ok((active, receiver))
    }

    pub fn spawn(
        generation: u64,
        binary: PathBuf,
        session_path: PathBuf,
        cwd: PathBuf,
        history: ConversationDocument,
        tool_preset: ToolPreset,
    ) -> Result<(Self, UnboundedReceiver<PumpMessage>), String> {
        Self::spawn_with_agent_dir(
            generation,
            binary,
            session_path,
            cwd,
            history,
            tool_preset,
            pi_data::agent_dir(),
        )
    }

    fn spawn_with_agent_dir(
        generation: u64,
        binary: PathBuf,
        session_path: PathBuf,
        cwd: PathBuf,
        history: ConversationDocument,
        tool_preset: ToolPreset,
        agent_dir: Option<PathBuf>,
    ) -> Result<(Self, UnboundedReceiver<PumpMessage>), String> {
        let (pump, receiver) = mpsc::unbounded();
        let active = Self::spawn_with_pump(
            ActiveSessionSpawn {
                generation,
                binary,
                session_path: Some(session_path),
                cwd,
                history,
                tool_preset,
            },
            pump,
            true,
            agent_dir,
        )?;
        if let Some(message) = active.startup_diagnostic.clone() {
            let _ = active.pump.unbounded_send(PumpMessage::Diagnostic {
                generation,
                message,
            });
        }
        Ok((active, receiver))
    }

    fn spawn_with_pump(
        spawn: ActiveSessionSpawn,
        pump: UnboundedSender<PumpMessage>,
        refresh_metadata: bool,
        agent_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let (mut config, diagnostic) = active_session_config(
            spawn.binary,
            spawn.session_path,
            spawn.cwd,
            spawn.tool_preset,
        );
        if let Some(agent_dir) = agent_dir.as_ref() {
            config.env.push((
                pi_data::AGENT_DIR_ENV.into(),
                agent_dir.as_os_str().to_owned(),
            ));
        }
        let calibration_path =
            std::sync::Arc::new(std::sync::Mutex::new(config.initial_session.clone()));
        let client = Client::spawn(config).map_err(|error| error.to_string())?;
        let events = client.subscribe();
        spawn_event_pump(
            spawn.generation,
            calibration_path.clone(),
            events,
            pump.clone(),
        );
        let active = Self {
            generation: spawn.generation,
            client,
            reducer: LiveSessionReducer::new(spawn.history),
            pump,
            calibration_path,
            startup_diagnostic: diagnostic,
            agent_dir,
        };
        if refresh_metadata {
            active.refresh_metadata();
        }
        Ok(active)
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn client(&self) -> Client {
        self.client.clone()
    }

    pub fn phase(&self) -> LivePhase {
        self.reducer.phase()
    }

    pub fn reducer(&self) -> &LiveSessionReducer {
        &self.reducer
    }

    pub fn reducer_mut(&mut self) -> &mut LiveSessionReducer {
        &mut self.reducer
    }

    pub fn startup_diagnostic(&self) -> Option<&str> {
        self.startup_diagnostic.as_deref()
    }

    pub fn document(&mut self) -> ConversationDocument {
        self.reducer.document()
    }

    pub fn calibrate(&mut self, document: ConversationDocument) {
        self.reducer.calibrate(document);
    }

    /// fresh 会话的身份由 pi 在启动后决定。同步 reducer 与事件泵的校准路径，
    /// 避免下一批 live event 把真实 session id/path 覆盖回临时值。
    pub fn set_session_identity(&mut self, session_id: String, path: PathBuf) {
        set_calibration_path(&self.calibration_path, Some(path.clone()));
        self.reducer.set_session_identity(session_id, path);
    }

    pub fn dispatch(
        &mut self,
        intent: RpcIntent,
        submission: Option<ComposerSubmission>,
        mode: ComposerMode,
        activity_generation: u64,
    ) {
        debug_assert!(
            self.phase() != LivePhase::Stopping,
            "stopping session must reject new RPC intents"
        );
        let pending_activity_generation = (intent != RpcIntent::Abort
            && self.phase() != LivePhase::Running)
            .then_some(activity_generation);
        match intent {
            RpcIntent::Abort => self.reducer.set_stopping(),
            _ => self.reducer.set_running(),
        }
        let command = match intent {
            RpcIntent::Prompt => Command::Prompt {
                message: submission
                    .as_ref()
                    .map(|submission| submission.message.clone())
                    .unwrap_or_default(),
                images: submission.as_ref().and_then(ComposerSubmission::rpc_images),
                streaming_behavior: None,
            },
            RpcIntent::Steer | RpcIntent::FollowUp => Command::Prompt {
                message: submission
                    .as_ref()
                    .map(|submission| submission.message.clone())
                    .unwrap_or_default(),
                images: submission.as_ref().and_then(ComposerSubmission::rpc_images),
                streaming_behavior: Some(mode.streaming_behavior()),
            },
            RpcIntent::Abort => Command::Abort,
        };
        let client = self.client.clone();
        let generation = self.generation;
        let pump = self.pump.clone();
        let timeout = request_timeout(intent);
        thread::Builder::new()
            .name(format!("pi-rpc-request-{generation}"))
            .spawn(move || {
                let result = match client.request(command, timeout) {
                    Ok(response) if response.success => Ok(()),
                    Ok(response) => Err((
                        RequestFailureKind::Rejected,
                        response.error.unwrap_or_else(|| "unknown RPC error".into()),
                    )),
                    Err(error) => Err((RequestFailureKind::Ambiguous, error.to_string())),
                };
                let _ = pump.unbounded_send(PumpMessage::RequestFinished {
                    generation,
                    intent,
                    submission,
                    pending_activity_generation,
                    result,
                });
            })
            .expect("failed to spawn RPC request thread");
    }

    pub fn refresh_metadata(&self) {
        spawn_commands_request(self.generation, self.client.clone(), self.pump.clone());
        spawn_controls_request(
            self.generation,
            self.client.clone(),
            self.pump.clone(),
            self.agent_dir.clone(),
        );
    }

    pub fn request_control(&self, operation: ControlOperation, request: ControlRequest) {
        let generation = self.generation;
        let client = self.client.clone();
        let pump = self.pump.clone();
        let agent_dir = self.agent_dir.clone();
        thread::Builder::new()
            .name(format!("pi-rpc-control-{generation}"))
            .spawn(move || {
                let result = execute_control(&client, request, agent_dir.as_deref());
                let _ = pump.unbounded_send(PumpMessage::ControlFinished {
                    generation,
                    operation,
                    result,
                });
            })
            .expect("failed to spawn RPC control thread");
    }

    pub fn restart_with_tools(
        self,
        generation: u64,
        binary: PathBuf,
        session_path: Option<PathBuf>,
        cwd: PathBuf,
        history: ConversationDocument,
        preset: ToolPreset,
    ) {
        let pump = self.pump.clone();
        let agent_dir = self.agent_dir.clone();
        thread::Builder::new()
            .name(format!("pi-rpc-tool-restart-{generation}"))
            .spawn(move || {
                let shutdown = self.client.shutdown().map_err(|error| error.to_string());
                drop(self);
                let result = shutdown.and_then(|()| {
                    Self::spawn_with_pump(
                        ActiveSessionSpawn {
                            generation,
                            binary,
                            session_path,
                            cwd,
                            history,
                            tool_preset: preset,
                        },
                        pump.clone(),
                        false,
                        agent_dir,
                    )
                    .map(Box::new)
                });
                let diagnostic = result
                    .as_ref()
                    .ok()
                    .and_then(|active| active.startup_diagnostic.clone());
                let _ = pump.unbounded_send(PumpMessage::ToolRestartFinished {
                    generation,
                    preset,
                    result,
                });
                if let Some(message) = diagnostic {
                    let _ = pump.unbounded_send(PumpMessage::Diagnostic {
                        generation,
                        message,
                    });
                }
            })
            .expect("failed to spawn tool restart thread");
    }

    pub fn shutdown(self) {
        let generation = self.generation;
        let pump = self.pump.clone();
        thread::Builder::new()
            .name(format!("pi-rpc-shutdown-{generation}"))
            .spawn(move || {
                let _ = self.client.shutdown();
                let _ = pump.unbounded_send(PumpMessage::Stopped {
                    generation,
                    error: None,
                });
            })
            .expect("failed to spawn RPC shutdown thread");
    }
}

const fn request_timeout(intent: RpcIntent) -> Duration {
    match intent {
        RpcIntent::Prompt | RpcIntent::Steer | RpcIntent::FollowUp => INTERACTIVE_REQUEST_TIMEOUT,
        RpcIntent::Abort => REQUEST_TIMEOUT,
    }
}

fn spawn_controls_request(
    generation: u64,
    client: Client,
    pump: UnboundedSender<PumpMessage>,
    agent_dir: Option<PathBuf>,
) {
    thread::Builder::new()
        .name(format!("pi-rpc-controls-{generation}"))
        .spawn(move || {
            let result = load_controls(&client, agent_dir.as_deref());
            let _ = pump.unbounded_send(PumpMessage::ControlsLoaded { generation, result });
        })
        .expect("failed to spawn RPC controls thread");
}

pub(crate) fn load_controls(
    client: &Client,
    agent_dir: Option<&Path>,
) -> Result<SessionControls, String> {
    let state = client
        .request_data::<RpcSessionState>(Command::GetState, REQUEST_TIMEOUT)
        .map_err(|error| error.to_string())?;
    load_controls_from_state(client, state, agent_dir)
}

fn load_controls_from_state(
    client: &Client,
    state: RpcSessionState,
    agent_dir: Option<&Path>,
) -> Result<SessionControls, String> {
    let mut models = client
        .request_data::<AvailableModelsData>(Command::GetAvailableModels, REQUEST_TIMEOUT)
        .map_err(|error| error.to_string())?
        .models;
    models.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.id.cmp(&right.id))
    });
    let thinking_levels = client
        .request_data::<ThinkingLevelsData>(Command::GetAvailableThinkingLevels, REQUEST_TIMEOUT)
        .map_err(|error| error.to_string())?
        .levels;
    let tree = client
        .request_data::<TreeData>(Command::GetTree, REQUEST_TIMEOUT)
        .map_err(|error| error.to_string())?;
    let auto_retry_enabled = agent_dir
        .map(pi_data::read_auto_retry_enabled)
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(true);
    Ok(SessionControls {
        model: state.model,
        thinking_level: state.thinking_level,
        models,
        thinking_levels,
        session_file: state.session_file.map(PathBuf::from),
        session_id: state.session_id,
        tree,
        auto_compaction_enabled: state.auto_compaction_enabled,
        auto_retry_enabled,
        is_compacting: state.is_compacting,
    })
}

pub(crate) fn execute_control(
    client: &Client,
    request: ControlRequest,
    agent_dir: Option<&Path>,
) -> Result<ControlOutcome, String> {
    match request {
        ControlRequest::SetModel { provider, model_id } => {
            client
                .request_data::<Model>(Command::SetModel { provider, model_id }, REQUEST_TIMEOUT)
                .map_err(|error| error.to_string())?;
        }
        ControlRequest::CycleModel => {
            let response = client
                .request(Command::CycleModel, REQUEST_TIMEOUT)
                .map_err(|error| error.to_string())?;
            if !response.success {
                return Err(response.error.unwrap_or_else(|| "unknown RPC error".into()));
            }
        }
        ControlRequest::SetThinking(level) => {
            let response = client
                .request(Command::SetThinkingLevel { level }, REQUEST_TIMEOUT)
                .map_err(|error| error.to_string())?;
            if !response.success {
                return Err(response.error.unwrap_or_else(|| "unknown RPC error".into()));
            }
        }
        ControlRequest::Compact => {
            let result = client
                .request_data::<CompactionResult>(
                    Command::Compact {
                        custom_instructions: None,
                    },
                    Duration::from_secs(300),
                )
                .map_err(|error| error.to_string())?;
            return Ok(ControlOutcome::Compacted(result));
        }
        ControlRequest::SetAutoCompaction(enabled) => {
            ensure_success(client, Command::SetAutoCompaction { enabled })?;
        }
        ControlRequest::SetAutoRetry(enabled) => {
            ensure_success(client, Command::SetAutoRetry { enabled })?;
        }
        ControlRequest::AbortRetry => {
            ensure_success(client, Command::AbortRetry)?;
            return Ok(ControlOutcome::RetryAborted);
        }
        ControlRequest::Fork { entry_id } => {
            let outcome = client
                .request_session_rebind_data::<ForkData>(
                    Command::Fork { entry_id },
                    REQUEST_TIMEOUT,
                )
                .map_err(|error| error.to_string())?;
            if outcome.data.cancelled {
                return Ok(ControlOutcome::ForkCancelled(outcome.data));
            }
            let data = outcome.data;
            let state = match calibrated_state("fork", outcome.calibration) {
                Ok(state) => state,
                Err(message) => {
                    return Ok(ControlOutcome::RebindCalibrationFailed {
                        operation: ControlOperation::Fork,
                        message,
                        fork_data: Some(data),
                    });
                }
            };
            let controls = match load_controls_from_state(client, state, agent_dir) {
                Ok(controls) => controls,
                Err(error) => {
                    return Ok(ControlOutcome::RebindCalibrationFailed {
                        operation: ControlOperation::Fork,
                        message: format!(
                            "fork 已成功，但会话控制元数据刷新失败；请勿重复操作：{error}"
                        ),
                        fork_data: Some(data),
                    });
                }
            };
            return Ok(ControlOutcome::Forked { data, controls });
        }
        ControlRequest::Clone => {
            let outcome = client
                .request_session_rebind_data::<CloneData>(Command::Clone, REQUEST_TIMEOUT)
                .map_err(|error| error.to_string())?;
            if outcome.data.cancelled {
                return Ok(ControlOutcome::CloneCancelled);
            }
            let state = match calibrated_state("clone", outcome.calibration) {
                Ok(state) => state,
                Err(message) => {
                    return Ok(ControlOutcome::RebindCalibrationFailed {
                        operation: ControlOperation::Clone,
                        message,
                        fork_data: None,
                    });
                }
            };
            let controls = match load_controls_from_state(client, state, agent_dir) {
                Ok(controls) => controls,
                Err(error) => {
                    return Ok(ControlOutcome::RebindCalibrationFailed {
                        operation: ControlOperation::Clone,
                        message: format!(
                            "clone 已成功，但会话控制元数据刷新失败；请勿重复操作：{error}"
                        ),
                        fork_data: None,
                    });
                }
            };
            return Ok(ControlOutcome::Cloned {
                data: outcome.data,
                controls,
            });
        }
        ControlRequest::SwitchSession { path } => {
            let outcome = client
                .request_session_rebind_data::<pi_rpc::SwitchSessionData>(
                    Command::SwitchSession {
                        session_path: path.to_string_lossy().into_owned(),
                    },
                    REQUEST_TIMEOUT,
                )
                .map_err(|error| error.to_string())?;
            if outcome.data.cancelled {
                return Ok(ControlOutcome::SwitchCancelled);
            }
            let state = match calibrated_state("switch_session", outcome.calibration) {
                Ok(state) => state,
                Err(message) => {
                    return Ok(ControlOutcome::RebindCalibrationFailed {
                        operation: ControlOperation::SwitchSession,
                        message,
                        fork_data: None,
                    });
                }
            };
            return Ok(match load_controls_from_state(client, state, agent_dir) {
                Ok(controls) => ControlOutcome::Switched(controls),
                Err(error) => ControlOutcome::RebindCalibrationFailed {
                    operation: ControlOperation::SwitchSession,
                    message: format!(
                        "switch_session 已成功，但会话控制元数据刷新失败；请勿重复操作：{error}"
                    ),
                    fork_data: None,
                },
            });
        }
        ControlRequest::ExportHtml { output_path } => {
            let data = client
                .request_data::<ExportPathData>(
                    Command::ExportHtml {
                        output_path: Some(output_path.to_string_lossy().into_owned()),
                    },
                    Duration::from_secs(60),
                )
                .map_err(|error| error.to_string())?;
            return Ok(ControlOutcome::Exported(data));
        }
    }
    load_controls(client, agent_dir).map(ControlOutcome::Controls)
}

fn calibrated_state(
    operation: &str,
    calibration: Option<Result<RpcSessionState, pi_rpc::ClientError>>,
) -> Result<RpcSessionState, String> {
    calibration
        .ok_or_else(|| format!("{operation} 已取消"))?
        .map_err(|error| format!("{operation} 已成功，但会话元数据校准失败；请勿重复操作：{error}"))
}

fn ensure_success(client: &Client, command: Command) -> Result<(), String> {
    let response = client
        .request(command, REQUEST_TIMEOUT)
        .map_err(|error| error.to_string())?;
    if response.success {
        Ok(())
    } else {
        Err(response.error.unwrap_or_else(|| "unknown RPC error".into()))
    }
}

fn spawn_commands_request(generation: u64, client: Client, pump: UnboundedSender<PumpMessage>) {
    thread::Builder::new()
        .name(format!("pi-rpc-commands-{generation}"))
        .spawn(move || {
            let result = client
                .request_data::<CommandsData>(Command::GetCommands, REQUEST_TIMEOUT)
                .map(|mut data| {
                    data.commands.sort_by(|left, right| {
                        slash_source_order(left.source)
                            .cmp(&slash_source_order(right.source))
                            .then_with(|| left.name.cmp(&right.name))
                    });
                    data.commands
                })
                .map_err(|error| error.to_string());
            let _ = pump.unbounded_send(PumpMessage::CommandsLoaded { generation, result });
        })
        .expect("failed to spawn RPC commands thread");
}

const fn slash_source_order(source: pi_rpc::SlashCommandSource) -> u8 {
    match source {
        pi_rpc::SlashCommandSource::Extension => 0,
        pi_rpc::SlashCommandSource::Prompt => 1,
        pi_rpc::SlashCommandSource::Skill => 2,
    }
}

fn active_session_config(
    binary: PathBuf,
    session_path: Option<PathBuf>,
    cwd: PathBuf,
    tool_preset: ToolPreset,
) -> (ClientConfig, Option<String>) {
    active_session_config_with_materializer(binary, session_path, cwd, tool_preset, || {
        pi_rpc::materialize_host_extension()
    })
}

fn active_session_config_with_materializer(
    binary: PathBuf,
    session_path: Option<PathBuf>,
    cwd: PathBuf,
    tool_preset: ToolPreset,
    materialize: impl FnOnce() -> std::io::Result<PathBuf>,
) -> (ClientConfig, Option<String>) {
    let mut config = ClientConfig::new(binary);
    config.current_dir = Some(cwd);
    config.initial_session = session_path;
    config.args = vec!["--no-context-files".into()];
    let diagnostic = match materialize() {
        Ok(host_extension) => {
            config
                .args
                .extend(["-e".into(), host_extension.into_os_string()]);
            None
        }
        Err(error) => Some(format!("项目命令环境扩展未加载：{error}")),
    };
    tool_preset.append_args(&mut config.args);
    (config, diagnostic)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalHtmlExport {
    pub path: PathBuf,
    pub cleanup_warning: Option<String>,
}

fn finish_historical_export(
    export: Result<PathBuf, String>,
    shutdown: Result<(), String>,
) -> Result<HistoricalHtmlExport, String> {
    match export {
        Ok(path) => Ok(HistoricalHtmlExport {
            path,
            cleanup_warning: shutdown.err(),
        }),
        Err(error) => {
            // shutdown 仍已尝试；主导出失败优先展示，清理失败附带保留可观测性。
            Err(match shutdown {
                Ok(()) => error,
                Err(shutdown_error) => format!("{error}；进程清理也失败：{shutdown_error}"),
            })
        }
    }
}

pub fn export_historical_html(
    session_path: PathBuf,
    output_path: PathBuf,
) -> Result<HistoricalHtmlExport, String> {
    let cwd = pi_data::load_session(&session_path)
        .map(|session| PathBuf::from(session.header.cwd))
        .map_err(|error| error.to_string())?;
    let mut config = ClientConfig::new(official_binary());
    config.current_dir = Some(cwd);
    config.initial_session = Some(session_path);
    config.args = vec![
        "--no-extensions".into(),
        "--no-skills".into(),
        "--no-prompt-templates".into(),
        "--no-context-files".into(),
        "--offline".into(),
    ];
    let client = Client::spawn(config).map_err(|error| error.to_string())?;
    let result = client
        .request_data::<ExportPathData>(
            Command::ExportHtml {
                output_path: Some(output_path.to_string_lossy().into_owned()),
            },
            Duration::from_secs(60),
        )
        .map(|data| PathBuf::from(data.path))
        .map_err(|error| error.to_string());
    let shutdown = client.shutdown().map_err(|error| error.to_string());
    finish_historical_export(result.map(|_| output_path), shutdown)
}

pub fn official_binary() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("vendor")
        .join("pi")
        .join(pi_rpc::pi_binary_name())
}

fn set_calibration_path(
    slot: &std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    path: Option<PathBuf>,
) {
    *slot.lock().unwrap() = path;
}

fn spawn_event_pump(
    generation: u64,
    calibration_path: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    events: Receiver<ClientEvent>,
    pump: UnboundedSender<PumpMessage>,
) {
    thread::Builder::new()
        .name(format!("pi-rpc-event-pump-{generation}"))
        .spawn(move || {
            let mut activity_generation = 0_u64;
            loop {
                let first = match events.recv() {
                    Ok(event) => event,
                    Err(_) => {
                        let _ = pump.unbounded_send(PumpMessage::Stopped {
                            generation,
                            error: Some("pi RPC 事件泵意外停止".to_owned()),
                        });
                        return;
                    }
                };
                let mut batch = Vec::with_capacity(64);
                let mut runtime_events = Vec::new();
                let mut extension_requests = Vec::new();
                let mut extension_reset = false;
                let mut settled = false;
                project_pump_event(
                    first,
                    &mut batch,
                    &mut runtime_events,
                    &mut extension_requests,
                    &mut extension_reset,
                    &mut activity_generation,
                    &mut settled,
                );
                let deadline = Instant::now() + PUMP_FRAME;
                let mut disconnected = false;
                while batch.len() < MAX_EVENTS_PER_BATCH {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    match events.recv_timeout(deadline.saturating_duration_since(now)) {
                        Ok(event) => project_pump_event(
                            event,
                            &mut batch,
                            &mut runtime_events,
                            &mut extension_requests,
                            &mut extension_reset,
                            &mut activity_generation,
                            &mut settled,
                        ),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                if extension_reset
                    && pump
                        .unbounded_send(PumpMessage::ExtensionUiReset { generation })
                        .is_err()
                {
                    return;
                }
                if !extension_requests.is_empty()
                    && pump
                        .unbounded_send(PumpMessage::ExtensionUiBatch {
                            generation,
                            requests: coalesce_extension_ui_requests(extension_requests),
                        })
                        .is_err()
                {
                    return;
                }
                if (!batch.is_empty() || !runtime_events.is_empty())
                    && pump
                        .unbounded_send(PumpMessage::Events {
                            generation,
                            events: batch,
                            runtime_events,
                        })
                        .is_err()
                {
                    return;
                }
                if settled && let Some(session_path) = calibration_path.lock().unwrap().clone() {
                    spawn_calibration(generation, activity_generation, session_path, &pump);
                }
                if disconnected {
                    let _ = pump.unbounded_send(PumpMessage::Stopped {
                        generation,
                        error: Some("pi RPC 事件泵意外停止".to_owned()),
                    });
                    return;
                }
            }
        })
        .expect("failed to spawn RPC event pump");
}

fn project_pump_event(
    event: ClientEvent,
    batch: &mut Vec<LiveEvent>,
    runtime_events: &mut Vec<SessionRuntimeEvent>,
    extension_requests: &mut Vec<(String, ExtensionUiRequest)>,
    extension_reset: &mut bool,
    activity_generation: &mut u64,
    settled: &mut bool,
) {
    if let ClientEvent::Rpc(event) = &event {
        match event.as_ref() {
            RpcEvent::ExtensionUiRequest { .. } => {}
            RpcEvent::AgentEnd { will_retry, .. } => {
                runtime_events.push(SessionRuntimeEvent::AgentEnded {
                    will_retry: *will_retry,
                })
            }
            RpcEvent::CompactionStart { .. } => {
                runtime_events.push(SessionRuntimeEvent::CompactionStarted)
            }
            RpcEvent::CompactionEnd { error_message, .. } => {
                runtime_events.push(SessionRuntimeEvent::CompactionEnded {
                    error: error_message.clone(),
                });
                // 手动 compact 不保证另发 agent_settled；结束事件同样触发文件校准。
                *settled = true;
            }
            RpcEvent::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
                error_message,
            } => runtime_events.push(SessionRuntimeEvent::RetryStarted {
                attempt: *attempt,
                max_attempts: *max_attempts,
                delay_ms: *delay_ms,
                error: error_message.clone(),
            }),
            RpcEvent::AutoRetryEnd {
                success,
                attempt,
                final_error,
            } => runtime_events.push(SessionRuntimeEvent::RetryEnded {
                success: *success,
                attempt: *attempt,
                error: final_error.clone(),
            }),
            _ => {}
        }
    }

    match event {
        ClientEvent::Rpc(event) => match *event {
            RpcEvent::ExtensionUiRequest { id, request } => {
                extension_requests.push((id, request));
            }
            event => {
                if let Some(event) = project_rpc_event(event) {
                    if matches!(event, LiveEvent::AgentStart) {
                        *activity_generation = activity_generation.wrapping_add(1);
                    }
                    *settled |= matches!(event, LiveEvent::AgentSettled);
                    batch.push(event);
                }
            }
        },
        ClientEvent::Lifecycle(pi_rpc::LifecycleEvent::Restarted { .. }) => {
            *extension_reset = true;
            extension_requests.clear();
        }
        event => {
            if let Some(event) = project_event(event) {
                batch.push(event);
            }
        }
    }
}

fn coalesce_extension_ui_requests(
    requests: Vec<(String, ExtensionUiRequest)>,
) -> Vec<(String, ExtensionUiRequest)> {
    let mut result = Vec::<(String, ExtensionUiRequest)>::new();
    let mut coalesced = std::collections::HashMap::<(u8, String), usize>::new();
    for (id, request) in requests {
        let key = match &request {
            ExtensionUiRequest::SetStatus { status_key, .. } => Some((0, status_key.clone())),
            ExtensionUiRequest::SetWidget { widget_key, .. } => Some((1, widget_key.clone())),
            _ => None,
        };
        if let Some(key) = key {
            if let Some(index) = coalesced.get(&key).copied() {
                result[index] = (id, request);
            } else {
                coalesced.insert(key, result.len());
                result.push((id, request));
            }
        } else {
            result.push((id, request));
        }
    }
    result
}

fn spawn_calibration(
    generation: u64,
    calibration: u64,
    session_path: PathBuf,
    pump: &UnboundedSender<PumpMessage>,
) {
    let pump = pump.clone();
    thread::Builder::new()
        .name(format!("pi-session-calibration-{generation}"))
        .spawn(move || {
            // pi 在 settled 前完成会话 append；重读只发生在后台，UI 不等待文件 IO。
            let result = pi_render::render_path(session_path).map_err(|error| error.to_string());
            let _ = pump.unbounded_send(PumpMessage::Calibrated {
                generation,
                calibration,
                result,
            });
        })
        .expect("failed to spawn session calibration thread");
}

fn project_event(event: ClientEvent) -> Option<LiveEvent> {
    match event {
        ClientEvent::Rpc(event) => project_rpc_event(*event),
        ClientEvent::Unknown(value) => Some(LiveEvent::Diagnostic(format!(
            "未识别的 pi RPC 事件：{value}"
        ))),
        ClientEvent::Lifecycle(event) => match event {
            pi_rpc::LifecycleEvent::RestartFailed { error } => Some(LiveEvent::Diagnostic(error)),
            _ => None,
        },
    }
}

fn project_rpc_event(event: RpcEvent) -> Option<LiveEvent> {
    match event {
        RpcEvent::AgentStart => Some(LiveEvent::AgentStart),
        RpcEvent::AgentEnd { .. } => Some(LiveEvent::AgentEnd),
        RpcEvent::AgentSettled => Some(LiveEvent::AgentSettled),
        RpcEvent::MessageStart { message } => Some(LiveEvent::MessageStart { message: message.0 }),
        RpcEvent::MessageUpdate {
            assistant_message_event,
            ..
        } => Some(LiveEvent::MessageUpdate(project_update(
            assistant_message_event,
        ))),
        RpcEvent::MessageEnd { message } => Some(LiveEvent::MessageEnd { message: message.0 }),
        RpcEvent::ToolExecutionStart {
            tool_call_id,
            tool_name,
            args,
        } => Some(LiveEvent::ToolExecutionStart {
            id: tool_call_id,
            name: tool_name,
            arguments: args,
        }),
        RpcEvent::ToolExecutionUpdate {
            tool_call_id,
            tool_name,
            args,
            partial_result,
        } => Some(LiveEvent::ToolExecutionUpdate {
            id: tool_call_id,
            name: tool_name,
            arguments: args,
            partial_result,
        }),
        RpcEvent::ToolExecutionEnd {
            tool_call_id,
            tool_name,
            result,
            is_error,
        } => Some(LiveEvent::ToolExecutionEnd {
            id: tool_call_id,
            name: tool_name,
            result,
            is_error,
        }),
        RpcEvent::QueueUpdate {
            steering,
            follow_up,
        } => Some(LiveEvent::QueueUpdate {
            steering,
            follow_up,
        }),
        _ => None,
    }
}

fn project_update(event: AssistantMessageEvent) -> LiveAssistantUpdate {
    match event {
        AssistantMessageEvent::Start => LiveAssistantUpdate::Start,
        AssistantMessageEvent::TextStart { content_index } => LiveAssistantUpdate::BlockStart {
            index: content_index,
            kind: LiveBlockKind::Text,
        },
        AssistantMessageEvent::TextDelta {
            content_index,
            delta,
        } => LiveAssistantUpdate::BlockDelta {
            index: content_index,
            kind: LiveBlockKind::Text,
            delta,
        },
        AssistantMessageEvent::TextEnd {
            content_index,
            content,
        } => LiveAssistantUpdate::BlockEnd {
            index: content_index,
            kind: LiveBlockKind::Text,
            content: content.into(),
        },
        AssistantMessageEvent::ThinkingStart { content_index } => LiveAssistantUpdate::BlockStart {
            index: content_index,
            kind: LiveBlockKind::Thinking,
        },
        AssistantMessageEvent::ThinkingDelta {
            content_index,
            delta,
        } => LiveAssistantUpdate::BlockDelta {
            index: content_index,
            kind: LiveBlockKind::Thinking,
            delta,
        },
        AssistantMessageEvent::ThinkingEnd {
            content_index,
            content,
        } => LiveAssistantUpdate::BlockEnd {
            index: content_index,
            kind: LiveBlockKind::Thinking,
            content: content.into(),
        },
        AssistantMessageEvent::ToolcallStart { content_index } => LiveAssistantUpdate::BlockStart {
            index: content_index,
            kind: LiveBlockKind::ToolCall,
        },
        AssistantMessageEvent::ToolcallDelta {
            content_index,
            delta,
        } => LiveAssistantUpdate::BlockDelta {
            index: content_index,
            kind: LiveBlockKind::ToolCall,
            delta,
        },
        AssistantMessageEvent::ToolcallEnd {
            content_index,
            tool_call,
        } => LiveAssistantUpdate::BlockEnd {
            index: content_index,
            kind: LiveBlockKind::ToolCall,
            content: tool_call,
        },
        AssistantMessageEvent::Done { .. } => LiveAssistantUpdate::Done,
        AssistantMessageEvent::Error { reason, error } => LiveAssistantUpdate::Error {
            message: format!("assistant stream {reason:?}: {}", error.0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires GPUI_PI_TEST_FAKE_CHILD=target/debug/fake_child.exe"]
    fn session_controls_and_switches_use_typed_rpc_state() {
        let binary = std::env::var_os("GPUI_PI_TEST_FAKE_CHILD")
            .map(PathBuf::from)
            .expect("GPUI_PI_TEST_FAKE_CHILD must point to pi-rpc fake_child");
        let agent_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            agent_dir.path().join("settings.json"),
            r#"{"retry":{"enabled":false}}"#,
        )
        .unwrap();
        let mut config = ClientConfig::new(binary);
        config.env.push((
            pi_data::AGENT_DIR_ENV.into(),
            agent_dir.path().as_os_str().to_owned(),
        ));
        let client = Client::spawn(config).unwrap();
        let controls = load_controls(&client, Some(agent_dir.path())).unwrap();
        assert_eq!(controls.models.len(), 2);
        assert!(!controls.auto_retry_enabled);
        assert_eq!(controls.model.as_ref().unwrap().id, "model-one");
        assert_eq!(
            controls.thinking_levels,
            [ThinkingLevel::Off, ThinkingLevel::Low, ThinkingLevel::High,]
        );

        let controls =
            execute_control(&client, ControlRequest::CycleModel, Some(agent_dir.path())).unwrap();
        let ControlOutcome::Controls(controls) = controls else {
            panic!("expected controls")
        };
        assert_eq!(controls.model.as_ref().unwrap().id, "model-two");
        let controls = execute_control(
            &client,
            ControlRequest::SetThinking(ThinkingLevel::High),
            Some(agent_dir.path()),
        )
        .unwrap();
        let ControlOutcome::Controls(controls) = controls else {
            panic!("expected controls")
        };
        assert_eq!(controls.thinking_level, ThinkingLevel::High);
        let controls = execute_control(
            &client,
            ControlRequest::SetModel {
                provider: "provider-one".to_owned(),
                model_id: "model-one".to_owned(),
            },
            Some(agent_dir.path()),
        )
        .unwrap();
        let ControlOutcome::Controls(controls) = controls else {
            panic!("expected controls")
        };
        assert_eq!(controls.model.as_ref().unwrap().id, "model-one");
        client.shutdown().unwrap();
    }

    #[test]
    fn historical_export_success_survives_shutdown_failure_with_warning() {
        let output = PathBuf::from("exported.html");
        let result =
            finish_historical_export(Ok(output.clone()), Err("shutdown timed out".to_owned()))
                .unwrap();
        assert_eq!(result.path, output);
        assert_eq!(
            result.cleanup_warning.as_deref(),
            Some("shutdown timed out")
        );

        let error = finish_historical_export(
            Err("export failed".to_owned()),
            Err("shutdown failed".to_owned()),
        )
        .unwrap_err();
        assert!(error.contains("export failed"));
        assert!(error.contains("shutdown failed"));
    }

    #[test]
    fn active_session_config_always_loads_host_extension_without_changing_tool_presets() {
        let expected = [
            (ToolPreset::Inherit, None),
            (ToolPreset::None, Some("")),
            (ToolPreset::ReadOnly, Some("read,grep,find,ls")),
            (ToolPreset::Default, Some("read,bash,edit,write")),
            (ToolPreset::Full, Some("bash,read,edit,write,grep,find,ls")),
        ];
        for (preset, allowlist) in expected {
            let (config, diagnostic) = active_session_config(
                PathBuf::from("pi.exe"),
                Some(PathBuf::from("session.jsonl")),
                PathBuf::from("project"),
                preset,
            );
            assert!(diagnostic.is_none());
            assert_eq!(config.args[0], "--no-context-files");
            assert_eq!(config.args[1], "-e");
            let extension_path = Path::new(&config.args[2]);
            assert!(extension_path.is_file());
            assert_eq!(
                extension_path.file_name().and_then(|name| name.to_str()),
                Some("project-command-environment.ts")
            );
            match allowlist {
                Some(allowlist) => assert_eq!(
                    &config.args[3..],
                    &["--tools", allowlist].map(std::ffi::OsString::from)
                ),
                None => assert_eq!(config.args.len(), 3),
            }
        }
    }

    #[test]
    fn fresh_active_session_config_has_no_initial_session() {
        let (config, diagnostic) = active_session_config_with_materializer(
            PathBuf::from("pi.exe"),
            None,
            PathBuf::from("project"),
            ToolPreset::Inherit,
            || Ok(PathBuf::from("host.ts")),
        );
        assert!(diagnostic.is_none());
        assert_eq!(config.current_dir.as_deref(), Some(Path::new("project")));
        assert_eq!(config.initial_session, None);
    }

    #[test]
    fn calibration_path_helper_updates_the_event_pump_slot() {
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        set_calibration_path(&slot, Some(PathBuf::from("real.jsonl")));
        assert_eq!(
            slot.lock().unwrap().as_deref(),
            Some(Path::new("real.jsonl"))
        );
        set_calibration_path(&slot, None);
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn active_session_config_degrades_without_extension_and_reports_diagnostic() {
        let (config, diagnostic) = active_session_config_with_materializer(
            PathBuf::from("pi.exe"),
            Some(PathBuf::from("session.jsonl")),
            PathBuf::from("project"),
            ToolPreset::ReadOnly,
            || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "denied",
                ))
            },
        );
        assert_eq!(
            config.args,
            ["--no-context-files", "--tools", "read,grep,find,ls"].map(std::ffi::OsString::from)
        );
        assert_eq!(
            diagnostic.as_deref(),
            Some("项目命令环境扩展未加载：denied")
        );
    }

    #[test]
    fn streaming_intents_use_atomic_prompt_behavior() {
        assert_eq!(
            ComposerMode::Steer.streaming_behavior(),
            StreamingBehavior::Steer
        );
        assert_eq!(
            ComposerMode::FollowUp.streaming_behavior(),
            StreamingBehavior::FollowUp
        );
    }

    #[test]
    fn human_operated_submissions_outlive_the_short_rpc_timeout() {
        for intent in [RpcIntent::Prompt, RpcIntent::Steer, RpcIntent::FollowUp] {
            assert!(request_timeout(intent) > Duration::from_secs(30));
            assert_eq!(request_timeout(intent), INTERACTIVE_REQUEST_TIMEOUT);
        }
        assert_eq!(request_timeout(RpcIntent::Abort), REQUEST_TIMEOUT);
    }

    #[test]
    fn restarted_discards_only_pre_restart_extension_requests_in_same_frame() {
        let mut batch = Vec::new();
        let mut runtime_events = Vec::new();
        let mut extension_requests = Vec::new();
        let mut extension_reset = false;
        let mut activity_generation = 0;
        let mut settled = false;
        for event in [
            ClientEvent::Rpc(Box::new(RpcEvent::ExtensionUiRequest {
                id: "before".into(),
                request: ExtensionUiRequest::Notify {
                    message: "before".into(),
                    notify_type: None,
                },
            })),
            ClientEvent::Lifecycle(pi_rpc::LifecycleEvent::Restarted {
                pid: 42,
                session_file: None,
            }),
            ClientEvent::Rpc(Box::new(RpcEvent::ExtensionUiRequest {
                id: "after-1".into(),
                request: ExtensionUiRequest::SetStatus {
                    status_key: "key".into(),
                    status_text: Some("one".into()),
                },
            })),
            ClientEvent::Rpc(Box::new(RpcEvent::ExtensionUiRequest {
                id: "after-2".into(),
                request: ExtensionUiRequest::SetStatus {
                    status_key: "key".into(),
                    status_text: Some("two".into()),
                },
            })),
        ] {
            project_pump_event(
                event,
                &mut batch,
                &mut runtime_events,
                &mut extension_requests,
                &mut extension_reset,
                &mut activity_generation,
                &mut settled,
            );
        }
        assert!(extension_reset);
        let coalesced = coalesce_extension_ui_requests(extension_requests);
        assert_eq!(coalesced.len(), 1);
        assert_eq!(coalesced[0].0, "after-2");
    }

    #[test]
    fn extension_ui_request_sanitization_limits_dialogs_statuses_and_widgets() {
        let mut state = ExtensionUiState::default();
        let raw_option = format!("family 👨‍👩‍👧‍👦\tvalue\u{202e}{}", "x".repeat(300));
        state.apply(
            "select".into(),
            ExtensionUiRequest::Select {
                title: format!("title\nspoof\u{202e}\u{200b}\u{0}{}", "x".repeat(200)),
                options: vec![raw_option.clone()],
                timeout: Some(10),
            },
        );
        let dialog = state.active_dialog().unwrap();
        let ExtensionUiRequest::Select { title, options, .. } = &dialog.request else {
            panic!("select request expected");
        };
        assert!(!title.chars().any(char::is_control));
        assert!(!title.contains('\u{202e}'));
        assert!(!title.contains('\u{200b}'));
        assert!(title.starts_with("title spoof"));
        assert_eq!(title.chars().count(), EXTENSION_DIALOG_TITLE_LIMIT);
        assert!(options.is_empty(), "raw option 不得进入 render request");
        let option = &dialog.select_options.as_ref().unwrap()[0];
        assert_eq!(option.raw, raw_option);
        assert!(!option.display.contains('\u{202e}'));
        assert!(!option.display.contains('\t'));
        assert!(option.display.chars().count() <= EXTENSION_DIALOG_OPTION_LIMIT);
        for index in 0..(EXTENSION_STATUS_COUNT_LIMIT + 4) {
            state.apply(
                format!("status-{index}"),
                ExtensionUiRequest::SetStatus {
                    status_key: format!("key-{index}"),
                    status_text: Some("x".repeat(EXTENSION_STATUS_LIMIT + 20)),
                },
            );
        }
        assert_eq!(state.statuses().count(), EXTENSION_STATUS_COUNT_LIMIT);
        for index in 0..(EXTENSION_WIDGET_COUNT_LIMIT + 4) {
            state.apply(
                format!("widget-{index}"),
                ExtensionUiRequest::SetWidget {
                    widget_key: format!("key-{index}"),
                    widget_lines: Some(vec!["line".into(); EXTENSION_WIDGET_LINES_LIMIT + 5]),
                    widget_placement: None,
                },
            );
        }
        assert_eq!(state.widgets.len(), EXTENSION_WIDGET_COUNT_LIMIT);
        assert!(
            state
                .widgets
                .values()
                .all(|widget| widget.lines.len() == EXTENSION_WIDGET_LINES_LIMIT)
        );
    }

    #[test]
    fn extension_ui_bounds_dialogs_and_notifications_without_hanging() {
        let mut state = ExtensionUiState::default();
        for index in 0..EXTENSION_DIALOG_QUEUE_LIMIT {
            assert!(
                state
                    .apply(
                        format!("dialog-{index}"),
                        ExtensionUiRequest::Confirm {
                            title: "Confirm".into(),
                            message: "Continue?".into(),
                            timeout: None,
                        },
                    )
                    .is_none()
            );
        }
        assert_eq!(state.dialogs.len(), EXTENSION_DIALOG_QUEUE_LIMIT);
        assert_eq!(
            state.apply(
                "overflow".into(),
                ExtensionUiRequest::Editor {
                    title: "Editor".into(),
                    prefill: None,
                },
            ),
            Some(ExtensionUiResponse::cancelled("overflow"))
        );
        assert_eq!(
            state.apply(
                "empty".into(),
                ExtensionUiRequest::Select {
                    title: "Empty".into(),
                    options: Vec::new(),
                    timeout: None,
                },
            ),
            Some(ExtensionUiResponse::cancelled("empty"))
        );
        assert_eq!(
            state.apply(
                "too-many-options".into(),
                ExtensionUiRequest::Select {
                    title: "Too many".into(),
                    options: vec!["value".into(); EXTENSION_DIALOG_OPTIONS_LIMIT + 1],
                    timeout: None,
                },
            ),
            Some(ExtensionUiResponse::cancelled("too-many-options"))
        );
        assert!(
            state
                .take_diagnostic()
                .is_some_and(|diagnostic| diagnostic.contains("超过上限"))
        );
        assert_eq!(
            state.apply(
                "dialog-0".into(),
                ExtensionUiRequest::Select {
                    title: "Duplicate".into(),
                    options: Vec::new(),
                    timeout: None,
                },
            ),
            None,
            "重复 id 的空 Select 不得发送第二个 cancelled response"
        );
        for index in 0..(EXTENSION_NOTIFICATION_QUEUE_LIMIT + 3) {
            state.apply(
                format!("notify-{index}"),
                ExtensionUiRequest::Notify {
                    message: index.to_string(),
                    notify_type: None,
                },
            );
        }
        assert_eq!(
            state.notifications.len(),
            EXTENSION_NOTIFICATION_QUEUE_LIMIT
        );
        assert_eq!(state.notifications.front().unwrap().message, "3");
        assert_eq!(
            state.drain_cancelled_dialogs().len(),
            EXTENSION_DIALOG_QUEUE_LIMIT
        );
        assert!(state.active_dialog().is_none());
    }

    #[test]
    fn extension_editable_payloads_are_not_silently_truncated() {
        let mut state = ExtensionUiState::default();
        let raw = format!("line\nwith\ttab\u{0}\u{202e}{}", "x".repeat(5000));
        let expected = sanitize_extension_editable(&raw);
        assert!(expected.len() > EXTENSION_TEXT_LIMIT);
        assert_eq!(
            state.apply(
                "editor".into(),
                ExtensionUiRequest::Editor {
                    title: "Editor".into(),
                    prefill: Some(raw.clone()),
                },
            ),
            None
        );
        let ExtensionUiRequest::Editor {
            prefill: Some(prefill),
            ..
        } = &state.active_dialog().unwrap().request
        else {
            panic!("editor expected");
        };
        assert_eq!(prefill, &expected);
        state.finish_dialog("editor");
        state.apply(
            "set-editor".into(),
            ExtensionUiRequest::SetEditorText { text: raw },
        );
        assert_eq!(state.take_editor_text().as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn status_and_widget_raw_keys_do_not_collide_after_display_sanitization() {
        let mut state = ExtensionUiState::default();
        for key in ["same", "sa\u{200b}me"] {
            state.apply(
                format!("status-{key}"),
                ExtensionUiRequest::SetStatus {
                    status_key: key.into(),
                    status_text: Some(key.into()),
                },
            );
            state.apply(
                format!("widget-{key}"),
                ExtensionUiRequest::SetWidget {
                    widget_key: key.into(),
                    widget_lines: Some(vec!["line".into()]),
                    widget_placement: None,
                },
            );
        }
        assert_eq!(state.statuses().count(), 2);
        assert_eq!(state.widgets.len(), 2);
        state.apply(
            "remove-status".into(),
            ExtensionUiRequest::SetStatus {
                status_key: "sa\u{200b}me".into(),
                status_text: None,
            },
        );
        assert_eq!(state.statuses().count(), 1);
        assert_eq!(state.statuses().next().unwrap().raw_key, "same");
    }

    #[test]
    fn extension_ui_coalescing_keeps_first_key_position_and_latest_value() {
        let requests = vec![
            (
                "status-1".into(),
                ExtensionUiRequest::SetStatus {
                    status_key: "status".into(),
                    status_text: Some("one".into()),
                },
            ),
            (
                "notify".into(),
                ExtensionUiRequest::Notify {
                    message: "middle".into(),
                    notify_type: None,
                },
            ),
            (
                "status-2".into(),
                ExtensionUiRequest::SetStatus {
                    status_key: "status".into(),
                    status_text: Some("two".into()),
                },
            ),
        ];
        let coalesced = coalesce_extension_ui_requests(requests);
        assert_eq!(coalesced.len(), 2);
        assert_eq!(coalesced[0].0, "status-2");
        assert!(matches!(
            &coalesced[0].1,
            ExtensionUiRequest::SetStatus {
                status_text: Some(text),
                ..
            } if text == "two"
        ));
        assert!(matches!(coalesced[1].1, ExtensionUiRequest::Notify { .. }));
    }

    #[test]
    fn extension_ui_state_upserts_sanitizes_and_resets() {
        let mut state = ExtensionUiState::default();
        state.apply(
            "status-2".into(),
            ExtensionUiRequest::SetStatus {
                status_key: "z".into(),
                status_text: Some("run\u{1b}[31m".into()),
            },
        );
        state.apply(
            "status-1".into(),
            ExtensionUiRequest::SetStatus {
                status_key: "a".into(),
                status_text: Some("ready".into()),
            },
        );
        assert_eq!(
            state
                .statuses()
                .map(|status| (status.raw_key.as_str(), status.text.as_str()))
                .collect::<Vec<_>>(),
            [("a", "ready"), ("z", "run[31m")]
        );
        state.apply(
            "widget".into(),
            ExtensionUiRequest::SetWidget {
                widget_key: "fixture".into(),
                widget_lines: Some(vec!["line\u{0}".into()]),
                widget_placement: None,
            },
        );
        assert_eq!(
            state
                .widgets(WidgetPlacement::AboveEditor)
                .next()
                .unwrap()
                .lines,
            ["line"]
        );
        state.apply(
            "dialog".into(),
            ExtensionUiRequest::Confirm {
                title: "Confirm".into(),
                message: "Continue?".into(),
                timeout: None,
            },
        );
        assert_eq!(state.active_dialog().unwrap().id, "dialog");
        assert!(!state.finish_dialog("stale"));
        assert!(state.finish_dialog("dialog"));
        state.reset();
        assert!(state.statuses().next().is_none());
        assert!(state.active_dialog().is_none());
    }

    #[test]
    fn projects_agent_end_and_settled_separately() {
        assert_eq!(
            project_event(ClientEvent::Rpc(Box::new(RpcEvent::AgentEnd {
                messages: Vec::new(),
                will_retry: false,
            }))),
            Some(LiveEvent::AgentEnd)
        );
        assert_eq!(
            project_event(ClientEvent::Rpc(Box::new(RpcEvent::AgentSettled))),
            Some(LiveEvent::AgentSettled)
        );
    }
}
