use std::{path::PathBuf, sync::Arc};

use gpui::{
    App, AppContext as _, Context, EventEmitter, FocusHandle, Focusable, Image, ImageFormat,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelControl, PanelEvent},
    h_flex,
    input::{Editor, EditorState},
    v_flex,
};
use gpui_pi_ui::{WorkspaceContentTab, WorkspaceContentTabs};

use crate::panels::ChatPanel;

const CHAT_TAB_ID: &str = "chat";
const MAX_FILE_TABS: usize = 16;

#[derive(Debug, Clone)]
pub struct OpenFileRequest {
    pub relative_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileTab {
    id: String,
    relative_path: PathBuf,
    label: String,
    generation: u64,
    status: FileTabStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileTabStatus {
    Loading,
    Text(Arc<pi_data::TextFileContent>),
    Image {
        kind: pi_data::ImageKind,
        size: u64,
        image: Arc<Image>,
    },
    Unsupported {
        size: u64,
        reason: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveContent {
    Chat,
    File(String),
}

pub struct MainPanel {
    focus_handle: FocusHandle,
    chat: gpui::Entity<ChatPanel>,
    root: Option<PathBuf>,
    root_generation: u64,
    next_tab_generation: u64,
    tabs: Vec<FileTab>,
    active: ActiveContent,
    editor: gpui::Entity<EditorState>,
    image: Option<Arc<Image>>,
    _open_subscription: Subscription,
}

impl MainPanel {
    pub fn new(
        chat: gpui::Entity<ChatPanel>,
        explorer: &gpui::Entity<crate::file_explorer::FileExplorerPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .folding(true)
                .default_value("")
        });
        let open_subscription = cx.subscribe_in(
            explorer,
            window,
            |panel, _, event: &OpenFileRequest, window, cx| {
                panel.open_file(event.relative_path.clone(), window, cx)
            },
        );
        Self {
            focus_handle: cx.focus_handle(),
            chat,
            root: None,
            root_generation: 0,
            next_tab_generation: 0,
            tabs: Vec::new(),
            active: ActiveContent::Chat,
            editor,
            image: None,
            _open_subscription: open_subscription,
        }
    }

    pub fn set_root(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if same_project_root(self.root.as_deref(), root.as_deref()) {
            return;
        }
        self.root_generation = self.root_generation.wrapping_add(1);
        self.root = root;
        for tab in self.tabs.drain(..) {
            if let FileTabStatus::Image { image, .. } = tab.status {
                image.remove_asset(cx);
            }
        }
        self.active = ActiveContent::Chat;
        self.image = None;
        cx.notify();
    }

    fn open_file(&mut self, relative_path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let id = file_tab_id(&relative_path);
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active = ActiveContent::File(id);
            self.refresh_active_editor(window, cx);
            cx.notify();
            return;
        }
        if self.tabs.len() >= MAX_FILE_TABS {
            let removed = self.tabs.remove(0);
            if let FileTabStatus::Image { image, .. } = removed.status {
                image.remove_asset(cx);
            }
        }
        self.next_tab_generation = self.next_tab_generation.wrapping_add(1);
        let generation = self.next_tab_generation;
        self.tabs.push(FileTab {
            id: id.clone(),
            label: file_label(&relative_path),
            relative_path: relative_path.clone(),
            generation,
            status: FileTabStatus::Loading,
        });
        self.active = ActiveContent::File(id.clone());
        self.image = None;
        cx.notify();
        let root_generation = self.root_generation;
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let files = pi_data::ProjectFiles::open(&root)?;
                    files.read(&relative_path)
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if panel.finish_file_load(root_generation, &id, generation, result) {
                        if panel.active == ActiveContent::File(id.clone()) {
                            panel.refresh_active_editor(window, cx);
                        }
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn finish_file_load(
        &mut self,
        root_generation: u64,
        id: &str,
        generation: u64,
        result: Result<pi_data::FileContent, pi_data::FileAccessError>,
    ) -> bool {
        if root_generation != self.root_generation {
            return false;
        }
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == id && tab.generation == generation)
        else {
            return false;
        };
        tab.status = match result {
            Ok(pi_data::FileContent::Text(content)) => FileTabStatus::Text(Arc::new(content)),
            Ok(pi_data::FileContent::Image(content)) => {
                let kind = content.kind;
                let size = content.size;
                let image = image_format(kind)
                    .map(|format| Arc::new(Image::from_bytes(format, content.bytes)));
                match image {
                    Some(image) => FileTabStatus::Image { kind, size, image },
                    None => FileTabStatus::Error("不支持的图片格式".to_owned()),
                }
            }
            Ok(pi_data::FileContent::Unsupported { size, reason }) => {
                FileTabStatus::Unsupported { size, reason }
            }
            Err(error) => FileTabStatus::Error(error.to_string()),
        };
        true
    }

    fn refresh_active_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_active_editor_without_window();
        if let Some((text, language)) = self.active_text() {
            let editor = self.editor.clone();
            let text = text.to_owned();
            editor.update(cx, |state, cx| {
                state.set_highlighter(language, cx);
                state.set_value(text, window, cx);
            });
        }
    }

    fn refresh_active_editor_without_window(&mut self) {
        self.image = match &self.active_tab().map(|tab| &tab.status) {
            Some(FileTabStatus::Image { image, .. }) => Some(image.clone()),
            _ => None,
        };
    }

    fn active_tab(&self) -> Option<&FileTab> {
        let ActiveContent::File(id) = &self.active else {
            return None;
        };
        self.tabs.iter().find(|tab| &tab.id == id)
    }

    fn active_text(&self) -> Option<(&str, &'static str)> {
        match &self.active_tab()?.status {
            FileTabStatus::Text(content) => Some((&content.text, content.language)),
            _ => None,
        }
    }

    fn select_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index == 0 {
            self.active = ActiveContent::Chat;
            self.image = None;
        } else if let Some(tab) = self.tabs.get(index - 1) {
            self.active = ActiveContent::File(tab.id.clone());
            self.refresh_active_editor(window, cx);
        }
        cx.notify();
    }

    fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file_index) = index.checked_sub(1) else {
            return;
        };
        if file_index >= self.tabs.len() {
            return;
        }
        let was_active = self
            .tabs
            .get(file_index)
            .is_some_and(|tab| self.active == ActiveContent::File(tab.id.clone()));
        let removed = self.tabs.remove(file_index);
        if let FileTabStatus::Image { image, .. } = removed.status {
            image.remove_asset(cx);
        }
        if was_active {
            if self.tabs.is_empty() {
                self.active = ActiveContent::Chat;
                self.image = None;
            } else {
                let next_index = file_index.min(self.tabs.len() - 1);
                self.active = ActiveContent::File(self.tabs[next_index].id.clone());
                self.refresh_active_editor(window, cx);
            }
        }
        cx.notify();
    }

    fn retry_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        let Some(root) = self.root.clone() else {
            return;
        };
        self.next_tab_generation = self.next_tab_generation.wrapping_add(1);
        if let Some(candidate) = self
            .tabs
            .iter_mut()
            .find(|candidate| candidate.id == tab.id)
        {
            candidate.generation = self.next_tab_generation;
            candidate.status = FileTabStatus::Loading;
        }
        let generation = self
            .tabs
            .iter()
            .find(|candidate| candidate.id == tab.id)
            .map_or(0, |candidate| candidate.generation);
        let id = tab.id;
        let relative_path = tab.relative_path;
        let root_generation = self.root_generation;
        self.image = None;
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let files = pi_data::ProjectFiles::open(&root)?;
                    files.read(&relative_path)
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if panel.finish_file_load(root_generation, &id, generation, result) {
                        if panel.active == ActiveContent::File(id.clone()) {
                            panel.refresh_active_editor(window, cx);
                        }
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn tab_models(&self) -> Vec<WorkspaceContentTab> {
        let mut tabs = vec![WorkspaceContentTab::new(CHAT_TAB_ID, "对话", "对话").fixed()];
        tabs.extend(self.tabs.iter().map(|tab| {
            WorkspaceContentTab::new(
                tab.id.clone(),
                tab.label.clone(),
                tab.relative_path.to_string_lossy().into_owned(),
            )
        }));
        tabs
    }

    fn selected_index(&self) -> usize {
        match &self.active {
            ActiveContent::Chat => 0,
            ActiveContent::File(id) => self
                .tabs
                .iter()
                .position(|tab| &tab.id == id)
                .map_or(0, |index| index + 1),
        }
    }

    fn render_file(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(tab) = self.active_tab() else {
            return centered_state(IconName::File, "没有打开的文件", "从右侧文件树选择文件", cx);
        };
        let relative = tab.relative_path.to_string_lossy().into_owned();
        let is_error = matches!(tab.status, FileTabStatus::Error(_));
        let toolbar = h_flex()
            .flex_none()
            .h_8()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(relative),
            )
            .when(is_error, |row| {
                row.child(
                    Button::new("retry-file")
                        .small()
                        .ghost()
                        .label("重试")
                        .on_click(
                            cx.listener(|panel, _, window, cx| panel.retry_active(window, cx)),
                        ),
                )
            });
        let body = match &tab.status {
            FileTabStatus::Loading => centered_state(
                IconName::LoaderCircle,
                "正在后台读取文件…",
                "仅访问当前项目目录",
                cx,
            ),
            FileTabStatus::Text(content) => v_flex()
                .debug_selector(|| "file-text-viewer".into())
                .size_full()
                .child(
                    h_flex()
                        .flex_none()
                        .px_3()
                        .py_1()
                        .gap_3()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(content.language)
                        .child(format!("{} 行", content.lines))
                        .child(format_size(content.size)),
                )
                .child(
                    div().flex_1().min_h_0().child(
                        div()
                            .debug_selector(|| "readonly-file-editor".into())
                            .size_full()
                            .child(
                                Editor::new(&self.editor)
                                    .appearance(false)
                                    .bordered(false)
                                    .readonly(true)
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_xs()
                                    .size_full(),
                            ),
                    ),
                )
                .into_any_element(),
            FileTabStatus::Image { kind, size, .. } => {
                let image = self.image.clone();
                v_flex()
                    .debug_selector(|| "file-image-viewer".into())
                    .size_full()
                    .child(
                        h_flex()
                            .flex_none()
                            .px_3()
                            .py_1()
                            .gap_3()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(kind.mime_type())
                            .child(format_size(*size)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .p_4()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(cx.theme().muted.opacity(0.42))
                            .children(image.map(|image| {
                                img(image)
                                    .debug_selector(|| "file-preview-image".into())
                                    .max_w_full()
                                    .max_h_full()
                            })),
                    )
                    .into_any_element()
            }
            FileTabStatus::Unsupported { size, reason } => centered_state(
                IconName::File,
                "此文件不在原生安全预览范围内",
                format!("{reason} · {}", format_size(*size)),
                cx,
            ),
            FileTabStatus::Error(error) => {
                centered_state(IconName::TriangleAlert, "文件读取失败", error, cx)
            }
        };
        v_flex()
            .size_full()
            .min_h_0()
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

impl EventEmitter<PanelEvent> for MainPanel {}

impl Focusable for MainPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for MainPanel {
    fn panel_name(&self) -> &'static str {
        "gpui-pi-main"
    }

    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some("工作区".into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "工作区"
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

impl Render for MainPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity();
        let select_panel = panel.clone();
        let close_panel = panel.clone();
        let body = match self.active {
            ActiveContent::Chat => self.chat.clone().into_any_element(),
            ActiveContent::File(_) => self.render_file(cx),
        };
        v_flex()
            .id("main-workspace")
            .debug_selector(|| "main-workspace".into())
            .track_focus(&self.focus_handle)
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                WorkspaceContentTabs::new(self.tab_models(), self.selected_index())
                    .on_select(move |index, window, cx| {
                        select_panel.update(cx, |panel, cx| panel.select_tab(index, window, cx));
                    })
                    .on_close(move |index, window, cx| {
                        close_panel.update(cx, |panel, cx| panel.close_tab(index, window, cx));
                    }),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}

fn same_project_root(left: Option<&std::path::Path>, right: Option<&std::path::Path>) -> bool {
    left.map(pi_data::project_identity_key) == right.map(pi_data::project_identity_key)
}

fn file_tab_id(path: &std::path::Path) -> String {
    format!("file:{}", path.to_string_lossy().replace('\\', "/"))
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn image_format(kind: pi_data::ImageKind) -> Option<ImageFormat> {
    match kind {
        pi_data::ImageKind::Png => Some(ImageFormat::Png),
        pi_data::ImageKind::Jpeg => Some(ImageFormat::Jpeg),
        pi_data::ImageKind::Gif => Some(ImageFormat::Gif),
        pi_data::ImageKind::Webp => Some(ImageFormat::Webp),
        pi_data::ImageKind::Bmp => Some(ImageFormat::Bmp),
        pi_data::ImageKind::Ico => Some(ImageFormat::Ico),
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn centered_state(
    icon: IconName,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    cx: &App,
) -> gpui::AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .child(gpui_component::Icon::new(icon).size_6())
        .child(div().font_semibold().child(title.into()))
        .child(
            div()
                .max_w(px(520.))
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(detail.into()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_identity_normalizes_windows_separators() {
        assert_eq!(
            file_tab_id(std::path::Path::new(r"src\main.rs")),
            "file:src/main.rs"
        );
    }

    #[gpui::test]
    fn stale_file_load_is_rejected(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        cx.open_window(gpui::size(px(800.), px(560.)), move |window, cx| {
            let explorer = cx.new(|cx| crate::file_explorer::FileExplorerPanel::new(window, cx));
            let chat = cx.new(|cx| ChatPanel::new(window, cx));
            let panel = cx.new(|cx| MainPanel::new(chat, &explorer, window, cx));
            *output.borrow_mut() = Some(panel.clone());
            gpui_component::Root::new(panel, window, cx)
        });
        let panel = captured.borrow().clone().unwrap();
        panel.update(cx, |panel, _| {
            panel.root_generation = 2;
            panel.tabs.push(FileTab {
                id: "file:a".to_owned(),
                relative_path: PathBuf::from("a"),
                label: "a".to_owned(),
                generation: 3,
                status: FileTabStatus::Loading,
            });
            let result = Ok(pi_data::FileContent::Text(pi_data::TextFileContent {
                text: "old".to_owned(),
                language: "text",
                size: 3,
                lines: 1,
            }));
            assert!(!panel.finish_file_load(1, "file:a", 3, result));
            let result = Ok(pi_data::FileContent::Text(pi_data::TextFileContent {
                text: "old".to_owned(),
                language: "text",
                size: 3,
                lines: 1,
            }));
            assert!(!panel.finish_file_load(2, "file:a", 2, result));
            assert!(matches!(panel.tabs[0].status, FileTabStatus::Loading));
        });
    }

    #[gpui::test]
    fn tab_selection_and_close_keep_chat_offset_correct(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        let window = cx.open_window(gpui::size(px(800.), px(560.)), move |window, cx| {
            let explorer = cx.new(|cx| crate::file_explorer::FileExplorerPanel::new(window, cx));
            let chat = cx.new(|cx| ChatPanel::new(window, cx));
            let panel = cx.new(|cx| MainPanel::new(chat, &explorer, window, cx));
            *output.borrow_mut() = Some(panel.clone());
            gpui_component::Root::new(panel, window, cx)
        });
        let panel = captured.borrow().clone().unwrap();
        let _ = window.update(cx, |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.tabs = vec![
                    FileTab {
                        id: "file:a".to_owned(),
                        relative_path: PathBuf::from("a"),
                        label: "a".to_owned(),
                        generation: 1,
                        status: FileTabStatus::Loading,
                    },
                    FileTab {
                        id: "file:b".to_owned(),
                        relative_path: PathBuf::from("b"),
                        label: "b".to_owned(),
                        generation: 2,
                        status: FileTabStatus::Loading,
                    },
                ];
                panel.select_tab(2, window, cx);
                assert_eq!(panel.active, ActiveContent::File("file:b".to_owned()));
                panel.close_tab(0, window, cx);
                assert_eq!(panel.tabs.len(), 2);
                panel.close_tab(2, window, cx);
                assert_eq!(panel.active, ActiveContent::File("file:a".to_owned()));
                panel.close_tab(1, window, cx);
                assert_eq!(panel.active, ActiveContent::Chat);
            });
        });
    }

    #[test]
    fn format_sizes_are_stable() {
        assert_eq!(format_size(10), "10 B");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MiB");
    }
}
