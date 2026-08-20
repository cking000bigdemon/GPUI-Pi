use std::{collections::HashSet, path::PathBuf, sync::Arc};

use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, Icon, IconName, Sizable as _,
    StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogFooter,
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::ListItem,
    notification::Notification,
    tree::{TreeEntry, TreeItem, TreeState, tree},
    v_flex,
};

use crate::{
    main_panel::{OpenDiffRequest, OpenFileRequest},
    panels::LayoutProbe,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExplorerStatus {
    Empty,
    Loading,
    Ready(Arc<pi_data::FileTreeSnapshot>),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UploadState {
    Idle,
    Selecting,
    AwaitingConflict,
    Publishing,
}

#[derive(Debug, Clone)]
struct UploadContext {
    root_generation: u64,
    upload_generation: u64,
    root: PathBuf,
}

pub struct FileExplorerPanel {
    focus_handle: FocusHandle,
    root: Option<PathBuf>,
    root_generation: u64,
    upload_generation: u64,
    status: ExplorerStatus,
    index: Option<Arc<pi_data::FileIndex>>,
    git_status: Option<Arc<pi_data::GitStatusSnapshot>>,
    git_model: Option<Arc<gpui_pi_ui::GitChangesModel>>,
    git_error: Option<String>,
    directory_ids: Arc<HashSet<String>>,
    tree_state: gpui::Entity<TreeState>,
    search: gpui::Entity<InputState>,
    search_results: Vec<pi_data::FileIndexEntry>,
    upload_state: UploadState,
    expanded_ids: HashSet<String>,
    pending_tree_items: Option<(Vec<TreeItem>, Arc<HashSet<String>>)>,
    probe: Option<LayoutProbe>,
    _search_subscription: Subscription,
    _tree_subscription: Subscription,
}

impl FileExplorerPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("模糊查找文件"));
        let search_subscription = cx.subscribe_in(&search, window, |panel, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                panel.refresh_search(cx);
            }
        });
        let tree_subscription = cx.subscribe(
            &tree_state,
            |panel, _, event: &gpui_component::tree::TreeEvent, _| match event {
                gpui_component::tree::TreeEvent::Expanded(id) => {
                    panel.expanded_ids.insert(id.to_string());
                }
                gpui_component::tree::TreeEvent::Collapsed(id) => {
                    panel.expanded_ids.remove(id.as_str());
                }
            },
        );
        Self {
            focus_handle: cx.focus_handle(),
            root: None,
            root_generation: 0,
            upload_generation: 0,
            status: ExplorerStatus::Empty,
            index: None,
            git_status: None,
            git_model: None,
            git_error: None,
            directory_ids: Arc::new(HashSet::new()),
            tree_state,
            search,
            search_results: Vec::new(),
            upload_state: UploadState::Idle,
            expanded_ids: HashSet::new(),
            pending_tree_items: None,
            probe: None,
            _search_subscription: search_subscription,
            _tree_subscription: tree_subscription,
        }
    }

    #[cfg(test)]
    pub(crate) fn root_for_test(&self) -> Option<&std::path::Path> {
        self.root.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn with_probe(mut self, probe: LayoutProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    pub fn set_root(&mut self, root: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if same_project_root(self.root.as_deref(), root.as_deref()) {
            return;
        }
        self.root_generation = self.root_generation.wrapping_add(1);
        self.upload_generation = self.upload_generation.wrapping_add(1);
        if self.upload_state == UploadState::AwaitingConflict {
            window.close_dialog(cx);
        }
        self.root = root;
        self.index = None;
        self.git_status = None;
        self.git_model = None;
        self.git_error = None;
        self.pending_tree_items = None;
        self.search_results.clear();
        self.upload_state = UploadState::Idle;
        self.expanded_ids.clear();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        if self.root.is_some() {
            self.start_refresh(cx);
        } else {
            self.status = ExplorerStatus::Empty;
            self.directory_ids = Arc::new(HashSet::new());
            self.tree_state
                .update(cx, |state, cx| state.set_items(Vec::new(), cx));
            cx.notify();
        }
    }

    fn start_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let generation = self.root_generation;
        self.status = ExplorerStatus::Loading;
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let files = pi_data::ProjectFiles::open(&root)?;
                    let tree = files.scan_tree()?;
                    let index = files.build_index()?;
                    Ok::<_, pi_data::FileAccessError>((tree, index))
                })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.finish_refresh(generation, result) {
                    panel.start_git_refresh(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn start_git_refresh(&self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let generation = self.root_generation;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |panel, cx| {
            let result = executor
                .spawn(async move { pi_data::git_status(root) })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if generation != panel.root_generation {
                    return;
                }
                match result {
                    Ok(snapshot) => {
                        panel.git_model = Some(Arc::new(git_changes_model(&snapshot)));
                        panel.git_status = Some(Arc::new(snapshot));
                        panel.git_error = None;
                    }
                    Err(error) => {
                        panel.git_status = None;
                        panel.git_model = None;
                        panel.git_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn finish_refresh(
        &mut self,
        generation: u64,
        result: Result<(pi_data::FileTreeSnapshot, pi_data::FileIndex), pi_data::FileAccessError>,
    ) -> bool {
        if generation != self.root_generation {
            return false;
        }
        match result {
            Ok((snapshot, index)) => {
                let mut directory_ids = HashSet::new();
                let items = snapshot
                    .nodes
                    .iter()
                    .map(|node| {
                        file_node_to_tree_item(node, &mut directory_ids, &self.expanded_ids)
                    })
                    .collect::<Vec<_>>();
                self.status = ExplorerStatus::Ready(Arc::new(snapshot));
                self.index = Some(Arc::new(index));
                self.pending_tree_items = Some((items, Arc::new(directory_ids)));
            }
            Err(error) => {
                self.status = ExplorerStatus::Error(error.to_string());
                self.index = None;
                self.pending_tree_items = Some((Vec::new(), Arc::new(HashSet::new())));
            }
        }
        true
    }

    fn apply_pending_tree(&mut self, cx: &mut Context<Self>) {
        if let Some((items, directory_ids)) = self.pending_tree_items.take() {
            self.directory_ids = directory_ids;
            self.tree_state
                .update(cx, |state, cx| state.set_items(items, cx));
            self.refresh_search(cx);
        }
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        self.search_results = self.index.as_ref().map_or_else(Vec::new, |index| {
            let files = index
                .entries
                .iter()
                .filter(|entry| !entry.is_dir)
                .cloned()
                .collect::<Vec<_>>();
            pi_data::filter_file_entries(&files, &query, pi_data::FILE_SEARCH_RESULT_LIMIT)
        });
        cx.notify();
    }

    fn handle_refresh(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.start_refresh(cx);
    }

    fn choose_uploads(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.upload_state != UploadState::Idle || self.root.is_none() {
            return;
        }
        self.upload_generation = self.upload_generation.wrapping_add(1);
        let upload_generation = self.upload_generation;
        self.upload_state = UploadState::Selecting;
        cx.notify();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择要上传到项目根目录的文件".into()),
        });
        let generation = self.root_generation;
        cx.spawn_in(window, async move |panel, cx| {
            let paths = receiver.await.ok().and_then(Result::ok).flatten();
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if generation != panel.root_generation
                        || upload_generation != panel.upload_generation
                    {
                        return;
                    }
                    match paths {
                        Some(paths) if !paths.is_empty() => {
                            panel.preflight_upload(paths, upload_generation, window, cx)
                        }
                        _ => {
                            panel.upload_state = UploadState::Idle;
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    pub fn upload_external_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.upload_state != UploadState::Idle || self.root.is_none() || paths.is_empty() {
            return;
        }
        self.upload_generation = self.upload_generation.wrapping_add(1);
        self.preflight_upload(paths, self.upload_generation, window, cx);
    }

    fn preflight_upload(
        &mut self,
        paths: Vec<PathBuf>,
        upload_generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            self.upload_state = UploadState::Idle;
            return;
        };
        let generation = self.root_generation;
        let inspect_root = root.clone();
        self.upload_state = UploadState::Publishing;
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let files = pi_data::ProjectFiles::open(&inspect_root)?;
                    files.inspect_upload(paths, "")
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if generation != panel.root_generation
                        || upload_generation != panel.upload_generation
                    {
                        return;
                    }
                    match result {
                        Ok(inspection) if inspection.conflicts.is_empty() => {
                            panel.upload_state = UploadState::Idle;
                            panel.publish_upload(
                                UploadContext {
                                    root_generation: generation,
                                    upload_generation,
                                    root: root.clone(),
                                },
                                inspection,
                                pi_data::UploadConflictStrategy::Error,
                                window,
                                cx,
                            );
                        }
                        Ok(inspection) => panel.confirm_conflicts(
                            UploadContext {
                                root_generation: generation,
                                upload_generation,
                                root: root.clone(),
                            },
                            inspection,
                            window,
                            cx,
                        ),
                        Err(error) => {
                            panel.upload_state = UploadState::Idle;
                            window.push_notification(
                                Notification::error(format!("上传预检失败：{error}")),
                                cx,
                            );
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    fn confirm_conflicts(
        &mut self,
        upload: UploadContext,
        inspection: pi_data::UploadInspection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.upload_state = UploadState::AwaitingConflict;
        cx.notify();
        let replace_inspection = inspection.clone();
        let skip_inspection = inspection.clone();
        let replace_upload = upload.clone();
        let skip_upload = upload.clone();
        let panel = cx.entity();
        let close_panel = panel.clone();
        let replace_panel = panel.clone();
        let skip_panel = panel.clone();
        let conflicts = inspection.conflicts.join("、");
        let non_replaceable = inspection.non_replaceable.join("、");
        window.open_dialog(cx, move |dialog, _, _| {
            let skip_panel = skip_panel.clone();
            let skip_inspection = skip_inspection.clone();
            let skip_upload = skip_upload.clone();
            let replace_panel = replace_panel.clone();
            let replace_inspection = replace_inspection.clone();
            let replace_upload = replace_upload.clone();
            dialog
                .title("上传文件已存在")
                .overlay_closable(false)
                .keyboard(false)
                .close_button(false)
                .on_close({
                    let close_panel = close_panel.clone();
                    move |_, _, cx| {
                        close_panel.update(cx, |panel, cx| {
                            if panel.upload_state == UploadState::AwaitingConflict {
                                panel.upload_state = UploadState::Idle;
                                cx.notify();
                            }
                        });
                    }
                })
                .child(
                    v_flex()
                        .gap_2()
                        .child(format!("冲突：{conflicts}"))
                        .when(!non_replaceable.is_empty(), |view| {
                            view.child(format!("不能覆盖：{non_replaceable}"))
                        }),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel-upload-conflict")
                                .secondary()
                                .label("取消")
                                .on_click({
                                    let cancel_panel = panel.clone();
                                    move |_, window, cx| {
                                        cancel_panel.update(cx, |panel, cx| {
                                            panel.upload_state = UploadState::Idle;
                                            cx.notify();
                                        });
                                        window.close_dialog(cx);
                                    }
                                }),
                        )
                        .child(
                            Button::new("skip-upload-conflict")
                                .secondary()
                                .label("跳过已有")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    skip_panel.update(cx, |panel, cx| {
                                        panel.upload_state = UploadState::Idle;
                                        panel.publish_upload(
                                            skip_upload.clone(),
                                            skip_inspection.clone(),
                                            pi_data::UploadConflictStrategy::Skip,
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            Button::new("replace-upload-conflict")
                                .danger()
                                .label("覆盖普通文件")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    replace_panel.update(cx, |panel, cx| {
                                        panel.upload_state = UploadState::Idle;
                                        panel.publish_upload(
                                            replace_upload.clone(),
                                            replace_inspection.clone(),
                                            pi_data::UploadConflictStrategy::Overwrite,
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        ),
                )
        });
    }

    fn publish_upload(
        &mut self,
        upload: UploadContext,
        inspection: pi_data::UploadInspection,
        strategy: pi_data::UploadConflictStrategy,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.upload_state != UploadState::Idle
            || upload.root_generation != self.root_generation
            || upload.upload_generation != self.upload_generation
            || self.root.as_ref() != Some(&upload.root)
        {
            return;
        }
        self.upload_state = UploadState::Publishing;
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let files = pi_data::ProjectFiles::open(&upload.root)?;
                    files.upload(&inspection, "", strategy)
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if upload.root_generation != panel.root_generation
                        || upload.upload_generation != panel.upload_generation
                    {
                        return;
                    }
                    panel.upload_state = UploadState::Idle;
                    match result {
                        Ok(report) => {
                            if report.errors.is_empty() {
                                window.push_notification(
                                    Notification::success(format!(
                                        "上传完成：{} 个成功，{} 个跳过",
                                        report.uploaded.len(),
                                        report.skipped.len()
                                    )),
                                    cx,
                                );
                            } else {
                                let details = report
                                    .errors
                                    .iter()
                                    .take(3)
                                    .map(|item| format!("{}：{}", item.name, item.error))
                                    .collect::<Vec<_>>()
                                    .join("；");
                                let remaining = report.errors.len().saturating_sub(3);
                                window.push_notification(
                                    Notification::warning(format!(
                                        "上传部分完成：{} 个成功，{} 个跳过，{} 个失败。{}{}",
                                        report.uploaded.len(),
                                        report.skipped.len(),
                                        report.errors.len(),
                                        details,
                                        if remaining > 0 {
                                            format!("；另有 {remaining} 个失败")
                                        } else {
                                            String::new()
                                        }
                                    )),
                                    cx,
                                );
                            }
                            panel.start_refresh(cx);
                            panel.start_git_refresh(cx);
                        }
                        Err(error) => window.push_notification(
                            Notification::error(format!("上传失败：{error}")),
                            cx,
                        ),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn open_relative(&mut self, relative_path: PathBuf, cx: &mut Context<Self>) {
        if let Some(source_root) = self.root.clone() {
            cx.emit(OpenFileRequest {
                source_root,
                relative_path,
            });
        }
    }

    fn open_diff(&mut self, relative_path: PathBuf, cx: &mut Context<Self>) {
        if let Some(source_root) = self.root.clone() {
            cx.emit(OpenDiffRequest {
                source_root,
                relative_path,
            });
        }
    }

    fn render_search_results(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.search_results.is_empty() {
            return div()
                .px_2()
                .py_3()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("没有匹配文件")
                .into_any_element();
        }
        v_flex()
            .gap_1()
            .children(
                self.search_results
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let relative = PathBuf::from(&entry.path);
                        let open_relative = relative.clone();
                        let is_dir = entry.is_dir;
                        let display_path = entry.path.clone();
                        div()
                            .id(format!("file-search-result-{index}"))
                            .debug_selector(|| "file-search-result".into())
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|row| row.bg(cx.theme().muted))
                            .on_click(cx.listener(move |panel, _, _, cx| {
                                if !is_dir {
                                    panel.open_relative(open_relative.clone(), cx);
                                }
                            }))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Icon::new(if is_dir {
                                            IconName::Folder
                                        } else {
                                            IconName::File
                                        })
                                        .small(),
                                    )
                                    .child(
                                        div().min_w_0().text_xs().truncate().child(display_path),
                                    ),
                            )
                    }),
            )
            .into_any_element()
    }

    fn render_tree(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let panel = cx.entity();
        let directory_ids = self.directory_ids.clone();
        tree(
            &self.tree_state,
            move |index, entry: &TreeEntry, _, _, _cx| {
                let item = entry.item().clone();
                let relative = PathBuf::from(item.id.as_str());
                let is_dir = directory_ids.contains(item.id.as_str());
                let icon = if is_dir {
                    if entry.is_expanded() {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    }
                } else {
                    IconName::File
                };
                ListItem::new(index)
                    .debug_selector(|| "file-tree-row".into())
                    .w_full()
                    .px_2()
                    .pl(px(12.) * entry.depth() + px(8.))
                    .rounded_md()
                    .child(
                        h_flex().gap_2().child(Icon::new(icon).small()).child(
                            div()
                                .min_w_0()
                                .text_xs()
                                .truncate()
                                .child(item.label.clone()),
                        ),
                    )
                    .on_click({
                        let panel = panel.clone();
                        move |_, _, cx| {
                            if !is_dir {
                                panel.update(cx, |panel, cx| {
                                    panel.open_relative(relative.clone(), cx);
                                });
                            }
                        }
                    })
            },
        )
        .size_full()
        .into_any_element()
    }
}

impl EventEmitter<PanelEvent> for FileExplorerPanel {}
impl EventEmitter<OpenFileRequest> for FileExplorerPanel {}
impl EventEmitter<OpenDiffRequest> for FileExplorerPanel {}

impl Focusable for FileExplorerPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for FileExplorerPanel {
    fn panel_name(&self) -> &'static str {
        "gpui-pi-files"
    }

    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("文件".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "文件"
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

impl Render for FileExplorerPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_tree(cx);
        let query = self.search.read(cx).value();
        let upload_busy = self.upload_state != UploadState::Idle;
        let body = if !query.is_empty() {
            self.render_search_results(cx)
        } else {
            match &self.status {
                ExplorerStatus::Empty => centered("选择项目后浏览文件", cx),
                ExplorerStatus::Loading => centered("正在后台扫描文件…", cx),
                ExplorerStatus::Error(error) => v_flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .p_4()
                    .text_color(cx.theme().danger)
                    .child("文件扫描失败")
                    .child(error.clone())
                    .into_any_element(),
                ExplorerStatus::Ready(snapshot) if snapshot.nodes.is_empty() => {
                    centered("项目目录为空", cx)
                }
                ExplorerStatus::Ready(_) => self.render_tree(cx),
            }
        };
        let git_changes = self.git_model.clone().map(|model| {
            let panel = cx.entity();
            gpui_pi_ui::GitChangesView::new(model).on_open_diff(move |path, cx| {
                panel.update(cx, |panel, cx| panel.open_diff(path, cx));
            })
        });
        let diagnostic = match &self.status {
            ExplorerStatus::Ready(snapshot) => {
                let mut parts = Vec::new();
                if snapshot.truncated {
                    parts.push("结果已截断".to_owned());
                }
                if snapshot.skipped_links > 0 {
                    parts.push(format!("跳过 {} 个目录链接", snapshot.skipped_links));
                }
                if snapshot.skipped_unreadable > 0 {
                    parts.push(format!("跳过 {} 个不可读目录", snapshot.skipped_unreadable));
                }
                if self.index.as_ref().is_some_and(|index| index.truncated) {
                    parts.push("文件索引已截断，搜索结果可能不完整".to_owned());
                }
                (!parts.is_empty()).then(|| parts.join(" · "))
            }
            _ => None,
        };
        #[cfg(test)]
        let probe = self.probe.clone();
        #[cfg(not(test))]
        let probe = self.probe;
        v_flex()
            .id("file-explorer-panel")
            .when_some(probe, |view, probe| {
                view.on_prepaint(move |bounds, _, _| probe.record_files(bounds))
            })
            .debug_selector(|| "file-explorer-panel".into())
            .on_drop(
                cx.listener(|panel, paths: &gpui::ExternalPaths, window, cx| {
                    panel.upload_external_paths(paths.paths().to_vec(), window, cx);
                }),
            )
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h_0()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .flex_none()
                    .h_8()
                    .px_2()
                    .gap_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_lg()
                            .font_semibold()
                            .child("项目文件"),
                    )
                    .child(
                        Button::new("upload-project-files")
                            .debug_selector(|| "upload-project-files".into())
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("上传文件到项目根目录")
                            .disabled(upload_busy || self.root.is_none())
                            .on_click(cx.listener(Self::choose_uploads)),
                    )
                    .child(
                        Button::new("refresh-project-files")
                            .ghost()
                            .small()
                            .icon(IconName::Redo)
                            .tooltip("刷新文件树")
                            .disabled(self.root.is_none())
                            .on_click(cx.listener(Self::handle_refresh)),
                    ),
            )
            .when_some(git_changes, |view, changes| view.child(changes))
            .when_some(self.git_error.clone(), |view, error| {
                view.child(
                    div()
                        .debug_selector(|| "git-status-error".into())
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(format!("Git 状态读取失败：{error}")),
                )
            })
            .child(
                div()
                    .debug_selector(|| "file-search-input".into())
                    .flex_none()
                    .p_2()
                    .child(Input::new(&self.search).prefix(IconName::Search)),
            )
            .when_some(diagnostic, |view, diagnostic| {
                view.child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(diagnostic),
                )
            })
            .when(upload_busy, |view| {
                view.child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(match self.upload_state {
                            UploadState::Selecting => "正在选择文件…",
                            UploadState::AwaitingConflict => "等待处理同名文件冲突…",
                            UploadState::Publishing => "正在安全发布文件…",
                            UploadState::Idle => "",
                        }),
                )
            })
            .child(div().flex_1().min_h_0().child(body))
    }
}

fn git_changes_model(snapshot: &pi_data::GitStatusSnapshot) -> gpui_pi_ui::GitChangesModel {
    gpui_pi_ui::GitChangesModel {
        is_git_repository: snapshot.is_git_repository,
        files: snapshot
            .files
            .iter()
            .map(|file| gpui_pi_ui::GitChangeItem {
                relative_path: file.relative_path.clone(),
                kind: match file.kind {
                    pi_data::GitFileStatusKind::Modified => gpui_pi_ui::GitChangeKind::Modified,
                    pi_data::GitFileStatusKind::Added => gpui_pi_ui::GitChangeKind::Added,
                    pi_data::GitFileStatusKind::Deleted => gpui_pi_ui::GitChangeKind::Deleted,
                    pi_data::GitFileStatusKind::Renamed => gpui_pi_ui::GitChangeKind::Renamed,
                    pi_data::GitFileStatusKind::Untracked => gpui_pi_ui::GitChangeKind::Untracked,
                    pi_data::GitFileStatusKind::Conflict => gpui_pi_ui::GitChangeKind::Conflict,
                },
            })
            .collect(),
        total_files: snapshot.total_files,
        files_truncated: snapshot.files_truncated,
        additions: snapshot.additions,
        deletions: snapshot.deletions,
        line_stats_truncated: snapshot.line_stats_truncated,
    }
}

fn same_project_root(left: Option<&std::path::Path>, right: Option<&std::path::Path>) -> bool {
    left.map(pi_data::project_identity_key) == right.map(pi_data::project_identity_key)
}

fn file_node_to_tree_item(
    node: &pi_data::FileNode,
    directory_ids: &mut HashSet<String>,
    expanded_ids: &HashSet<String>,
) -> TreeItem {
    let id = node.relative_path.to_string_lossy().replace('\\', "/");
    let item = TreeItem::new(id.clone(), node.name.clone());
    if node.is_dir() {
        directory_ids.insert(id.clone());
        item.expanded(expanded_ids.contains(&id)).children(
            node.children
                .iter()
                .map(|child| file_node_to_tree_item(child, directory_ids, expanded_ids)),
        )
    } else {
        item
    }
}

fn centered(message: impl Into<SharedString>, cx: &App) -> gpui::AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .p_4()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(message.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_panel(window: &mut Window, cx: &mut Context<FileExplorerPanel>) -> FileExplorerPanel {
        let mut panel = FileExplorerPanel::new(window, cx);
        panel.root = Some(PathBuf::from("C:/fixture/project"));
        let snapshot = pi_data::FileTreeSnapshot {
            nodes: vec![pi_data::FileNode {
                name: "visible.txt".to_owned(),
                relative_path: PathBuf::from("visible.txt"),
                kind: pi_data::FileNodeKind::File,
                children: Vec::new(),
            }],
            ..Default::default()
        };
        let mut directory_ids = HashSet::new();
        let items = snapshot
            .nodes
            .iter()
            .map(|node| file_node_to_tree_item(node, &mut directory_ids, &HashSet::new()))
            .collect();
        panel.pending_tree_items = Some((items, Arc::new(directory_ids)));
        panel.status = ExplorerStatus::Ready(Arc::new(snapshot));
        panel
    }

    #[test]
    fn git_changes_model_preserves_empty_non_git_and_truncation_states() {
        let snapshot = pi_data::GitStatusSnapshot {
            is_git_repository: false,
            repository_root: None,
            files: Vec::new(),
            total_files: 0,
            files_truncated: false,
            additions: 0,
            deletions: 0,
            line_stats_truncated: false,
        };
        let model = git_changes_model(&snapshot);
        assert!(!model.is_git_repository);
        assert!(model.files.is_empty());

        let snapshot = pi_data::GitStatusSnapshot {
            is_git_repository: true,
            repository_root: Some(PathBuf::from("C:/repo")),
            files: vec![pi_data::GitFileStatus {
                relative_path: PathBuf::from("a.txt"),
                original_path: None,
                kind: pi_data::GitFileStatusKind::Modified,
                index_status: ' ',
                worktree_status: 'M',
            }],
            total_files: 501,
            files_truncated: true,
            additions: 1,
            deletions: 2,
            line_stats_truncated: true,
        };
        let model = git_changes_model(&snapshot);
        assert!(model.files_truncated);
        assert_eq!(model.total_files, 501);
        assert_eq!(model.files[0].relative_path, PathBuf::from("a.txt"));
    }

    #[gpui::test]
    fn explorer_renders_tree_search_and_upload_picker(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let handle = cx.open_window(gpui::size(px(360.), px(560.)), |window, cx| {
            let panel = cx.new(|cx| ready_panel(window, cx));
            gpui_component::Root::new(panel, window, cx)
        });
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..4 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("file-explorer-panel").is_some());
        assert!(visual.debug_bounds("file-tree-row").is_some());
        let upload = visual
            .debug_bounds("upload-project-files")
            .expect("upload button missing");
        visual.simulate_click(upload.center(), Default::default());
        assert!(visual.did_prompt_for_paths());
        visual.simulate_path_prompt_response(|options| {
            assert!(options.files);
            assert!(!options.directories);
            assert!(options.multiple);
            None
        });
    }

    #[gpui::test]
    fn search_filters_index_and_emits_only_file_results(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let handle = cx.open_window(gpui::size(px(360.), px(560.)), |window, cx| {
            let panel = cx.new(|cx| {
                let mut panel = ready_panel(window, cx);
                panel.index = Some(Arc::new(pi_data::FileIndex {
                    entries: vec![
                        pi_data::FileIndexEntry {
                            path: "src".to_owned(),
                            is_dir: true,
                        },
                        pi_data::FileIndexEntry {
                            path: "src/main.rs".to_owned(),
                            is_dir: false,
                        },
                    ],
                    truncated: false,
                }));
                panel
            });
            gpui_component::Root::new(panel, window, cx)
        });
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..4 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        let input = visual
            .debug_bounds("file-search-input")
            .expect("search input missing");
        visual.simulate_click(input.center(), Default::default());
        visual.simulate_input("main");
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("file-search-result").is_some());
    }

    #[test]
    fn project_root_comparison_uses_windows_identity_rules() {
        assert!(same_project_root(
            Some(std::path::Path::new(r"C:\Work\Demo")),
            Some(std::path::Path::new(r"c:/work/demo/")),
        ));
        assert!(!same_project_root(
            Some(std::path::Path::new(r"C:\Work\Demo")),
            Some(std::path::Path::new(r"C:\Work\Other")),
        ));
    }

    #[test]
    fn upload_error_summary_keeps_actionable_details() {
        let report = pi_data::UploadReport {
            uploaded: vec!["ok.txt".to_owned()],
            skipped: Vec::new(),
            errors: vec![pi_data::UploadItemError {
                name: "bad.txt".to_owned(),
                error: "拒绝覆盖目录".to_owned(),
            }],
        };
        let details = report
            .errors
            .iter()
            .take(3)
            .map(|item| format!("{}：{}", item.name, item.error))
            .collect::<Vec<_>>()
            .join("；");
        assert_eq!(details, "bad.txt：拒绝覆盖目录");
    }

    #[test]
    fn tree_conversion_preserves_relative_ids_and_folders() {
        let node = pi_data::FileNode {
            name: "src".to_owned(),
            relative_path: PathBuf::from("src"),
            kind: pi_data::FileNodeKind::Directory,
            children: vec![pi_data::FileNode {
                name: "main.rs".to_owned(),
                relative_path: PathBuf::from("src/main.rs"),
                kind: pi_data::FileNodeKind::File,
                children: Vec::new(),
            }],
        };
        let mut directory_ids = HashSet::new();
        let item = file_node_to_tree_item(&node, &mut directory_ids, &HashSet::new());
        assert_eq!(item.id.as_str(), "src");
        assert!(item.is_folder());
        assert!(directory_ids.contains("src"));
        assert_eq!(item.children[0].id.as_str(), "src/main.rs");

        let empty = pi_data::FileNode {
            name: "empty".to_owned(),
            relative_path: PathBuf::from("empty"),
            kind: pi_data::FileNodeKind::Directory,
            children: Vec::new(),
        };
        let item = file_node_to_tree_item(&empty, &mut directory_ids, &HashSet::new());
        assert!(!item.is_folder());
        assert!(directory_ids.contains("empty"));
    }
}
