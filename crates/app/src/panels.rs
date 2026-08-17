use std::{collections::HashSet, path::PathBuf, sync::Arc};

use futures::{StreamExt as _, channel::mpsc::UnboundedReceiver};
use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, FollowMode,
    InteractiveElement as _, IntoElement, ListAlignment, ListState, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, Icon, IconName, Selectable as _,
    Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    v_flex,
};
use pi_render::{ConversationDocument, LivePhase};

use crate::{
    live_session::{ActiveSession, ComposerMode, PumpMessage, RpcIntent, official_binary},
    session_sidebar::SessionSelected,
};

pub struct ChatPanel {
    focus_handle: FocusHandle,
    status: ChatStatus,
    load_generation: u64,
    active: Option<ActiveSession>,
    composer: gpui::Entity<TextareaState>,
    composer_mode: ComposerMode,
    list_state: ListState,
    list_message_ids: Vec<String>,
    tail_attached: bool,
    follow_requested: bool,
    minimap_visible: bool,
    expanded_tools: HashSet<String>,
    rpc_error: Option<String>,
    activity_generation: u64,
    calibration_generation: u64,
    _composer_subscription: Subscription,
    probe: Option<LayoutProbe>,
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
        let subscription = cx.subscribe_in(&composer, window, |this, input, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                this.submit_composer(input, window, cx);
            }
        });
        let list_state = ListState::new(0, ListAlignment::Top, px(1200.));
        list_state.set_follow_mode(FollowMode::Tail);
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
            list_state,
            list_message_ids: Vec::new(),
            tail_attached: true,
            follow_requested: false,
            minimap_visible: true,
            expanded_tools: HashSet::new(),
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

    pub fn load_selection(&mut self, selection: SessionSelected, cx: &mut Context<Self>) {
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;
        if let Some(active) = self.active.take() {
            active.shutdown();
        }
        self.status = ChatStatus::Loading {
            title: selection.title.clone(),
        };
        self.rpc_error = None;
        self.activity_generation = 0;
        self.calibration_generation = 0;
        self.tail_attached = true;
        self.follow_requested = false;
        self.minimap_visible = true;
        self.expanded_tools.clear();
        self.list_state = ListState::new(0, ListAlignment::Top, px(1200.));
        self.list_state.set_follow_mode(FollowMode::Tail);
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
        self.list_message_ids.clear();
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
            PumpMessage::RequestFinished { intent, result, .. } => match result {
                Ok(()) => {
                    self.rpc_error = None;
                }
                Err(error) => {
                    if intent == RpcIntent::Abort {
                        active.reducer_mut().restore_running_if_stopping();
                    } else if active.phase() == LivePhase::Running {
                        active.reducer_mut().restore_phase(LivePhase::Idle);
                    }
                    self.rpc_error = Some(error);
                }
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

    fn submit_composer(
        &mut self,
        input: &gpui::Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = input.read(cx).value().trim().to_owned();
        if message.is_empty() {
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
        active.dispatch(intent, Some(message), self.composer_mode);
        input.update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }

    fn abort(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(active) = self.active.as_mut()
            && active.phase() == LivePhase::Running
        {
            active.dispatch(RpcIntent::Abort, None, self.composer_mode);
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
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        let old_len = self.list_message_ids.len();
        let shared_prefix = self
            .list_message_ids
            .iter()
            .zip(&next_ids)
            .take_while(|(old, new)| old == new)
            .count();
        if shared_prefix == old_len && next_ids.len() >= old_len {
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
        self.list_message_ids = next_ids;
    }

    fn toggle_minimap(&mut self, cx: &mut Context<Self>) {
        self.minimap_visible = !self.minimap_visible;
        cx.notify();
    }

    fn toggle_tool(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded_tools.insert(key.clone()) {
            self.expanded_tools.remove(&key);
        }
        if let Some((message_id, _)) = key.split_once(':')
            && let Some(index) = self.list_message_ids.iter().position(|id| id == message_id)
        {
            self.list_state.remeasure_items(index..index + 1);
        }
        cx.notify();
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .on_toggle_tool({
                        let panel = cx.entity();
                        move |key, cx| {
                            panel.update(cx, |panel, cx| panel.toggle_tool(key, cx));
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
                            .child(Textarea::new(&self.composer).h(px(76.)))
                            .child(
                                h_flex()
                                    .gap_2()
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
                "{\"type\":\"message\",\"id\":\"a\",\"parentId\":\"u\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"# Answer\\n```rust\\nfn main() {}\\n```\\n```mermaid\\ngraph TD; A-->B\\n```\"},{\"type\":\"toolCall\",\"id\":\"tool\",\"name\":\"bash\",\"arguments\":{\"command\":\"cargo test\"}}]}}\n",
                "{\"type\":\"message\",\"id\":\"r\",\"parentId\":\"a\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"tool\",\"toolName\":\"bash\",\"content\":[{\"type\":\"text\",\"text\":\"\\u001b[31mfailed\\u001b[0m\"}],\"details\":{\"patch\":\"--- a/a.rs\\n+++ b/a.rs\\n@@ -1 +1 @@\\n-old\\n+new\"},\"isError\":true}}\n"
            ),
        )
        .unwrap();
        Arc::new(pi_render::render_path(path).unwrap())
    }

    fn render_status(cx: &mut TestAppContext, status: ChatStatus) -> VisualTestContext {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let handle = cx.open_window(size(gpui::px(520.), gpui::px(480.)), move |window, cx| {
            let panel = cx.new(|cx| {
                let mut panel = ChatPanel::new(window, cx);
                if let ChatStatus::Ready(document) = &status {
                    panel.sync_list_document(document);
                }
                panel.status = status;
                panel
            });
            Root::new(panel, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..8 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        visual
    }

    #[gpui::test]
    fn empty_chat_renders_state_selector(cx: &mut TestAppContext) {
        let mut empty = render_status(cx, ChatStatus::Empty);
        assert!(empty.debug_bounds("chat-empty-or-loading").is_some());
        assert!(empty.debug_bounds("live-composer").is_some());
    }

    #[gpui::test]
    fn ready_chat_renders_virtualized_shell_and_minimap(cx: &mut TestAppContext) {
        let mut ready = render_status(cx, ChatStatus::Ready(rich_document()));
        for selector in ["chat-window", "chat-minimap", "live-composer"] {
            assert!(ready.debug_bounds(selector).is_some(), "missing {selector}");
        }
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
