use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, Icon, IconName, Sizable as _,
    StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dock::{Panel, PanelControl, PanelEvent},
    input::{Input, InputState},
    notification::Notification,
    scroll::ScrollableElement as _,
    v_flex,
};
use pi_data::{
    ProjectSessionView, RunningSessionOverlay, SessionSummary, SessionView, delete_leaf_session,
    export_session_jsonl, list_sessions, rename_session,
};

use crate::panels::LayoutProbe;
use crate::trust_prompt::prompt_project_trust;

#[derive(Debug, Clone, PartialEq)]
pub enum SidebarStatus {
    Loading,
    Ready(Vec<ProjectSessionView>),
    Error(String),
}

#[derive(Clone)]
pub struct SessionSelected {
    pub id: String,
    pub path: PathBuf,
    pub cwd: PathBuf,
    pub title: String,
}

pub struct SessionSidebar {
    focus_handle: FocusHandle,
    status: SidebarStatus,
    selected_id: Option<String>,
    sessions_root: Option<PathBuf>,
    agent_dir: Option<PathBuf>,
    running: RunningSessionOverlay,
    summaries: Vec<SessionSummary>,
    diagnostics_count: usize,
    busy_actions: HashSet<String>,
    rename_input: gpui::Entity<InputState>,
    rename_target: Option<String>,
    load_generation: u64,
    probe: Option<LayoutProbe>,
}

