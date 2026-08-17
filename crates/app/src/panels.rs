use std::{collections::HashSet, path::PathBuf, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use futures::{StreamExt as _, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, AppContext as _, ClipboardEntry, Context, EventEmitter, ExternalPaths, FocusHandle,
    Focusable, FollowMode, Image, ImageFormat, InteractiveElement as _, IntoElement, KeyDownEvent,
    ListAlignment, ListState, ParentElement as _, PathPromptOptions, Render, SharedString,
    Styled as _, Subscription, Window, div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, Icon, IconName, Selectable as _,
    Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    scroll::ScrollableElement as _,
    v_flex,
};
use pi_render::{ConversationDocument, ConversationItem, LivePhase};

use crate::{
    live_session::{
        ActiveSession, ComposerMode, ComposerSubmission, PumpMessage, RequestFailureKind,
        RpcIntent, official_binary,
    },
    session_sidebar::SessionSelected,
};

pub struct ChatPanel {
    focus_handle: FocusHandle,
    status: ChatStatus,
    load_generation: u64,
    active: Option<ActiveSession>,
    composer: gpui::Entity<TextareaState>,
    composer_mode: ComposerMode,
    draft_key: Option<String>,
    drafts: pi_data::DraftStore,
    attachments: Vec<ComposerAttachment>,
    slash_commands: Vec<pi_rpc::RpcSlashCommand>,
    popup: Option<ComposerPopup>,
    popup_index: usize,
    file_index: Option<pi_data::FileIndex>,
    composer_cwd: Option<PathBuf>,
    pending_draft_restore: bool,
    list_state: ListState,
    list_item_ids: Vec<String>,
    tail_attached: bool,
    follow_requested: bool,
    minimap_visible: bool,
    expanded_tools: HashSet<String>,
    expanded_processes: HashSet<String>,
    rpc_error: Option<String>,
    activity_generation: u64,
    calibration_generation: u64,
    _composer_subscription: Subscription,
    probe: Option<LayoutProbe>,
}

#[derive(Debug, Clone)]
struct ComposerAttachment {
    draft: pi_data::DraftImage,
    preview: Arc<Image>,
}

#[derive(Debug, Clone)]
enum ComposerPopup {
    Slash(Vec<pi_rpc::RpcSlashCommand>),
    At {
        query: pi_data::AtQuery,
        entries: Vec<pi_data::FileIndexEntry>,
    },
}

#[derive(Debug, Clone)]
pub enum ChatStatus {
    Empty,
    Loading { title: String },
    Ready(Arc<ConversationDocument>),
    Error { title: String, message: String },
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct LayoutProbe {
    pub sidebar: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    pub sidebar_prepaints: std::rc::Rc<std::cell::Cell<usize>>,
    pub workspace: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}

#[cfg(not(test))]
#[derive(Clone, Copy)]
pub(crate) struct LayoutProbe;

impl LayoutProbe {
    #[cfg(test)]
    pub(crate) fn record_sidebar(&self, bounds: gpui::Bounds<gpui::Pixels>) {
        self.sidebar.set(bounds);
        self.sidebar_prepaints
            .set(self.sidebar_prepaints.get().saturating_add(1));
    }

    #[cfg(not(test))]
    pub(crate) fn record_sidebar(&self, _: gpui::Bounds<gpui::Pixels>) {}

    #[cfg(test)]
    fn record_workspace(&self, bounds: gpui::Bounds<gpui::Pixels>) {
        self.workspace.set(bounds);
    }

    #[cfg(not(test))]
    fn record_workspace(&self, _: gpui::Bounds<gpui::Pixels>) {}
}

impl ChatPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 5)
                .submit_on_enter(true)
                .placeholder("输入消息；Enter 发送，Shift+Enter 换行")
        });
        let subscription =
            cx.subscribe_in(
                &composer,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::PressEnter { shift: false, .. } => {
                        if !this.accept_popup(input, window, cx) {
                            this.submit_composer(input, window, cx);
                        }
                    }
                    InputEvent::Change => this.composer_changed(input, cx),
                    _ => {}
                },
            );
        let list_state = ListState::new(0, ListAlignment::Top, px(1200.));
        let scroll_state = list_state.clone();
        let panel = cx.weak_entity();
        list_state.set_scroll_handler(move |event, _, cx| {
            let attached = event.is_following_tail;
            let scroll_state = scroll_state.clone();
            let panel = panel.clone();
            // ListState 在回调时持有可变借用；延后读取/更新，避免 RefCell 重入。
            cx.defer(move |cx| {
                let attached = attached || scroll_state.is_scrolled_to_end().unwrap_or(true);
                let _ = panel.update(cx, |panel, cx| {
                    if panel.tail_attached != attached {
                        panel.tail_attached = attached;
                        cx.notify();
                    }
                });
            });
        });
        Self {
            focus_handle: cx.focus_handle(),
            status: ChatStatus::Empty,
            load_generation: 0,
            active: None,
            composer,
            composer_mode: ComposerMode::Steer,
            draft_key: None,
            drafts: pi_data::DraftStore::default(),
            attachments: Vec::new(),
            slash_commands: Vec::new(),
            popup: None,
            popup_index: 0,
            file_index: None,
            composer_cwd: None,
            pending_draft_restore: false,
            list_state,
            list_item_ids: Vec::new(),
            tail_attached: true,
            follow_requested: false,
            minimap_visible: true,
            expanded_tools: HashSet::new(),
            expanded_processes: HashSet::new(),
            rpc_error: None,
            activity_generation: 0,
            calibration_generation: 0,
            _composer_subscription: subscription,
            probe: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_probe(mut self, probe: LayoutProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    fn current_draft(&self, cx: &App) -> pi_data::ComposerDraft {
        pi_data::ComposerDraft {
            text: self.composer.read(cx).value().to_string(),
            images: self
                .attachments
                .iter()
                .map(|attachment| attachment.draft.clone())
                .collect(),
        }
    }

    fn save_current_draft(&mut self, cx: &App) {
        if let Some(key) = self.draft_key.clone() {
            self.drafts.set(key, self.current_draft(cx));
        }
    }

    fn prepare_draft_restore(&mut self) {
        let draft = self
            .draft_key
            .as_deref()
            .map(|key| self.drafts.get(key))
            .unwrap_or_default();
        self.pending_draft_restore = true;
        self.attachments = draft
            .images
            .into_iter()
            .filter_map(attachment_from_draft)
            .collect();
    }

    fn start_file_index(&self, generation: u64, cwd: PathBuf, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |panel, cx| {
            let result = executor
                .spawn(async move { pi_data::build_file_index(&cwd) })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if generation == panel.load_generation {
                    panel.file_index = Some(result);
                    panel.refresh_popup(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn load_selection(&mut self, selection: SessionSelected, cx: &mut Context<Self>) {
        self.save_current_draft(cx);
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;
        if let Some(active) = self.active.take() {
            active.shutdown();
        }
        self.status = ChatStatus::Loading {
            title: selection.title.clone(),
        };
        self.rpc_error = None;
        self.draft_key = Some(selection.id.clone());
        self.composer_cwd = Some(selection.cwd.clone());
        self.file_index = None;
        self.slash_commands.clear();
        self.popup = None;
        self.popup_index = 0;
        self.prepare_draft_restore();
        self.start_file_index(generation, selection.cwd.clone(), cx);
        self.activity_generation = 0;
        self.calibration_generation = 0;
        self.tail_attached = true;
        self.follow_requested = false;
        self.minimap_visible = true;
        self.expanded_tools.clear();
        self.expanded_processes.clear();
        self.list_state = ListState::new(0, ListAlignment::Top, px(1200.));
        let scroll_state = self.list_state.clone();
        let panel = cx.weak_entity();
        self.list_state.set_scroll_handler(move |event, _, cx| {
            let attached = event.is_following_tail;
            let scroll_state = scroll_state.clone();
            let panel = panel.clone();
            cx.defer(move |cx| {
                let attached = attached || scroll_state.is_scrolled_to_end().unwrap_or(true);
                let _ = panel.update(cx, |panel, cx| {
                    if panel.tail_attached != attached {
                        panel.tail_attached = attached;
                        cx.notify();
                    }
                });
            });
        });
        self.list_item_ids.clear();
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |panel, cx| {
            let path = selection.path;
            let title = selection.title;
            let result = executor
                .spawn(async move {
                    pi_render::render_path(&path)
                        .map(Arc::new)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.finish_load(generation, title, result) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn finish_load(
        &mut self,
        generation: u64,
        title: String,
        result: Result<Arc<ConversationDocument>, String>,
    ) -> bool {
        if generation != self.load_generation {
            return false;
        }
        self.status = match result {
            Ok(document) => {
                self.sync_list_document(&document);
                self.list_state.scroll_to_end();
                ChatStatus::Ready(document)
            }
            Err(message) => ChatStatus::Error { title, message },
        };
        true
    }

    fn start_live(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.active.is_some() {
            return;
        }
        let ChatStatus::Ready(history) = &self.status else {
            return;
        };
        let generation = self.load_generation;
        let history = history.clone();
        let session_path = history.source_path.clone();
        let cwd = session_cwd(&session_path).unwrap_or_else(|| PathBuf::from("."));
        let result = ActiveSession::spawn(
            generation,
            official_binary(),
            session_path,
            cwd,
            (*history).clone(),
        );
        match result {
            Ok((active, receiver)) => {
                self.active = Some(active);
                self.rpc_error = None;
                self.spawn_pump(receiver, cx);
            }
            Err(error) => self.rpc_error = Some(error),
        }
        cx.notify();
    }

    fn spawn_pump(&self, mut receiver: UnboundedReceiver<PumpMessage>, cx: &mut Context<Self>) {
        cx.spawn(async move |panel, cx| {
            while let Some(message) = receiver.next().await {
                let stopped = matches!(message, PumpMessage::Stopped { .. });
                let _ = panel.update(cx, |panel, cx| {
                    panel.handle_pump(message);
                    cx.notify();
                });
                if stopped {
                    return;
                }
            }
        })
        .detach();
    }

    fn handle_pump(&mut self, message: PumpMessage) {
        let generation = match &message {
            PumpMessage::Events { generation, .. }
            | PumpMessage::RequestFinished { generation, .. }
            | PumpMessage::CommandsLoaded { generation, .. }
            | PumpMessage::Calibrated { generation, .. }
            | PumpMessage::Stopped { generation, .. } => *generation,
        };
        if generation != self.load_generation {
            return;
        }
        if let PumpMessage::Stopped { error, .. } = &message
            && self.active.is_none()
        {
            if let Some(error) = error {
                self.rpc_error = Some(error.clone());
            }
            return;
        }
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if generation != active.generation() {
            return;
        }
        match message {
            PumpMessage::Events { events, .. } => {
                if events
                    .iter()
                    .any(|event| matches!(event, pi_render::LiveEvent::AgentStart))
                {
                    self.activity_generation = self.activity_generation.wrapping_add(1);
                }
                let outcome = active.reducer_mut().apply_batch(events);
                if outcome.follow_tail && self.tail_attached {
                    // 一个 batch 对应最多一次滚动请求，不随 token 数增长。
                    self.follow_requested = true;
                }
                let document = Arc::new(active.document());
                self.sync_list_document(&document);
                self.status = ChatStatus::Ready(document);
            }
            PumpMessage::RequestFinished {
                intent,
                submission,
                pending_activity_generation,
                result,
                ..
            } => match result {
                Ok(()) => {
                    self.rpc_error = None;
                }
                Err((kind, error)) => {
                    if intent == RpcIntent::Abort {
                        active.reducer_mut().restore_running_if_stopping();
                    } else if should_restore_idle_phase(
                        pending_activity_generation,
                        self.activity_generation,
                        active.phase(),
                    ) {
                        active.reducer_mut().restore_phase(LivePhase::Idle);
                    }
                    if should_restore_submission(kind)
                        && let (Some(key), Some(submission)) =
                            (self.draft_key.as_deref(), submission)
                    {
                        let restored = self.drafts.restore_submission(
                            key,
                            pi_data::ComposerDraft {
                                text: submission.message,
                                images: submission.images,
                            },
                        );
                        self.pending_draft_restore = true;
                        self.attachments = restored
                            .images
                            .into_iter()
                            .filter_map(attachment_from_draft)
                            .collect();
                    }
                    self.rpc_error = Some(match kind {
                        RequestFailureKind::Rejected => {
                            format!("pi 明确拒绝提交，已恢复草稿：{error}")
                        }
                        RequestFailureKind::Ambiguous => {
                            format!("提交结果不明确，为避免重复 turn 未自动恢复：{error}")
                        }
                    });
                }
            },
            PumpMessage::CommandsLoaded { result, .. } => match result {
                Ok(commands) => {
                    self.slash_commands = commands;
                    self.refresh_popup_without_input();
                }
                Err(error) => self.rpc_error = Some(format!("加载 slash 命令失败：{error}")),
            },
            PumpMessage::Calibrated {
                calibration,
                result,
                ..
            } => {
                if calibration < self.calibration_generation {
                    return;
                }
                self.calibration_generation = calibration;
                // 校准线程属于发起它的 settled 状态；若其间已有新 run 开始，
                // 旧文件快照不能覆盖正在流式的草稿。
                if active.phase() != LivePhase::Idle || self.activity_generation != calibration {
                    return;
                }
                match result {
                    Ok(document) => {
                        active.calibrate(document);
                        let document = Arc::new(active.document());
                        self.sync_list_document(&document);
                        self.status = ChatStatus::Ready(document);
                    }
                    Err(error) => self.rpc_error = Some(format!("会话落盘校准失败：{error}")),
                }
            }
            PumpMessage::Stopped { error, .. } => {
                if let Some(error) = error {
                    self.rpc_error = Some(error);
                }
                self.active = None;
            }
        }
    }

    fn composer_changed(&mut self, input: &gpui::Entity<TextareaState>, cx: &mut Context<Self>) {
        if self.pending_draft_restore {
            self.pending_draft_restore = false;
        }
        if let Some(key) = self.draft_key.clone() {
            self.drafts.set(key, self.current_draft(cx));
        }
        let input = input.read(cx);
        self.refresh_popup_for_value(input.value().as_ref(), input.cursor());
        cx.notify();
    }

    fn refresh_popup(&mut self, cx: &App) {
        let input = self.composer.read(cx);
        self.refresh_popup_for_value(input.value().as_ref(), input.cursor());
    }

    fn refresh_popup_without_input(&mut self) {
        if let Some(ComposerPopup::Slash(_)) = self.popup {
            self.popup = Some(ComposerPopup::Slash(self.filtered_slash_commands("")));
        }
    }

    fn refresh_popup_for_value(&mut self, value: &str, cursor: usize) {
        let cursor = cursor.min(value.len());
        if cursor == value.len()
            && let Some(query) = slash_query(value)
        {
            self.popup = Some(ComposerPopup::Slash(self.filtered_slash_commands(query)));
            self.popup_index = 0;
            return;
        }
        if self.composer_cwd.is_some()
            && let Some(query) = pi_data::extract_at_query(&value[..cursor])
        {
            let entries = self.file_index.as_ref().map_or_else(Vec::new, |index| {
                pi_data::filter_file_entries(&index.entries, &query.query, pi_data::AT_RESULT_LIMIT)
            });
            self.popup = Some(ComposerPopup::At { query, entries });
            self.popup_index = 0;
            return;
        }
        self.popup = None;
        self.popup_index = 0;
    }

    fn filtered_slash_commands(&self, query: &str) -> Vec<pi_rpc::RpcSlashCommand> {
        let query = query.to_lowercase();
        self.slash_commands
            .iter()
            .filter(|command| {
                command.name.to_lowercase().contains(&query)
                    || command
                        .description
                        .as_deref()
                        .is_some_and(|description| description.to_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    fn accept_popup(
        &mut self,
        input: &gpui::Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(popup) = self.popup.clone() else {
            return false;
        };
        let (value, cursor) = {
            let state = input.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let (next, next_cursor) = match popup {
            ComposerPopup::Slash(commands) => commands.get(self.popup_index).map(|command| {
                let next = format!("/{} ", command.name);
                let cursor = next.len();
                (next, cursor)
            }),
            ComposerPopup::At { query, entries } => entries
                .get(self.popup_index)
                .map(|entry| pi_data::apply_at_insertion(&value, cursor, &query, entry)),
        }
        .unwrap_or_else(|| (String::new(), 0));
        if next.is_empty() {
            return false;
        }
        input.update(cx, |input, cx| {
            input.set_value(next, window, cx);
            input.set_selected_range(next_cursor..next_cursor, cx);
        });
        let state = input.read(cx);
        self.refresh_popup_for_value(state.value().as_ref(), state.cursor());
        true
    }

    fn composer_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.secondary()
            && event.keystroke.key.eq_ignore_ascii_case("v")
            && self.attach_clipboard_images(cx)
        {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let Some(popup) = &self.popup else {
            return;
        };
        let len = match popup {
            ComposerPopup::Slash(commands) => commands.len(),
            ComposerPopup::At { entries, .. } => entries.len(),
        };
        match event.keystroke.key.as_str() {
            "up" => {
                self.popup_index = self.popup_index.saturating_sub(1);
                cx.stop_propagation();
                cx.notify();
            }
            "down" => {
                if len > 0 {
                    self.popup_index = (self.popup_index + 1).min(len - 1);
                }
                cx.stop_propagation();
                cx.notify();
            }
            "tab" => {
                let input = self.composer.clone();
                self.accept_popup(&input, window, cx);
                cx.stop_propagation();
                cx.notify();
            }
            "escape" => {
                self.popup = None;
                self.popup_index = 0;
                cx.stop_propagation();
                cx.notify();
            }
            _ => {}
        }
    }

    fn submit_composer(
        &mut self,
        input: &gpui::Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = input.read(cx).value().trim().to_owned();
        if message.is_empty() && self.attachments.is_empty() {
            return;
        }
        let Some(active) = self.active.as_mut() else {
            self.rpc_error = Some("请先启动活会话".to_owned());
            cx.notify();
            return;
        };
        let intent = match active.phase() {
            LivePhase::Stopping => {
                self.rpc_error = Some("正在停止，暂不能发送消息".to_owned());
                cx.notify();
                return;
            }
            LivePhase::Running => match self.composer_mode {
                ComposerMode::Steer => RpcIntent::Steer,
                ComposerMode::FollowUp => RpcIntent::FollowUp,
            },
            LivePhase::Idle | LivePhase::Error => RpcIntent::Prompt,
        };
        let submission = build_submission(message, &self.attachments);
        active.dispatch(
            intent,
            Some(submission),
            self.composer_mode,
            self.activity_generation,
        );
        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.attachments.clear();
        self.popup = None;
        if let Some(key) = self.draft_key.as_deref() {
            self.drafts.clear(key);
        }
        cx.notify();
    }

    fn choose_images(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择图片附件".into()),
        });
        cx.spawn_in(window, async move |panel, cx| {
            let Some(paths) = receiver.await.ok().and_then(Result::ok).flatten() else {
                return;
            };
            let _ = cx.update(|_, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.start_attach_paths(paths, cx);
                });
            });
        })
        .detach();
    }

    fn start_attach_paths(&self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let generation = self.load_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            std::fs::read(&path)
                                .map_err(|error| format!("{}：{error}", path.display()))
                                .and_then(|bytes| {
                                    pi_data::image_from_bytes(bytes)
                                        .map_err(|error| format!("{}：{error}", path.display()))
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if generation != panel.load_generation {
                    return;
                }
                match result {
                    Ok(images) => panel.add_draft_images(images, cx),
                    Err(error) => panel.rpc_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn add_draft_images(&mut self, images: Vec<pi_data::DraftImage>, cx: &mut Context<Self>) {
        if let Err(error) = pi_data::validate_image_batch(self.attachments.len(), &images) {
            self.rpc_error = Some(error.to_string());
            return;
        }
        self.attachments
            .extend(images.into_iter().filter_map(attachment_from_draft));
        if let Some(key) = self.draft_key.clone() {
            self.drafts.set(key, self.current_draft(cx));
        }
        self.rpc_error = None;
    }

    fn attach_clipboard_images(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let images = item
            .into_entries()
            .filter_map(|entry| match entry {
                ClipboardEntry::Image(image) => gpui_image_to_draft(image),
                _ => None,
            })
            .collect::<Vec<_>>();
        if images.is_empty() {
            return false;
        }
        self.add_draft_images(images, cx);
        true
    }

    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            if let Some(key) = self.draft_key.clone() {
                self.drafts.set(key, self.current_draft(cx));
            }
            cx.notify();
        }
    }

    fn abort(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(active) = self.active.as_mut()
            && active.phase() == LivePhase::Running
        {
            active.dispatch(
                RpcIntent::Abort,
                None,
                self.composer_mode,
                self.activity_generation,
            );
            cx.notify();
        }
    }

    fn select_steer(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.composer_mode = ComposerMode::Steer;
        cx.notify();
    }

    fn select_follow_up(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.composer_mode = ComposerMode::FollowUp;
        cx.notify();
    }

    fn resume_follow(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.tail_attached = true;
        self.follow_requested = false;
        self.list_state.scroll_to_end();
        self.list_state.set_follow_mode(FollowMode::Tail);
        window.refresh();
        cx.notify();
    }

    fn sync_list_document(&mut self, document: &ConversationDocument) {
        let next_ids = document
            .items
            .iter()
            .map(|item| item.id().to_owned())
            .collect::<Vec<_>>();
        let old_len = self.list_item_ids.len();
        let shared_prefix = self
            .list_item_ids
            .iter()
            .zip(&next_ids)
            .take_while(|(old, new)| old == new)
            .count();
        if old_len == 0 {
            self.list_state.reset(next_ids.len());
        } else if shared_prefix == old_len && next_ids.len() >= old_len {
            if next_ids.len() > old_len {
                self.list_state
                    .splice(old_len..old_len, next_ids.len() - old_len);
            } else if old_len > 0
                && self
                    .active
                    .as_ref()
                    .is_some_and(|active| active.phase() != LivePhase::Idle)
            {
                // 只有流式草稿会改变尾部高度；静态重绘不应反复废弃布局缓存。
                self.list_state.remeasure_items(old_len - 1..old_len);
            }
        } else {
            self.list_state.reset(next_ids.len());
        }
        self.list_item_ids = next_ids;
    }

    fn toggle_minimap(&mut self, cx: &mut Context<Self>) {
        self.minimap_visible = !self.minimap_visible;
        cx.notify();
    }

    fn toggle_tool(&mut self, key: String, item_id: String, cx: &mut Context<Self>) {
        if !self.expanded_tools.insert(key.clone()) {
            self.expanded_tools.remove(&key);
        }
        if let ChatStatus::Ready(document) = &self.status
            && let Some(index) = document.items.iter().position(|item| match item {
                ConversationItem::Message(message) => message.id == item_id,
                ConversationItem::Process(group) => {
                    group.messages.iter().any(|message| message.id == item_id)
                }
            })
        {
            self.list_state.remeasure_items(index..index + 1);
        }
        cx.notify();
    }

    fn toggle_process(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded_processes.insert(key.clone()) {
            self.expanded_processes.remove(&key);
        }
        if let Some(index) = self.list_item_ids.iter().position(|id| id == &key) {
            self.list_state.remeasure_items(index..index + 1);
        }
        cx.notify();
    }

    fn render_composer_popup(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let popup = self.popup.as_ref()?;
        let rows = match popup {
            ComposerPopup::Slash(commands) => commands
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    let description = command.description.clone().unwrap_or_default();
                    h_flex()
                        .debug_selector(|| "composer-popup-item".into())
                        .gap_2()
                        .px_2()
                        .py_1()
                        .when(index == self.popup_index, |row| {
                            row.bg(cx.theme().secondary_active)
                        })
                        .child(div().font_semibold().child(format!("/{}", command.name)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
            ComposerPopup::At { entries, .. } => entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    h_flex()
                        .debug_selector(|| "composer-popup-item".into())
                        .gap_2()
                        .px_2()
                        .py_1()
                        .when(index == self.popup_index, |row| {
                            row.bg(cx.theme().secondary_active)
                        })
                        .child(Icon::new(if entry.is_dir {
                            IconName::Folder
                        } else {
                            IconName::File
                        }))
                        .child(entry.path.clone())
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        };
        Some(
            v_flex()
                .debug_selector(|| "composer-popup".into())
                .max_h(px(180.))
                .overflow_y_scrollbar()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_attachments(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let panel = cx.entity();
        Some(
            h_flex()
                .debug_selector(|| "composer-attachments".into())
                .gap_2()
                .overflow_x_scrollbar()
                .children(
                    self.attachments
                        .iter()
                        .enumerate()
                        .map(|(index, attachment)| {
                            h_flex()
                                .id(("attachment", index))
                                .gap_1()
                                .p_1()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(img(attachment.preview.clone()).size(px(40.)))
                                .child(
                                    Button::new(("remove-attachment", index))
                                        .xsmall()
                                        .label("删除")
                                        .on_click({
                                            let panel = panel.clone();
                                            move |_, _, cx| {
                                                panel.update(cx, |panel, cx| {
                                                    panel.remove_attachment(index, cx);
                                                });
                                            }
                                        }),
                                )
                        }),
                )
                .into_any_element(),
        )
    }
}

impl Drop for ChatPanel {
    fn drop(&mut self) {
        if let Some(active) = self.active.take() {
            active.shutdown();
        }
    }
}

impl EventEmitter<PanelEvent> for ChatPanel {}

impl Focusable for ChatPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for ChatPanel {
    fn panel_name(&self) -> &'static str {
        "gpui-pi-chat"
    }

    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("对话".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "对话"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> Option<PanelControl> {
        None
    }

    fn inner_padding(&self, _: &App) -> bool {
        false
    }
}

impl Render for ChatPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        let probe = self.probe.clone();
        #[cfg(not(test))]
        let probe = self.probe;
        if self.tail_attached && matches!(self.list_state.is_scrolled_to_end(), Some(false)) {
            // 覆盖滚动条拖拽、PageUp/Home 等路径。
            self.tail_attached = false;
        }
        if self.follow_requested && self.tail_attached {
            self.list_state.scroll_to_end();
        }
        self.follow_requested = false;
        let panel = cx.entity();
        let content = match &self.status {
            ChatStatus::Empty => centered_state(
                IconName::Bot,
                "选择一个历史会话",
                "加载历史后可启动对应的官方 pi RPC 活会话",
                cx,
            ),
            ChatStatus::Loading { title } => {
                centered_state(IconName::LoaderCircle, "正在后台加载历史会话…", title, cx)
            }
            ChatStatus::Error { title, message } => v_flex()
                .debug_selector(|| "chat-error".into())
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .p_6()
                .text_color(cx.theme().danger)
                .child(div().font_semibold().child(format!("{title} 加载失败")))
                .child(div().text_sm().child(message.clone()))
                .into_any_element(),
            ChatStatus::Ready(document) => {
                gpui_pi_ui::ChatWindow::new(document.clone(), self.list_state.clone())
                    .show_minimap(self.minimap_visible)
                    .expanded_tools(Arc::new(self.expanded_tools.clone()))
                    .expanded_processes(Arc::new(self.expanded_processes.clone()))
                    .on_toggle_tool({
                        let panel = cx.entity();
                        move |key, item_id, cx| {
                            panel.update(cx, |panel, cx| {
                                panel.toggle_tool(key, item_id, cx);
                            });
                        }
                    })
                    .on_toggle_process({
                        let panel = cx.entity();
                        move |key, cx| {
                            panel.update(cx, |panel, cx| panel.toggle_process(key, cx));
                        }
                    })
                    .on_toggle_minimap({
                        let panel = cx.entity();
                        move |cx| {
                            panel.update(cx, |panel, cx| panel.toggle_minimap(cx));
                        }
                    })
                    .on_tail_attachment_change(move |attached, _, cx| {
                        panel.update(cx, |panel, cx| {
                            panel.tail_attached = attached;
                            if !attached {
                                panel.list_state.pause_following_tail();
                            }
                            cx.notify();
                        });
                    })
                    .on_tail_detach({
                        let panel = cx.entity();
                        move |_, cx| {
                            panel.update(cx, |panel, cx| {
                                panel.tail_attached = false;
                                panel.list_state.pause_following_tail();
                                cx.notify();
                            });
                        }
                    })
                    .into_any_element()
            }
        };
        let phase = self.active.as_ref().map(|active| active.phase());
        let running = matches!(phase, Some(LivePhase::Running));
        let stopping = matches!(phase, Some(LivePhase::Stopping));
        let live_started = self.active.is_some();
        if self.pending_draft_restore {
            let input = self.composer.clone();
            let text = self
                .draft_key
                .as_deref()
                .map(|key| self.drafts.get(key).text)
                .unwrap_or_default();
            input.update(cx, |input, cx| input.set_value(text, window, cx));
            self.pending_draft_restore = false;
        }
        let popup = self.render_composer_popup(cx);
        let attachments = self.render_attachments(cx);
        let queue_summary = self.active.as_ref().and_then(|active| {
            let steering = active.reducer().steering_queue().len();
            let follow_up = active.reducer().follow_up_queue().len();
            (steering + follow_up > 0)
                .then(|| format!("队列：steer {steering} · follow-up {follow_up}"))
        });

        div()
            .id("chat-workspace")
            .debug_selector(|| "chat-workspace".into())
            .when_some(probe, |this, probe| {
                this.on_prepaint(move |bounds, _, _| probe.record_workspace(bounds))
            })
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::composer_key_down))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.start_attach_paths(paths.paths().to_vec(), cx);
            }))
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .size_full()
                    .min_h_0()
                    .child(div().flex_1().min_h_0().child(content))
                    .when(!self.tail_attached, |view| {
                        view.child(
                            h_flex().justify_center().child(
                                Button::new("follow-latest")
                                    .debug_selector(|| "follow-latest".into())
                                    .small()
                                    .label("跟随最新")
                                    .on_click(cx.listener(Self::resume_follow)),
                            ),
                        )
                    })
                    .when_some(queue_summary, |view, summary| {
                        view.child(
                            div()
                                .px_3()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(summary),
                        )
                    })
                    .when_some(self.rpc_error.clone(), |view, error| {
                        view.child(
                            div()
                                .debug_selector(|| "live-error".into())
                                .px_3()
                                .py_1()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(error),
                        )
                    })
                    .child(
                        v_flex()
                            .debug_selector(|| "live-composer".into())
                            .flex_none()
                            .gap_2()
                            .p_3()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .when_some(popup, |composer, popup| composer.child(popup))
                            .when_some(attachments, |composer, attachments| {
                                composer.child(attachments)
                            })
                            .child(Textarea::new(&self.composer).h(px(76.)))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("attach-images")
                                            .debug_selector(|| "attach-images".into())
                                            .small()
                                            .label("添加图片")
                                            .disabled(
                                                self.attachments.len()
                                                    >= pi_data::MAX_ATTACHED_IMAGES,
                                            )
                                            .on_click(cx.listener(Self::choose_images)),
                                    )
                                    .child(
                                        Button::new("start-live-session")
                                            .small()
                                            .label(if live_started {
                                                "活会话已启动"
                                            } else {
                                                "启动活会话"
                                            })
                                            .disabled(
                                                live_started
                                                    || !matches!(self.status, ChatStatus::Ready(_)),
                                            )
                                            .on_click(cx.listener(Self::start_live)),
                                    )
                                    .child(
                                        Button::new("composer-steer")
                                            .small()
                                            .label("Steer")
                                            .selected(self.composer_mode == ComposerMode::Steer)
                                            .on_click(cx.listener(Self::select_steer)),
                                    )
                                    .child(
                                        Button::new("composer-follow-up")
                                            .small()
                                            .label("Follow-up")
                                            .selected(self.composer_mode == ComposerMode::FollowUp)
                                            .on_click(cx.listener(Self::select_follow_up)),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        Button::new("abort-live")
                                            .small()
                                            .danger()
                                            .label(if stopping {
                                                "正在停止…"
                                            } else {
                                                "停止"
                                            })
                                            .disabled(!running)
                                            .on_click(cx.listener(Self::abort)),
                                    )
                                    .child(
                                        Button::new("send-live")
                                            .small()
                                            .primary()
                                            .label(if running { "加入队列" } else { "发送" })
                                            .disabled(!live_started || stopping)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                let input = this.composer.clone();
                                                this.submit_composer(&input, window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
    }
}

const fn should_restore_submission(kind: RequestFailureKind) -> bool {
    matches!(kind, RequestFailureKind::Rejected)
}

const fn should_restore_idle_phase(
    pending_activity_generation: Option<u64>,
    current_activity_generation: u64,
    phase: LivePhase,
) -> bool {
    matches!(
        (pending_activity_generation, phase),
        (Some(start_generation), LivePhase::Running)
            if start_generation == current_activity_generation
    )
}

fn build_submission(message: String, attachments: &[ComposerAttachment]) -> ComposerSubmission {
    ComposerSubmission {
        message,
        images: attachments
            .iter()
            .map(|attachment| attachment.draft.clone())
            .collect(),
    }
}

fn slash_query(value: &str) -> Option<&str> {
    let query = value.strip_prefix('/')?;
    (!query.chars().any(char::is_whitespace)).then_some(query)
}

fn attachment_from_draft(draft: pi_data::DraftImage) -> Option<ComposerAttachment> {
    let bytes = STANDARD.decode(&draft.data).ok()?;
    let format = match draft.mime_type.as_str() {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        _ => return None,
    };
    Some(ComposerAttachment {
        preview: Arc::new(Image::from_bytes(format, bytes)),
        draft,
    })
}

fn gpui_image_to_draft(image: Image) -> Option<pi_data::DraftImage> {
    let draft = pi_data::image_from_bytes(image.bytes).ok()?;
    matches!(
        image.format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::Webp
    )
    .then_some(draft)
}

fn centered_state(
    icon: IconName,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    cx: &App,
) -> gpui::AnyElement {
    v_flex()
        .debug_selector(|| "chat-empty-or-loading".into())
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .p_6()
        .child(Icon::new(icon).size(gpui::px(32.)))
        .child(div().font_semibold().child(title.into()))
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(detail.into()),
        )
        .into_any_element()
}

fn session_cwd(path: &std::path::Path) -> Option<PathBuf> {
    pi_data::load_session(path)
        .ok()
        .map(|session| PathBuf::from(session.header.cwd))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{AppContext as _, TestAppContext, VisualTestContext, size};
    use gpui_component::Root;

    use super::*;

    fn document(message: &str) -> Arc<ConversationDocument> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.jsonl");
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session\",\"id\":\"s\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"C:/fixture\"}}\n{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"{message}\"}}}}\n"
            ),
        )
        .unwrap();
        Arc::new(pi_render::render_path(path).unwrap())
    }

    fn rich_document() -> Arc<ConversationDocument> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rich.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"rich\",\"timestamp\":\"2026-01-01T00:00:00Z\",\"cwd\":\"C:/fixture\"}\n",
                "{\"type\":\"message\",\"id\":\"u\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"---\\ntitle: Fixture\\ntags: [ui]\\n---\\nhello\"},{\"type\":\"image\",\"data\":\"<redacted>\",\"mimeType\":\"image/png\"}]}}\n",
                "{\"type\":\"message\",\"id\":\"trace\",\"parentId\":\"u\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"inspect the fixture\"},{\"type\":\"toolCall\",\"id\":\"tool\",\"name\":\"bash\",\"arguments\":{\"command\":\"cargo test\"}}]}}\n",
                "{\"type\":\"message\",\"id\":\"r\",\"parentId\":\"trace\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"tool\",\"toolName\":\"bash\",\"content\":[{\"type\":\"text\",\"text\":\"\\u001b[31mfailed\\u001b[0m\"}],\"details\":{\"patch\":\"--- a/a.rs\\n+++ b/a.rs\\n@@ -1 +1 @@\\n-old\\n+new\"},\"isError\":true}}\n",
                "{\"type\":\"message\",\"id\":\"answer\",\"parentId\":\"r\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"# Answer\\n```rust\\nfn main() {}\\n```\\n```mermaid\\ngraph TD; A-->B\\n```\"}]}}\n"
            ),
        )
        .unwrap();
        Arc::new(pi_render::render_path(path).unwrap())
    }

    fn render_status_with_panel(
        cx: &mut TestAppContext,
        status: ChatStatus,
    ) -> (VisualTestContext, gpui::Entity<ChatPanel>) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = captured.clone();
        let handle = cx.open_window(size(gpui::px(520.), gpui::px(480.)), move |window, cx| {
            let panel = cx.new(|cx| {
                let mut panel = ChatPanel::new(window, cx);
                if let ChatStatus::Ready(document) = &status {
                    panel.sync_list_document(document);
                }
                panel.status = status;
                panel
            });
            *result.borrow_mut() = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..8 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        let panel = captured.borrow().clone().unwrap();
        (visual, panel)
    }

    fn render_status(cx: &mut TestAppContext, status: ChatStatus) -> VisualTestContext {
        render_status_with_panel(cx, status).0
    }

    #[gpui::test]
    fn empty_chat_renders_state_selector(cx: &mut TestAppContext) {
        let mut empty = render_status(cx, ChatStatus::Empty);
        assert!(empty.debug_bounds("chat-empty-or-loading").is_some());
        assert!(empty.debug_bounds("live-composer").is_some());
    }

    #[gpui::test]
    fn ready_chat_folds_completed_process_trace(cx: &mut TestAppContext) {
        let (mut ready, panel) = render_status_with_panel(cx, ChatStatus::Ready(rich_document()));
        let scroll = ready.debug_bounds("chat-message-scroll").unwrap();
        assert!(scroll.size.height > px(0.), "message viewport collapsed");
        for selector in [
            "chat-window",
            "chat-message",
            "chat-minimap",
            "process-group",
            "live-composer",
        ] {
            assert!(ready.debug_bounds(selector).is_some(), "missing {selector}");
        }
        for hidden in [
            "process-group-details",
            "thinking-card",
            "thinking-card-details",
            "tool-card",
            "tool-card-details",
        ] {
            assert!(
                ready.debug_bounds(hidden).is_none(),
                "{hidden} must stay lazy while process is collapsed"
            );
        }
        let bounds = ready.debug_bounds("process-group-toggle").unwrap();
        ready.simulate_click(bounds.center(), gpui::Modifiers::default());
        for _ in 0..3 {
            ready.update(|window, cx| window.draw(cx).clear(cx));
            ready.run_until_parked();
        }
        for selector in ["process-group-details", "thinking-card", "tool-card"] {
            assert!(ready.debug_bounds(selector).is_some(), "missing {selector}");
        }
        assert!(ready.debug_bounds("thinking-card-details").is_none());
        assert!(ready.debug_bounds("tool-card-details").is_none());

        panel.update(cx, |panel, cx| {
            panel.toggle_tool("trace:thinking:0".to_owned(), "trace".to_owned(), cx);
        });
        for _ in 0..2 {
            ready.update(|window, cx| window.draw(cx).clear(cx));
            ready.run_until_parked();
        }
        assert!(ready.debug_bounds("thinking-card-details").is_some());
    }

    #[gpui::test]
    fn minimap_navigation_detaches_tail_follow(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = captured.clone();
        let handle = cx.open_window(size(gpui::px(520.), gpui::px(480.)), move |window, cx| {
            let panel = cx.new(|cx| {
                let mut panel = ChatPanel::new(window, cx);
                let document = rich_document();
                panel.sync_list_document(&document);
                panel.status = ChatStatus::Ready(document);
                *result.borrow_mut() = Some(cx.entity());
                panel
            });
            Root::new(panel, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        let bounds = visual.debug_bounds("chat-minimap-node").unwrap();
        visual.simulate_click(bounds.center(), gpui::Modifiers::default());
        let panel = captured.borrow().clone().unwrap();
        panel.update(cx, |panel, _| assert!(!panel.tail_attached));
    }

    #[gpui::test]
    fn composer_popup_attachment_and_file_prompt_render(cx: &mut TestAppContext) {
        let (mut visual, panel) =
            render_status_with_panel(cx, ChatStatus::Ready(document("hello")));
        panel.update(cx, |panel, cx| {
            panel.slash_commands = vec![pi_rpc::RpcSlashCommand {
                name: "fixture".into(),
                description: Some("Fixture command".into()),
                source: pi_rpc::SlashCommandSource::Prompt,
                source_info: pi_rpc::SourceInfo {
                    path: "C:/fixture.md".into(),
                    source: "fixture".into(),
                    scope: pi_rpc::SourceScope::Project,
                    origin: pi_rpc::SourceOrigin::TopLevel,
                    base_dir: None,
                },
            }];
            let draft = pi_data::image_from_bytes(b"\x89PNG\r\n\x1a\nfixture".to_vec()).unwrap();
            panel.attachments = vec![attachment_from_draft(draft).unwrap()];
            panel.popup = Some(ComposerPopup::Slash(panel.slash_commands.clone()));
            cx.notify();
        });
        for _ in 0..2 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("composer-popup").is_some());
        assert!(visual.debug_bounds("composer-popup-item").is_some());
        assert!(visual.debug_bounds("composer-attachments").is_some());

        let button = visual.debug_bounds("attach-images").unwrap();
        visual.simulate_click(button.center(), gpui::Modifiers::default());
        assert!(visual.did_prompt_for_paths());
        visual.simulate_path_prompt_response(|options| {
            assert!(options.files && !options.directories && options.multiple);
            None
        });
        visual.run_until_parked();
    }

    #[gpui::test]
    fn at_popup_uses_utf8_cursor_and_directory_accept_drills_down(cx: &mut TestAppContext) {
        let (mut visual, panel) =
            render_status_with_panel(cx, ChatStatus::Ready(document("hello")));
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.composer_cwd = Some(PathBuf::from("C:/fixture"));
                panel.file_index = Some(pi_data::FileIndex {
                    entries: vec![
                        pi_data::FileIndexEntry {
                            path: "src".into(),
                            is_dir: true,
                        },
                        pi_data::FileIndexEntry {
                            path: "src/main.rs".into(),
                            is_dir: false,
                        },
                    ],
                    truncated: false,
                });
                let cursor = "前缀 @sr".len();
                panel.composer.update(cx, |input, cx| {
                    input.set_value("前缀 @sr 后续", window, cx);
                    input.set_selected_range(cursor..cursor, cx);
                });
                panel.refresh_popup(cx);
                assert!(panel.accept_popup(&panel.composer.clone(), window, cx));
                let input = panel.composer.read(cx);
                assert_eq!(input.value().as_ref(), "前缀 @src/ 后续");
                assert_eq!(input.cursor(), "前缀 @src/".len());
                let ComposerPopup::At { query, entries } = panel.popup.as_ref().unwrap() else {
                    panic!("directory acceptance must immediately reopen @ popup");
                };
                assert_eq!(query.query, "src/");
                assert!(entries.iter().any(|entry| entry.path == "src/main.rs"));
            });
        });
        visual.run_until_parked();
    }

    #[gpui::test]
    fn switching_sessions_saves_and_restores_isolated_drafts(cx: &mut TestAppContext) {
        let (mut visual, panel) =
            render_status_with_panel(cx, ChatStatus::Ready(document("hello")));
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.draft_key = Some("one".into());
                panel.composer.update(cx, |input, cx| {
                    input.set_value("draft one", window, cx);
                });
                panel.save_current_draft(cx);
                panel.draft_key = Some("two".into());
                panel.prepare_draft_restore();
                assert_eq!(panel.drafts.get("one").text, "draft one");
                assert_eq!(panel.drafts.get("two"), pi_data::ComposerDraft::default());
                panel.drafts.set(
                    "two",
                    pi_data::ComposerDraft {
                        text: "draft two".into(),
                        images: Vec::new(),
                    },
                );
                panel.prepare_draft_restore();
                let restored = panel.drafts.get("two");
                panel.composer.update(cx, |input, cx| {
                    input.set_value(restored.text, window, cx);
                });
                assert_eq!(panel.composer.read(cx).value().as_ref(), "draft two");
                assert_eq!(panel.drafts.get("one").text, "draft one");
            });
        });
    }

    #[gpui::test]
    fn minimum_chat_keeps_composer_visible(cx: &mut TestAppContext) {
        let mut ready = render_status(cx, ChatStatus::Ready(document("hello")));
        let chat = ready.debug_bounds("chat-workspace").unwrap();
        let composer = ready.debug_bounds("live-composer").unwrap();
        assert!(chat.size.width > px(0.) && composer.size.height > px(0.));
        assert!(composer.bottom() <= chat.bottom());
    }

    #[gpui::test]
    fn stale_generation_does_not_replace_newer_chat(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let result = captured.clone();
        cx.open_window(size(gpui::px(520.), gpui::px(480.)), move |window, cx| {
            let panel = cx.new(|cx| ChatPanel::new(window, cx));
            *result.borrow_mut() = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let panel = captured.borrow().clone().unwrap();
        panel.update(cx, |panel, _| {
            panel.load_generation = 2;
            assert!(!panel.finish_load(1, "old".to_owned(), Ok(document("old"))));
            assert!(matches!(panel.status, ChatStatus::Empty));
            assert!(panel.finish_load(2, "new".to_owned(), Ok(document("new"))));
            assert!(matches!(panel.status, ChatStatus::Ready(_)));
        });
    }

    #[test]
    fn composer_popup_queries_and_image_only_submission_are_pure() {
        assert_eq!(slash_query("/fix"), Some("fix"));
        assert_eq!(slash_query("/fix now"), None);
        let draft = pi_data::image_from_bytes(b"\x89PNG\r\n\x1a\nfixture".to_vec()).unwrap();
        let attachment = attachment_from_draft(draft.clone()).unwrap();
        let submission = build_submission(String::new(), &[attachment]);
        assert!(submission.message.is_empty());
        assert_eq!(submission.images, vec![draft]);
    }

    #[test]
    fn explicit_rejection_restores_but_ambiguous_failure_does_not() {
        let mut drafts = pi_data::DraftStore::default();
        drafts.set(
            "session",
            pi_data::ComposerDraft {
                text: "new typing".into(),
                images: Vec::new(),
            },
        );
        let rejected = pi_data::ComposerDraft {
            text: "rejected".into(),
            images: Vec::new(),
        };
        if should_restore_submission(RequestFailureKind::Rejected) {
            drafts.restore_submission("session", rejected);
        }
        assert_eq!(drafts.get("session").text, "rejected\n\nnew typing");
        let before = drafts.get("session");
        if should_restore_submission(RequestFailureKind::Ambiguous) {
            drafts.restore_submission("session", pi_data::ComposerDraft::default());
        }
        assert_eq!(drafts.get("session"), before);
    }

    #[test]
    fn failed_submission_only_restores_idle_before_agent_start() {
        assert!(should_restore_idle_phase(Some(7), 7, LivePhase::Running));
        assert!(!should_restore_idle_phase(Some(7), 8, LivePhase::Running));
        assert!(!should_restore_idle_phase(None, 7, LivePhase::Running));
        assert!(!should_restore_idle_phase(Some(7), 7, LivePhase::Idle));
    }

    #[test]
    fn selection_carries_real_session_path() {
        let event = SessionSelected {
            id: "id".to_owned(),
            path: PathBuf::from("C:/sessions/id.jsonl"),
            cwd: PathBuf::from("C:/project"),
            title: "title".to_owned(),
        };
        assert_eq!(event.path.file_name().unwrap(), "id.jsonl");
    }
}
