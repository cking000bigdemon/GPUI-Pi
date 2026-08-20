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
    dialog::{DialogAction, DialogClose, DialogFooter},
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Input, InputState},
    notification::Notification,
    scroll::ScrollableElement as _,
    tooltip::Tooltip,
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

#[derive(Debug, Clone)]
pub struct WorktreeSelected {
    pub cwd: PathBuf,
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
    /// 当前鼠标悬停的会话行。行内低频操作（重命名/导出/删除）只在悬停行渲染，
    /// 规范 S-9：一行默认不铺开三个图标按钮。
    hovered_id: Option<String>,
    browsing_cwd: Option<PathBuf>,
    worktrees: Option<pi_data::WorktreeSnapshot>,
    worktree_error: Option<String>,
    worktree_generation: u64,
    worktree_busy: bool,
    worktree_expanded: bool,
    /// linked worktree 的移除操作只在对应行 hover 时出现（规范 S-9）。
    hovered_worktree_index: Option<usize>,
    worktree_input: gpui::Entity<InputState>,
    probe: Option<LayoutProbe>,
}

impl SessionSidebar {
    #[cfg(not(test))]
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("会话名称"));
        let agent_dir = pi_data::agent_dir();
        let sessions_root = agent_dir.as_ref().map(|dir| dir.join("sessions"));
        let worktree_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("新建 worktree 分支"));
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
            hovered_id: None,
            browsing_cwd: None,
            worktrees: None,
            worktree_error: None,
            worktree_generation: 0,
            worktree_busy: false,
            worktree_expanded: false,
            hovered_worktree_index: None,
            worktree_input,
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
            hovered_id: None,
            browsing_cwd: None,
            worktrees: None,
            worktree_error: None,
            worktree_generation: 0,
            worktree_busy: false,
            worktree_expanded: false,
            hovered_worktree_index: None,
            worktree_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("新建 worktree 分支")),
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
        self.set_browsing_cwd(Some(session.cwd.clone()), cx);
        cx.emit(SessionSelected {
            id: session.id.clone(),
            path: session.path.clone(),
            cwd: session.cwd.clone(),
            title: session.title.clone(),
        });
        prompt_project_trust(self.agent_dir.clone(), &session.cwd, window, cx);
        cx.notify();
    }

    pub fn set_browsing_cwd(&mut self, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        if self
            .browsing_cwd
            .as_ref()
            .map(pi_data::project_identity_key)
            == cwd.as_ref().map(pi_data::project_identity_key)
        {
            return;
        }
        self.browsing_cwd = cwd;
        self.reload_worktrees(cx);
    }

    fn reload_worktrees(&mut self, cx: &mut Context<Self>) {
        self.worktree_generation = self.worktree_generation.wrapping_add(1);
        let generation = self.worktree_generation;
        self.worktrees = None;
        self.worktree_error = None;
        // 刷新后的索引属于新快照，旧 hover 不得误点亮另一行。
        self.hovered_worktree_index = None;
        let Some(cwd) = self.browsing_cwd.clone() else {
            cx.notify();
            return;
        };
        let cwd_key = pi_data::project_identity_key(&cwd);
        let executor = cx.background_executor().clone();
        cx.spawn(async move |sidebar, cx| {
            let result = executor
                .spawn(async move { pi_data::list_worktrees(cwd) })
                .await;
            let _ = sidebar.update(cx, |sidebar, cx| {
                if sidebar.finish_worktree_refresh(generation, &cwd_key, result) {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_worktree_refresh(
        &mut self,
        generation: u64,
        cwd_key: &str,
        result: Result<pi_data::WorktreeSnapshot, pi_data::GitError>,
    ) -> bool {
        if generation != self.worktree_generation
            || self
                .browsing_cwd
                .as_ref()
                .map(pi_data::project_identity_key)
                .as_deref()
                != Some(cwd_key)
        {
            return false;
        }
        match result {
            Ok(snapshot) => {
                self.worktrees = Some(snapshot);
                self.worktree_error = None;
            }
            Err(error) => {
                self.worktrees = None;
                self.worktree_error = Some(error.to_string());
            }
        }
        true
    }

    fn switch_worktree(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.set_browsing_cwd(Some(cwd.clone()), cx);
        cx.emit(WorktreeSelected { cwd });
    }

    fn create_worktree(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.worktree_busy {
            return;
        }
        let Some(cwd) = self.browsing_cwd.clone() else {
            return;
        };
        let branch = self.worktree_input.read(cx).value().trim().to_owned();
        if branch.is_empty() {
            return;
        }
        self.worktree_busy = true;
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |sidebar, cx| {
            let result = executor
                .spawn(async move { pi_data::add_worktree(cwd, &branch) })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = sidebar.update(cx, |sidebar, cx| {
                    sidebar.worktree_busy = false;
                    match result {
                        Ok(worktree) => {
                            sidebar
                                .worktree_input
                                .update(cx, |input, cx| input.set_value("", window, cx));
                            sidebar.switch_worktree(worktree.path, cx);
                            window.push_notification(Notification::success("worktree 已创建"), cx);
                        }
                        Err(error) => {
                            window.push_notification(Notification::error(error.to_string()), cx)
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn request_remove_worktree(
        &self,
        path: PathBuf,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(cwd) = self.browsing_cwd.clone() else {
            return;
        };
        let sidebar = cx.entity();
        let (title, description) = remove_worktree_dialog_copy(force);
        window.open_dialog(cx, move |dialog, _, _| {
            let sidebar = sidebar.clone();
            let cwd = cwd.clone();
            let path = path.clone();
            dialog
                .title(title)
                .close_button(false)
                .overlay_closable(false)
                .child(description)
                // 普通 Dialog 不会根据 button_props 自动生成 footer；显式使用组件库
                // DialogClose/DialogAction，确保短文案与 dirty 长文案下操作区都参与测量。
                .on_ok(move |_, window, cx| {
                    let started = sidebar.update(cx, |sidebar, cx| {
                        if sidebar.worktree_busy {
                            return false;
                        }
                        sidebar.worktree_busy = true;
                        cx.notify();
                        true
                    });
                    if !started {
                        return false;
                    }
                    let executor = cx.background_executor().clone();
                    let sidebar = sidebar.clone();
                    let cwd = cwd.clone();
                    let path = path.clone();
                    let remove_path = path.clone();
                    let handle = window.window_handle();
                    cx.spawn(async move |cx| {
                        let result = executor
                            .spawn(
                                async move { pi_data::remove_worktree(cwd, &remove_path, force) },
                            )
                            .await;
                        let _ = handle.update(cx, |_, window, cx| {
                            sidebar.update(cx, |sidebar, cx| {
                                sidebar.worktree_busy = false;
                                match result {
                                    Ok(()) => {
                                        let fallback = sidebar
                                            .worktrees
                                            .as_ref()
                                            .map(|snapshot| snapshot.project_root.clone());
                                        if sidebar.browsing_cwd.as_ref().is_some_and(|current| {
                                            pi_data::project_identity_key(current)
                                                == pi_data::project_identity_key(&path)
                                        }) {
                                            if let Some(fallback) = fallback {
                                                sidebar.switch_worktree(fallback, cx);
                                            }
                                        } else {
                                            sidebar.reload_worktrees(cx);
                                        }
                                        window.push_notification(
                                            Notification::success("worktree 已移除"),
                                            cx,
                                        );
                                    }
                                    Err(pi_data::GitError::DirtyWorktree) if !force => {
                                        sidebar.request_remove_worktree(
                                            path.clone(),
                                            true,
                                            window,
                                            cx,
                                        );
                                    }
                                    Err(error) => window.push_notification(
                                        Notification::error(error.to_string()),
                                        cx,
                                    ),
                                }
                                cx.notify();
                            });
                        });
                    })
                    .detach();
                    true
                })
                .footer(
                    DialogFooter::new()
                        .child(
                            div()
                                .debug_selector(|| "cancel-remove-worktree".into())
                                .child(
                                    DialogClose::new().child(
                                        Button::new("cancel-remove-worktree")
                                            .secondary()
                                            .label("取消"),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| "confirm-remove-worktree".into())
                                .child(
                                    DialogAction::new().child(
                                        Button::new("confirm-remove-worktree")
                                            .danger()
                                            .label("移除"),
                                    ),
                                ),
                        ),
                )
        });
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
        let hover_id = node.id.clone();
        let rename_id = id.clone();
        let delete_id = id.clone();
        let export_id = id.clone();
        let selected = self.selected_id.as_deref() == Some(&node.id);
        let busy = self.busy_actions.contains(&node.id);
        // 规范 S-8：一行不堆超过 3 个片段，所以元信息拆两行，分支名进 tooltip。
        let (metric_primary, metric_secondary) = format_metrics(node);
        let branch_tooltip = node.branch.clone();
        // 低频操作在悬停行才出现；busy 时保持可见，否则禁用态一闪就消失，用户看不到反馈。
        let actions_visible = self.hovered_id.as_deref() == Some(&node.id) || busy;
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
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let next = hovered.then(|| hover_id.clone());
                        if this.hovered_id != next {
                            this.hovered_id = next;
                            cx.notify();
                        }
                    }))
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
                                    .gap_0p5()
                                    .child(div().text_sm().truncate().child(node.title.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(metric_primary),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "session-metric-{}",
                                                node.id
                                            )))
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .when_some(branch_tooltip, |row, branch| {
                                                row.tooltip(move |window, cx| {
                                                    Tooltip::new(format!("分支：{branch}"))
                                                        .build(window, cx)
                                                })
                                            })
                                            .child(metric_secondary),
                                    ),
                            )
                            .when(actions_visible, |row| {
                                row.child(
                                    div()
                                        .flex()
                                        .flex_none()
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
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.start_rename(
                                                            rename_id.clone(),
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
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
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.export_session(
                                                            export_id.clone(),
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
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
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.confirm_delete(
                                                            delete_id.clone(),
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        ),
                                )
                            }),
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
impl EventEmitter<WorktreeSelected> for SessionSidebar {}

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
        let worktree_controls = self
            .worktrees
            .as_ref()
            .filter(|snapshot| snapshot.is_top_level)
            .map(|snapshot| {
                let current = self.browsing_cwd.clone();
                let rows = snapshot
                    .worktrees
                    .iter()
                    .enumerate()
                    .map(|(index, worktree)| {
                        let path = worktree.path.clone();
                        let remove_path = path.clone();
                        let branch = worktree
                            .branch
                            .clone()
                            .unwrap_or_else(|| "detached HEAD".to_owned());
                        let selected = current.as_ref().is_some_and(|cwd| {
                            pi_data::project_identity_key(cwd)
                                == pi_data::project_identity_key(&path)
                        });
                        let can_remove = !worktree.is_main;
                        let remove_visible = self.hovered_worktree_index == Some(index);
                        h_flex()
                            .id(("worktree-row", index))
                            .debug_selector(move || format!("worktree-row-{index}"))
                            .px_2()
                            .py_1()
                            .gap_2()
                            .rounded_md()
                            .cursor_pointer()
                            .when(selected, |row| row.bg(cx.theme().accent.opacity(0.16)))
                            .hover(|row| row.bg(cx.theme().muted))
                            .on_hover(cx.listener(move |sidebar, hovered: &bool, _, cx| {
                                if *hovered {
                                    if sidebar.hovered_worktree_index != Some(index) {
                                        sidebar.hovered_worktree_index = Some(index);
                                        cx.notify();
                                    }
                                } else if sidebar.hovered_worktree_index == Some(index) {
                                    sidebar.hovered_worktree_index = None;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |sidebar, _, _, cx| {
                                sidebar.switch_worktree(path.clone(), cx)
                            }))
                            .child(
                                Icon::new(if selected {
                                    IconName::Check
                                } else {
                                    IconName::Folder
                                })
                                .size_4(),
                            )
                            .child(div().flex_1().min_w_0().truncate().text_xs().child(branch))
                            .when(can_remove && remove_visible, |row| {
                                row.child(
                                    Button::new(("remove-worktree", index))
                                        .debug_selector(move || format!("remove-worktree-{index}"))
                                        .ghost()
                                        .small()
                                        .icon(IconName::Delete)
                                        .tooltip("移除 worktree checkout")
                                        .disabled(self.worktree_busy)
                                        .on_click(cx.listener(move |sidebar, _, window, cx| {
                                            cx.stop_propagation();
                                            sidebar.request_remove_worktree(
                                                remove_path.clone(),
                                                false,
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                            })
                    });
                v_flex()
                    .debug_selector(|| "worktree-switcher".into())
                    .gap_1()
                    .child(
                        Button::new("toggle-worktree-manager")
                            .small()
                            .ghost()
                            .icon(worktree_disclosure_icon(self.worktree_expanded))
                            .label("Worktree")
                            .tooltip("展开或收起 worktree 管理")
                            .on_click(cx.listener(|sidebar, _, _, cx| {
                                sidebar.worktree_expanded = !sidebar.worktree_expanded;
                                cx.notify();
                            })),
                    )
                    .when(self.worktree_expanded, |manager| {
                        manager
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(Input::new(&self.worktree_input)),
                                    )
                                    .child(
                                        Button::new("create-worktree")
                                            .small()
                                            .primary()
                                            .label(if self.worktree_busy {
                                                "处理中…"
                                            } else {
                                                "新建"
                                            })
                                            .tooltip("按分支创建 worktree")
                                            .disabled(self.worktree_busy)
                                            .on_click(cx.listener(Self::create_worktree)),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .debug_selector(|| "worktree-rows-scroll".into())
                                    .max_h(px(220.))
                                    .overflow_y_scrollbar()
                                    .children(rows),
                            )
                    })
            });
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
                    .when_some(worktree_controls, |view, controls| view.child(controls))
                    .when_some(
                        self.worktree_error
                            .as_deref()
                            .map(worktree_error_presentation),
                        |view, error| {
                            view.child(
                                div()
                                    .text_xs()
                                    .text_color(if error.warning {
                                        cx.theme().warning
                                    } else {
                                        cx.theme().muted_foreground
                                    })
                                    .child(error.text),
                            )
                        },
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

/// 会话行元信息，拆成两行返回。
///
/// 规范 S-8 限制一行最多 3 个片段，原来的单行把「运行中 · 时间 · 消息数 · token · 花费 · 分支」
/// 六段挤在一起，窄侧栏里必然截断。这里第一行放时间性信息，第二行放用量，分支交给 tooltip。
fn format_metrics(session: &SessionView) -> (String, String) {
    let running = if session.running { "运行中 · " } else { "" };
    let primary = format!(
        "{running}{} · {} 条消息",
        format_relative_time(session.modified),
        session.message_count,
    );
    let token = session
        .metrics
        .recent_context_tokens
        .map(format_tokens)
        .unwrap_or_else(|| "— token".to_owned());
    let branch = session
        .branch
        .as_deref()
        .map(|branch| format!(" · {}", truncate_text(branch, 16)))
        .unwrap_or_default();
    let secondary = format!("{token} · ${:.4}{branch}", session.metrics.cumulative_cost);
    (primary, secondary)
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

const fn remove_worktree_dialog_copy(force: bool) -> (&'static str, &'static str) {
    if force {
        (
            "强制移除 dirty worktree？",
            "将丢弃此 checkout 的未提交改动；目录链接仍会被安全门禁拒绝。",
        )
    } else {
        ("移除 worktree？", "只移除 checkout 目录，保留分支。")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorktreeErrorPresentation {
    text: String,
    warning: bool,
}

/// “非 Git 仓库”是当前目录能力提示，不应以告警状态呈现；其余执行失败保留诊断原文。
fn worktree_error_presentation(error: &str) -> WorktreeErrorPresentation {
    let normalized = error.to_ascii_lowercase();
    let is_non_repository = normalized.contains("not a git repository")
        || error.contains("不是 Git 仓库")
        || error.contains("不是 git 仓库");
    if is_non_repository {
        WorktreeErrorPresentation {
            text: "当前目录不是 Git 仓库".to_owned(),
            warning: false,
        }
    } else {
        WorktreeErrorPresentation {
            text: format!("Worktree：{error}"),
            warning: true,
        }
    }
}

fn worktree_disclosure_icon(expanded: bool) -> IconName {
    if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
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

    use gpui::{Modifiers, TestAppContext, VisualTestContext, point, size};
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
            hovered_id: None,
            browsing_cwd: Some(PathBuf::from("C:/fixture/project")),
            worktrees: None,
            worktree_error: None,
            worktree_generation: 0,
            worktree_busy: false,
            worktree_expanded: false,
            hovered_worktree_index: None,
            worktree_input: cx.new(|cx| InputState::new(window, cx)),
            probe: None,
        }
    }

    #[test]
    fn worktree_error_presentation_distinguishes_non_repository_from_failures() {
        let non_repository = worktree_error_presentation(
            "fatal: not a git repository (or any of the parent directories): .git",
        );
        assert_eq!(non_repository.text, "当前目录不是 Git 仓库");
        assert!(!non_repository.warning);

        for error in ["git command timed out", "failed to spawn git.exe"] {
            let failure = worktree_error_presentation(error);
            assert!(failure.warning);
            assert!(failure.text.contains(error));
        }
    }

    #[gpui::test]
    fn stale_worktree_generation_and_cwd_do_not_replace_newer_state(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        cx.open_window(size(px(320.), px(560.)), move |window, cx| {
            let entity = cx.new(|cx| ready_sidebar(window, cx));
            *output.borrow_mut() = Some(entity.clone());
            Root::new(entity, window, cx)
        });
        let sidebar = captured.borrow().clone().unwrap();
        sidebar.update(cx, |sidebar, _| {
            sidebar.worktree_generation = 2;
            sidebar.browsing_cwd = Some(PathBuf::from("C:/fixture/b"));
            let stale = pi_data::WorktreeSnapshot {
                project_root: PathBuf::from("C:/fixture/a"),
                is_top_level: true,
                worktrees: Vec::new(),
            };
            assert!(!sidebar.finish_worktree_refresh(
                1,
                &pi_data::project_identity_key(Path::new("C:/fixture/a")),
                Ok(stale.clone()),
            ));
            assert!(!sidebar.finish_worktree_refresh(
                2,
                &pi_data::project_identity_key(Path::new("C:/fixture/a")),
                Ok(stale),
            ));
            assert!(sidebar.worktrees.is_none());
        });
    }

    fn worktree_sidebar(window: &mut Window, cx: &mut Context<SessionSidebar>) -> SessionSidebar {
        let mut sidebar = ready_sidebar(window, cx);
        sidebar.worktree_expanded = true;
        sidebar.worktrees = Some(pi_data::WorktreeSnapshot {
            project_root: PathBuf::from("C:/fixture/project"),
            is_top_level: true,
            worktrees: vec![
                pi_data::WorktreeInfo {
                    path: PathBuf::from("C:/fixture/project"),
                    branch: Some("main".to_owned()),
                    is_main: true,
                    is_current: false,
                },
                pi_data::WorktreeInfo {
                    path: PathBuf::from("C:/fixture/project-worktrees/feature"),
                    branch: Some("feature/visual".to_owned()),
                    is_main: false,
                    is_current: true,
                },
            ],
        });
        sidebar
    }

    struct WorktreeDialogHarness {
        sidebar: gpui::Entity<SessionSidebar>,
    }

    impl Render for WorktreeDialogHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.sidebar.clone())
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    #[gpui::test]
    fn worktree_remove_action_is_hover_only(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let handle = cx.open_window(size(px(320.), px(560.)), |window, cx| {
            let sidebar = cx.new(|cx| worktree_sidebar(window, cx));
            Root::new(sidebar, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        draw(&mut visual, 3);
        assert!(
            visual.debug_bounds("remove-worktree-0").is_none(),
            "main worktree 永远不能渲染删除按钮"
        );
        assert!(
            visual.debug_bounds("remove-worktree-1").is_none(),
            "linked worktree 删除按钮默认必须隐藏"
        );
        let main = visual
            .debug_bounds("worktree-row-0")
            .expect("main worktree row missing");
        let linked = visual
            .debug_bounds("worktree-row-1")
            .expect("linked worktree row missing");
        visual.simulate_mouse_move(main.center(), None, Modifiers::default());
        draw(&mut visual, 2);
        assert!(
            visual.debug_bounds("remove-worktree-0").is_none(),
            "main worktree hover 时仍不能渲染删除按钮"
        );

        // 先进入 row 0 再向下移动到 row 1，覆盖 row 1 enter 先于 row 0 leave 的事件顺序。
        visual.simulate_mouse_move(linked.center(), None, Modifiers::default());
        draw(&mut visual, 2);
        let remove = visual
            .debug_bounds("remove-worktree-1")
            .expect("linked worktree 删除按钮必须在跨行 hover 后出现");
        assert!(remove.size.width > px(0.) && remove.size.height > px(0.));
        assert!(visual.debug_bounds("remove-worktree-0").is_none());

        visual.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::default());
        draw(&mut visual, 2);
        assert!(
            visual.debug_bounds("remove-worktree-1").is_none(),
            "指针移出 linked 行后删除按钮必须重新隐藏"
        );
    }

    #[gpui::test]
    fn worktree_remove_dialog_renders_footer_for_normal_and_force_copy(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        let handle = cx.open_window(size(px(800.), px(560.)), move |window, cx| {
            let sidebar = cx.new(|cx| worktree_sidebar(window, cx));
            *output.borrow_mut() = Some(sidebar.clone());
            let harness = cx.new(|_| WorktreeDialogHarness { sidebar });
            Root::new(harness, window, cx)
        });
        let sidebar = captured.borrow().clone().unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.request_remove_worktree(
                    PathBuf::from("C:/fixture/project-worktrees/feature"),
                    false,
                    window,
                    cx,
                );
            });
        });
        visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
        draw(&mut visual, 3);
        assert_non_zero_debug_bounds(&mut visual, "cancel-remove-worktree");
        assert_non_zero_debug_bounds(&mut visual, "confirm-remove-worktree");

        visual.update(|window, cx| window.close_dialog(cx));
        visual.update(|window, cx| {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.request_remove_worktree(
                    PathBuf::from("C:/fixture/project-worktrees/feature"),
                    true,
                    window,
                    cx,
                );
            });
        });
        visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
        draw(&mut visual, 3);
        assert_non_zero_debug_bounds(&mut visual, "cancel-remove-worktree");
        assert_non_zero_debug_bounds(&mut visual, "confirm-remove-worktree");
        assert_eq!(
            remove_worktree_dialog_copy(false),
            ("移除 worktree？", "只移除 checkout 目录，保留分支。")
        );
        assert_eq!(
            remove_worktree_dialog_copy(true),
            (
                "强制移除 dirty worktree？",
                "将丢弃此 checkout 的未提交改动；目录链接仍会被安全门禁拒绝。",
            )
        );
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

    const ROW_ACTIONS: [&str; 3] = [
        "rename-fixture-session",
        "export-fixture-session",
        "delete-fixture-session",
    ];

    fn draw(visual: &mut VisualTestContext, frames: usize) {
        for _ in 0..frames {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
    }

    fn assert_non_zero_debug_bounds(visual: &mut VisualTestContext, selector: &'static str) {
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("missing debug selector {selector}"));
        assert!(
            bounds.size.width > px(0.) && bounds.size.height > px(0.),
            "{selector} 必须实际参与布局并具有非零尺寸"
        );
    }

    fn ready_sidebar_window(cx: &mut TestAppContext) -> VisualTestContext {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let handle = cx.open_window(size(px(320.), px(560.)), |window, cx| {
            let sidebar = cx.new(|cx| ready_sidebar(window, cx));
            Root::new(sidebar, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        draw(&mut visual, 3);
        visual
    }

    #[gpui::test]
    fn ready_sidebar_renders_project_row_and_diagnostics(cx: &mut TestAppContext) {
        let mut visual = ready_sidebar_window(cx);
        assert!(visual.debug_bounds("session-sidebar").is_some());
        assert!(visual.debug_bounds("session-diagnostics").is_some());
        let row = visual
            .debug_bounds("session-row")
            .expect("session row missing");
        assert!(row.size.width > px(0.) && row.size.height > px(0.));
    }

    /// T2 ③：低频行操作按规范 S-9 收进 hover 态，hover 前不占位、hover 后可点。
    #[gpui::test]
    fn session_row_actions_are_hover_only(cx: &mut TestAppContext) {
        let mut visual = ready_sidebar_window(cx);
        for action in ROW_ACTIONS {
            assert!(
                visual.debug_bounds(action).is_none(),
                "hover 前不应渲染 {action}"
            );
        }

        let row = visual
            .debug_bounds("session-row")
            .expect("session row missing");
        visual.simulate_mouse_move(row.center(), None, Modifiers::default());
        draw(&mut visual, 2);
        for action in ROW_ACTIONS {
            assert!(
                visual.debug_bounds(action).is_some(),
                "hover 后必须渲染 {action}"
            );
        }

        // 指针移出该行后操作重新收起，避免一屏里每行都挂三个图标。
        visual.simulate_mouse_move(
            point(row.center().x, row.origin.y - px(4.)),
            None,
            Modifiers::default(),
        );
        draw(&mut visual, 2);
        for action in ROW_ACTIONS {
            assert!(
                visual.debug_bounds(action).is_none(),
                "移出行后不应继续渲染 {action}"
            );
        }
    }

    #[test]
    fn worktree_disclosure_icon_tracks_expansion() {
        assert!(matches!(
            worktree_disclosure_icon(false),
            IconName::ChevronRight
        ));
        assert!(matches!(
            worktree_disclosure_icon(true),
            IconName::ChevronDown
        ));
    }

    #[test]
    fn metrics_split_into_two_short_lines() {
        let session = SessionView {
            id: "s".to_owned(),
            path: PathBuf::from("s.jsonl"),
            cwd: PathBuf::from("C:/fixture"),
            title: "t".to_owned(),
            modified: SystemTime::now(),
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
        let (primary, secondary) = format_metrics(&session);
        // 规范 S-8：每行最多 3 个片段，原来的单行是 6 段。
        assert_eq!(primary, "运行中 · 刚刚 · 3 条消息");
        assert_eq!(secondary, "42.0k token · $0.1250 · feature/fixture");
        assert_eq!(primary.matches(" · ").count(), 2);
        assert_eq!(secondary.matches(" · ").count(), 2);

        let plain = SessionView {
            branch: None,
            running: false,
            ..session
        };
        let (primary, secondary) = format_metrics(&plain);
        assert_eq!(primary, "刚刚 · 3 条消息");
        assert_eq!(secondary, "42.0k token · $0.1250");
    }
}
