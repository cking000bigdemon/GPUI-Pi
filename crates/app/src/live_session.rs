use std::{
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
    Command, CommandsData, CompactionResult, ExportPathData, ForkData, ImageContent, ImageKind,
    Model, RpcEvent, RpcSessionState, RpcSlashCommand, StreamingBehavior, ThinkingLevel,
    ThinkingLevelsData, TreeData,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const PUMP_FRAME: Duration = Duration::from_millis(20);
const MAX_EVENTS_PER_BATCH: usize = 512;

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
    Stopped {
        generation: u64,
        error: Option<String>,
    },
}

pub struct ActiveSession {
    generation: u64,
    client: Client,
    reducer: LiveSessionReducer,
    pump: UnboundedSender<PumpMessage>,
    agent_dir: Option<PathBuf>,
}

struct ActiveSessionSpawn {
    generation: u64,
    binary: PathBuf,
    session_path: PathBuf,
    cwd: PathBuf,
    history: ConversationDocument,
    tool_preset: ToolPreset,
}

impl ActiveSession {
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
                session_path,
                cwd,
                history,
                tool_preset,
            },
            pump,
            true,
            agent_dir,
        )?;
        Ok((active, receiver))
    }

    fn spawn_with_pump(
        spawn: ActiveSessionSpawn,
        pump: UnboundedSender<PumpMessage>,
        refresh_metadata: bool,
        agent_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let mut config = ClientConfig::new(spawn.binary);
        config.current_dir = Some(spawn.cwd);
        config.initial_session = Some(spawn.session_path);
        config.args = vec!["--no-context-files".into()];
        if let Some(agent_dir) = agent_dir.as_ref() {
            config.env.push((
                pi_data::AGENT_DIR_ENV.into(),
                agent_dir.as_os_str().to_owned(),
            ));
        }
        spawn.tool_preset.append_args(&mut config.args);
        let session_path = config
            .initial_session
            .clone()
            .expect("active session requires an initial path");
        let client = Client::spawn(config).map_err(|error| error.to_string())?;
        let events = client.subscribe();
        spawn_event_pump(spawn.generation, session_path, events, pump.clone());
        let active = Self {
            generation: spawn.generation,
            client,
            reducer: LiveSessionReducer::new(spawn.history),
            pump,
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

    pub fn phase(&self) -> LivePhase {
        self.reducer.phase()
    }

    pub fn reducer(&self) -> &LiveSessionReducer {
        &self.reducer
    }

    pub fn reducer_mut(&mut self) -> &mut LiveSessionReducer {
        &mut self.reducer
    }

    pub fn document(&mut self) -> ConversationDocument {
        self.reducer.document()
    }

    pub fn calibrate(&mut self, document: ConversationDocument) {
        self.reducer.calibrate(document);
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
        thread::Builder::new()
            .name(format!("pi-rpc-request-{generation}"))
            .spawn(move || {
                let result = match client.request(command, REQUEST_TIMEOUT) {
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
        session_path: PathBuf,
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
                let _ = pump.unbounded_send(PumpMessage::ToolRestartFinished {
                    generation,
                    preset,
                    result,
                });
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

fn spawn_event_pump(
    generation: u64,
    session_path: PathBuf,
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
                let mut settled = false;
                project_client_event(
                    first,
                    &mut batch,
                    &mut runtime_events,
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
                        Ok(event) => project_client_event(
                            event,
                            &mut batch,
                            &mut runtime_events,
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
                if settled {
                    spawn_calibration(generation, activity_generation, session_path.clone(), &pump);
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

fn project_client_event(
    event: ClientEvent,
    live_events: &mut Vec<LiveEvent>,
    runtime_events: &mut Vec<SessionRuntimeEvent>,
    activity_generation: &mut u64,
    settled: &mut bool,
) {
    if let ClientEvent::Rpc(event) = &event {
        match event.as_ref() {
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
            } => {
                runtime_events.push(SessionRuntimeEvent::RetryStarted {
                    attempt: *attempt,
                    max_attempts: *max_attempts,
                    delay_ms: *delay_ms,
                    error: error_message.clone(),
                });
            }
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
    if let Some(live) = project_event(event) {
        if matches!(live, LiveEvent::AgentStart) {
            *activity_generation = activity_generation.wrapping_add(1);
        }
        *settled |= matches!(live, LiveEvent::AgentSettled);
        live_events.push(live);
    }
}

fn project_event(event: ClientEvent) -> Option<LiveEvent> {
    match event {
        ClientEvent::Rpc(event) => match *event {
            RpcEvent::AgentStart => Some(LiveEvent::AgentStart),
            RpcEvent::AgentEnd { .. } => Some(LiveEvent::AgentEnd),
            RpcEvent::AgentSettled => Some(LiveEvent::AgentSettled),
            RpcEvent::MessageStart { message } => {
                Some(LiveEvent::MessageStart { message: message.0 })
            }
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
        },
        ClientEvent::Unknown(value) => Some(LiveEvent::Diagnostic(format!(
            "未识别的 pi RPC 事件：{value}"
        ))),
        ClientEvent::Lifecycle(event) => match event {
            pi_rpc::LifecycleEvent::RestartFailed { error } => Some(LiveEvent::Diagnostic(error)),
            _ => None,
        },
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
    fn tool_presets_generate_explicit_allowlists() {
        let expected = [
            (ToolPreset::Inherit, None),
            (ToolPreset::None, Some("")),
            (ToolPreset::ReadOnly, Some("read,grep,find,ls")),
            (ToolPreset::Default, Some("read,bash,edit,write")),
            (ToolPreset::Full, Some("bash,read,edit,write,grep,find,ls")),
        ];
        for (preset, allowlist) in expected {
            let mut args = Vec::new();
            preset.append_args(&mut args);
            match allowlist {
                Some(allowlist) => {
                    assert_eq!(args, ["--tools", allowlist].map(std::ffi::OsString::from));
                }
                None => assert!(args.is_empty()),
            }
        }
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