impl SessionSidebar {
    #[cfg(not(test))]
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("会话名称"));
        let agent_dir = pi_data::agent_dir();
        let sessions_root = agent_dir.as_ref().map(|dir| dir.join("sessions"));
        let mut sidebar = Self {
            focus_handle: cx.focus_handle(),
            status: SidebarStatus::Loading,
            selected_id: None,
            sessions_root,
            agent_dir,
            running: RunningSessionOverlay::default(),
            summaries: Vec::new(),
            diagnostics_count: 0,
            busy_actions: HashSet::new(),
            rename_input,
            rename_target: None,
            load_generation: 0,
            probe: None,
        };
        sidebar.refresh(window, cx);
        sidebar
    }

    #[cfg(test)]
    pub(crate) fn new_empty(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            status: SidebarStatus::Ready(Vec::new()),
            selected_id: None,
            sessions_root: None,
            agent_dir: None,
            running: RunningSessionOverlay::default(),
            summaries: Vec::new(),
            diagnostics_count: 0,
            busy_actions: HashSet::new(),
            rename_input: cx.new(|cx| InputState::new(window, cx).placeholder("会话名称")),
            rename_target: None,
            load_generation: 0,
            probe: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_probe(mut self, probe: LayoutProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;
        self.status = SidebarStatus::Loading;
        cx.notify();
        let Some(root) = self.sessions_root.clone() else {
            self.status = SidebarStatus::Error("无法定位 pi agent 数据目录".to_owned());
            return;
        };
        let executor = cx.background_executor().clone();
        let running = self.running.clone();
        cx.spawn_in(window, async move |sidebar, cx| {
            let (summaries, diagnostics_count, view) = executor
                .spawn(async move {
                    let listed = list_sessions(root);
                    let diagnostics_count = listed.diagnostics.len();
                    let view = pi_data::build_session_view(listed.sessions.clone(), &running);
                    (listed.sessions, diagnostics_count, view)
                })
                .await;
            let applied = sidebar
                .update(cx, |sidebar, cx| {
                    let applied =
                        sidebar.finish_refresh(generation, summaries, diagnostics_count, view);
                    if applied {
                        cx.notify();
                    }
                    applied
                })
                .unwrap_or(false);
            if applied && diagnostics_count > 0 {
                let _ = cx.update(|window, cx| {
                    window.push_notification(
                        Notification::warning(format!(
                            "会话扫描跳过了 {diagnostics_count} 个异常文件或目录"
                        )),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    fn finish_refresh(
        &mut self,
        generation: u64,
        summaries: Vec<SessionSummary>,
        diagnostics_count: usize,
        view: Vec<ProjectSessionView>,
    ) -> bool {
        if self.load_generation != generation {
            return false;
        }
        self.summaries = summaries;
        self.diagnostics_count = diagnostics_count;
        self.status = SidebarStatus::Ready(view);
        true
    }

    fn handle_refresh(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh(window, cx);
    }

    fn select_session(
        &mut self,
        session: &SessionView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_id = Some(session.id.clone());
        cx.emit(SessionSelected {
            id: session.id.clone(),
            path: session.path.clone(),
            cwd: session.cwd.clone(),
            title: session.title.clone(),
        });
        prompt_project_trust(self.agent_dir.clone(), &session.cwd, window, cx);
        cx.notify();
    }

    fn start_rename(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(summary) = self.summaries.iter().find(|item| item.id == id) else {
            return;
        };
        if self.running.contains(&summary.id) {
            window.push_notification(Notification::warning("运行中的会话不能重命名"), cx);
            return;
        }
        let value = summary.name.clone().unwrap_or_default();
        self.rename_input
            .update(cx, |input, cx| input.set_value(value, window, cx));
        self.rename_target = Some(id);
        cx.notify();
    }

    fn commit_rename(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.rename_target.clone() else {
            return;
        };
        if !self.busy_actions.insert(id.clone()) {
            return;
        }
        let Some(summary) = self.summaries.iter().find(|item| item.id == id).cloned() else {
            self.busy_actions.remove(&id);
            return;
        };
        let value = self.rename_input.read(cx).value().to_string();
        let running = self.running.contains(&id);
        let executor = cx.background_executor().clone();
        cx.notify();
        cx.spawn_in(window, async move |sidebar, cx| {
            let result = executor
                .spawn(async move { rename_session(&summary, &value, running) })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = sidebar.update(cx, |sidebar, cx| {
                    sidebar.busy_actions.remove(&id);
                    match result {
                        Ok(()) => {
                            sidebar.rename_target = None;
                            window.push_notification(Notification::success("会话名称已更新"), cx);
                            sidebar.refresh(window, cx);
                        }
                        Err(error) => window.push_notification(
                            Notification::error(format!("{error}；请刷新会话列表后重试")),
                            cx,
                        ),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn confirm_delete(&self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(summary) = self.summaries.iter().find(|item| item.id == id).cloned() else {
            return;
        };
        let summaries = self.summaries.clone();
        let running = self.running.contains(&id);
        let entity = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let summary = summary.clone();
            let summaries = summaries.clone();
            let entity = entity.clone();
            dialog
                .title("删除会话？")
                .overlay_closable(false)
                .child("只允许删除没有子会话的历史会话。此操作不可撤销。")
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .ok_text("删除")
                        .cancel_text("取消")
                        .show_cancel(true)
                        .ok_variant(ButtonVariant::Danger)
                        .on_ok(move |_, window, cx| {
                            let started = entity.update(cx, |sidebar, cx| {
                                let started = sidebar.busy_actions.insert(summary.id.clone());
                                if started {
                                    cx.notify();
                                }
                                started
                            });
                            if !started {
                                return true;
                            }
                            let summary = summary.clone();
                            let summaries = summaries.clone();
                            let id = summary.id.clone();
                            let entity = entity.clone();
                            let executor = cx.background_executor().clone();
                            let window_handle = window.window_handle();
                            cx.spawn(async move |cx| {
                                let result = executor
                                    .spawn(async move {
                                        delete_leaf_session(&summary, &summaries, running)
                                    })
                                    .await;
                                let _ = window_handle.update(cx, |_, window, cx| {
                                    entity.update(cx, |sidebar, cx| {
                                        sidebar.busy_actions.remove(&id);
                                        match result {
                                            Ok(()) => {
                                                window.push_notification(
                                                    Notification::success("会话已删除"),
                                                    cx,
                                                );
                                                sidebar.refresh(window, cx);
                                            }
                                            Err(error) => window.push_notification(
                                                Notification::error(format!(
                                                    "{error}；请刷新会话列表后重试"
                                                )),
                                                cx,
                                            ),
                                        }
                                        cx.notify();
                                    });
                                });
                            })
                            .detach();
                            true
                        }),
                )
        });
    }

    fn export_session(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.busy_actions.insert(id.clone()) {
            return;
        }
        let Some(summary) = self.summaries.iter().find(|item| item.id == id).cloned() else {
            self.busy_actions.remove(&id);
            return;
        };
        let start = if summary.cwd.is_dir() {
            summary.cwd.clone()
        } else {
            PathBuf::from(".")
        };
        let file_name = format!("pi-session-{}.jsonl", summary.id);
        let receiver = cx.prompt_for_new_path(&start, Some(&file_name));
        let executor = cx.background_executor().clone();
        cx.notify();
        cx.spawn_in(window, async move |sidebar, cx| {
            let destination = receiver.await.ok().into_iter().flatten().flatten().next();
            let result = if let Some(destination) = destination {
                Some(
                    executor
                        .spawn(async move { export_session_jsonl(&summary, destination) })
                        .await,
                )
            } else {
                None
            };
            let _ = cx.update(|window, cx| {
                let _ = sidebar.update(cx, |sidebar, cx| {
                    sidebar.busy_actions.remove(&id);
                    match result {
                        Some(Ok(())) => {
                            window.push_notification(Notification::success("原始 JSONL 已导出"), cx)
                        }
                        Some(Err(error)) => window.push_notification(
                            Notification::error(format!("{error}；请刷新会话列表后重试")),
                            cx,
                        ),
                        None => {}
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn render_session_node(
        &self,
        node: &SessionView,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = node.id.clone();
        let rename_id = id.clone();
        let delete_id = id.clone();
        let export_id = id.clone();
        let selected = self.selected_id.as_deref() == Some(&node.id);
        let busy = self.busy_actions.contains(&node.id);
        let metric = format_metrics(node);
        v_flex()
            .w_full()
            .child(
                div()
                    .id(SharedString::from(format!("session-row-{}", node.id)))
                    .debug_selector(|| "session-row".into())
                    .ml(px(depth as f32 * 12.))
                    .p_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(cx.theme().accent.opacity(0.16)))
                    .hover(|row| row.bg(cx.theme().muted))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let SidebarStatus::Ready(projects) = &this.status
                            && let Some(session) = find_session(projects, &id)
                        {
                            let session = session.clone();
                            this.select_session(&session, window, cx);
                        }
                    }))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .child(
                                Icon::new(if node.running {
                                    IconName::LoaderCircle
                                } else {
                                    IconName::File
                                })
                                .size_4()
                                .text_color(if node.running {
                                    cx.theme().success
                                } else {
                                    cx.theme().muted_foreground
                                }),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(div().text_sm().truncate().child(node.title.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(metric),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_1()
                                    .child(
                                        Button::new(format!("rename-{}", node.id))
                                            .debug_selector({
                                                let id = node.id.clone();
                                                move || format!("rename-{id}")
                                            })
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Settings)
                                            .tooltip("重命名")
                                            .disabled(busy)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.start_rename(rename_id.clone(), window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(format!("export-{}", node.id))
                                            .debug_selector({
                                                let id = node.id.clone();
                                                move || format!("export-{id}")
                                            })
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Copy)
                                            .tooltip("导出原始 JSONL")
                                            .disabled(busy)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.export_session(export_id.clone(), window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(format!("delete-{}", node.id))
                                            .debug_selector({
                                                let id = node.id.clone();
                                                move || format!("delete-{id}")
                                            })
                                            .ghost()
                                            .xsmall()
                                            .icon(IconName::Delete)
                                            .tooltip("删除")
                                            .disabled(busy)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.confirm_delete(delete_id.clone(), window, cx)
                                            })),
                                    ),
                            ),
                    ),
            )
            .children(
                node.children
                    .iter()
                    .map(|child| self.render_session_node(child, depth + 1, cx)),
            )
            .into_any_element()
    }
}

impl EventEmitter<PanelEvent> for SessionSidebar {}
impl EventEmitter<SessionSelected> for SessionSidebar {}

impl Focusable for SessionSidebar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for SessionSidebar {
    fn panel_name(&self) -> &'static str {
        "gpui-pi-sidebar"
    }
    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("会话".into())
    }
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "会话"
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

impl Render for SessionSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        let probe = self.probe.clone();
        #[cfg(not(test))]
        let probe = self.probe;
        let content = match &self.status {
            SidebarStatus::Loading => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child("正在后台加载会话…")
                .into_any_element(),
            SidebarStatus::Error(error) => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().danger)
                .child("会话加载失败")
                .child(error.clone())
                .into_any_element(),
            SidebarStatus::Ready(projects) if projects.is_empty() => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("没有历史会话")
                .into_any_element(),
            SidebarStatus::Ready(projects) => v_flex()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .gap_3()
                .children(projects.iter().map(|project| {
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .px_2()
                                .text_xs()
                                .font_semibold()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(project_label(&project.root)),
                        )
                        .children(
                            project
                                .sessions
                                .iter()
                                .map(|session| self.render_session_node(session, 0, cx)),
                        )
                }))
                .into_any_element(),
        };
        div()
            .id("session-sidebar")
            .debug_selector(|| "session-sidebar".into())
            .when_some(probe, |this, probe| {
                this.on_prepaint(move |bounds, _, _| probe.record_sidebar(bounds))
            })
            .track_focus(&self.focus_handle)
            .size_full()
            .min_w_0()
            .p_2()
            .bg(cx.theme().sidebar)
            .child(
                v_flex()
                    .size_full()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().font_semibold().child("项目与会话"))
                            .child(
                                Button::new("refresh-sessions")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Redo)
                                    .tooltip("刷新")
                                    .on_click(cx.listener(Self::handle_refresh)),
                            ),
                    )
                    .when(self.rename_target.is_some(), |view| {
                        view.child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&self.rename_input).w_full()),
                                )
                                .child(
                                    Button::new("commit-rename")
                                        .small()
                                        .primary()
                                        .label("保存")
                                        .disabled(
                                            self.rename_target
                                                .as_ref()
                                                .is_some_and(|id| self.busy_actions.contains(id)),
                                        )
                                        .on_click(cx.listener(Self::commit_rename)),
                                ),
                        )
                    })
                    .when(self.diagnostics_count > 0, |view| {
                        view.child(
                            div()
                                .id("session-diagnostics")
                                .debug_selector(|| "session-diagnostics".into())
                                .text_xs()
                                .text_color(cx.theme().warning)
                                .child(format!(
                                    "扫描跳过 {} 个异常文件或目录",
                                    self.diagnostics_count
                                )),
                        )
                    })
                    .child(content),
            )
    }
}

