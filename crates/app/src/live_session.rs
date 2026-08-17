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
    AssistantMessageEvent, Client, ClientConfig, ClientEvent, Command, CommandsData, ImageContent,
    ImageKind, RpcEvent, RpcSlashCommand, StreamingBehavior,
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

#[derive(Debug)]
pub enum PumpMessage {
    Events {
        generation: u64,
        events: Vec<LiveEvent>,
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
}

impl ActiveSession {
    pub fn spawn(
        generation: u64,
        binary: PathBuf,
        session_path: PathBuf,
        cwd: PathBuf,
        history: ConversationDocument,
    ) -> Result<(Self, UnboundedReceiver<PumpMessage>), String> {
        let mut config = ClientConfig::new(binary);
        config.current_dir = Some(cwd);
        config.initial_session = Some(session_path);
        config.args = vec!["--no-context-files".into()];
        let session_path = config
            .initial_session
            .clone()
            .expect("active session requires an initial path");
        let client = Client::spawn(config).map_err(|error| error.to_string())?;
        let events = client.subscribe();
        let (pump, receiver) = mpsc::unbounded();
        spawn_event_pump(generation, session_path, events, pump.clone());
        spawn_commands_request(generation, client.clone(), pump.clone());
        Ok((
            Self {
                generation,
                client,
                reducer: LiveSessionReducer::new(history),
                pump,
            },
            receiver,
        ))
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
                let mut settled = false;
                if let Some(event) = project_event(first) {
                    if matches!(event, LiveEvent::AgentStart) {
                        activity_generation = activity_generation.wrapping_add(1);
                    }
                    settled |= matches!(event, LiveEvent::AgentSettled);
                    batch.push(event);
                }
                let deadline = Instant::now() + PUMP_FRAME;
                let mut disconnected = false;
                while batch.len() < MAX_EVENTS_PER_BATCH {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    match events.recv_timeout(deadline.saturating_duration_since(now)) {
                        Ok(event) => {
                            if let Some(event) = project_event(event) {
                                if matches!(event, LiveEvent::AgentStart) {
                                    activity_generation = activity_generation.wrapping_add(1);
                                }
                                settled |= matches!(event, LiveEvent::AgentSettled);
                                batch.push(event);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }
                if !batch.is_empty()
                    && pump
                        .unbounded_send(PumpMessage::Events {
                            generation,
                            events: batch,
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