fn format_metrics(session: &SessionView) -> String {
    let token = session
        .metrics
        .recent_context_tokens
        .map(format_tokens)
        .unwrap_or_else(|| "— token".to_owned());
    let running = if session.running { "运行中 · " } else { "" };
    let branch = session
        .branch
        .as_deref()
        .map(|branch| format!(" · {}", truncate_text(branch, 24)))
        .unwrap_or_default();
    format!(
        "{running}{} · {} 条消息 · {token} · ${:.4}{branch}",
        format_relative_time(session.modified),
        session.message_count,
        session.metrics.cumulative_cost
    )
}

fn format_relative_time(modified: std::time::SystemTime) -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(modified)
        .map_or(0, |duration| duration.as_secs());
    match seconds {
        0..=59 => "刚刚".to_owned(),
        60..=3_599 => format!("{} 分钟前", seconds / 60),
        3_600..=86_399 => format!("{} 小时前", seconds / 3_600),
        86_400..=2_591_999 => format!("{} 天前", seconds / 86_400),
        _ => format!("{} 个月前", seconds / 2_592_000),
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000 {
        format!("{:.1}k token", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens} token")
    }
}

fn project_label(root: &Path) -> String {
    root.file_name()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
}

fn find_session<'a>(projects: &'a [ProjectSessionView], id: &str) -> Option<&'a SessionView> {
    fn find<'a>(sessions: &'a [SessionView], id: &str) -> Option<&'a SessionView> {
        sessions.iter().find_map(|session| {
            if session.id == id {
                Some(session)
            } else {
                find(&session.children, id)
            }
        })
    }
    projects
        .iter()
        .find_map(|project| find(&project.sessions, id))
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use gpui::{TestAppContext, VisualTestContext, size};
    use gpui_component::Root;
    use pi_data::SessionMetrics;

    use super::*;

    fn ready_sidebar(window: &mut Window, cx: &mut Context<SessionSidebar>) -> SessionSidebar {
        let session = SessionView {
            id: "fixture-session".to_owned(),
            path: PathBuf::from("fixture.jsonl"),
            cwd: PathBuf::from("C:/fixture/project"),
            title: "Fixture session".to_owned(),
            modified: SystemTime::UNIX_EPOCH,
            message_count: 3,
            metrics: SessionMetrics {
                cumulative_tokens: 100,
                cumulative_cost: 0.125,
                recent_context_tokens: Some(42_000),
            },
            branch: Some("feature/fixture".to_owned()),
            running: true,
            children: Vec::new(),
        };
        SessionSidebar {
            focus_handle: cx.focus_handle(),
            status: SidebarStatus::Ready(vec![ProjectSessionView {
                key: "fixture".to_owned(),
                root: PathBuf::from("C:/fixture/project"),
                modified: SystemTime::UNIX_EPOCH,
                sessions: vec![session],
            }]),
            selected_id: None,
            sessions_root: None,
            agent_dir: None,
            running: RunningSessionOverlay::new(["fixture-session".to_owned()]),
            summaries: Vec::new(),
            diagnostics_count: 2,
            busy_actions: HashSet::new(),
            rename_input: cx.new(|cx| InputState::new(window, cx)),
            rename_target: None,
            load_generation: 0,
            probe: None,
        }
    }

    #[gpui::test]
    fn stale_refresh_generation_does_not_replace_newer_state(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let sidebar = std::rc::Rc::new(std::cell::RefCell::new(None));
        let captured = sidebar.clone();
        cx.open_window(size(px(320.), px(560.)), move |window, cx| {
            let entity = cx.new(|cx| ready_sidebar(window, cx));
            *captured.borrow_mut() = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let sidebar = sidebar.borrow().clone().unwrap();
        sidebar.update(cx, |sidebar, _| {
            sidebar.load_generation = 2;
            assert!(!sidebar.finish_refresh(1, Vec::new(), 0, Vec::new()));
            assert!(matches!(
                sidebar.status,
                SidebarStatus::Ready(ref projects) if !projects.is_empty()
            ));
        });
    }

    #[gpui::test]
    fn ready_sidebar_renders_project_row_metrics_and_actions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let handle = cx.open_window(size(px(320.), px(560.)), |window, cx| {
            let sidebar = cx.new(|cx| ready_sidebar(window, cx));
            Root::new(sidebar, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("session-sidebar").is_some());
        assert!(visual.debug_bounds("session-row").is_some());
        assert!(visual.debug_bounds("rename-fixture-session").is_some());
        assert!(visual.debug_bounds("export-fixture-session").is_some());
        assert!(visual.debug_bounds("delete-fixture-session").is_some());
        assert!(visual.debug_bounds("session-diagnostics").is_some());
        let row = visual
            .debug_bounds("session-row")
            .expect("session row missing");
        assert!(row.size.width > px(0.) && row.size.height > px(0.));
    }
}
