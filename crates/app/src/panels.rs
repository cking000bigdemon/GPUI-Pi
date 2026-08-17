use std::sync::Arc;

use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, ElementExt as _, Icon, IconName, StyledExt as _,
    dock::{Panel, PanelControl, PanelEvent},
    v_flex,
};
use pi_render::ConversationDocument;

use crate::session_sidebar::SessionSelected;

pub struct ChatPanel {
    focus_handle: FocusHandle,
    status: ChatStatus,
    load_generation: u64,
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
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            status: ChatStatus::Empty,
            load_generation: 0,
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
        self.status = ChatStatus::Loading {
            title: selection.title.clone(),
        };
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
            Ok(document) if document.messages.is_empty() => ChatStatus::Empty,
            Ok(document) => ChatStatus::Ready(document),
            Err(message) => ChatStatus::Error { title, message },
        };
        true
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
        let content = match &self.status {
            ChatStatus::Empty => centered_state(
                IconName::Bot,
                "选择一个历史会话",
                "会话将只读加载，不会启动 pi RPC 进程",
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
                gpui_pi_ui::ChatWindow::new(document.clone()).into_any_element()
            }
        };
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
            .child(content)
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
                let mut panel = ChatPanel::new(cx);
                panel.status = status;
                panel
            });
            Root::new(panel, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        visual
    }

    #[gpui::test]
    fn empty_chat_renders_state_selector(cx: &mut TestAppContext) {
        let mut empty = render_status(cx, ChatStatus::Empty);
        assert!(empty.debug_bounds("chat-empty-or-loading").is_some());
    }

    #[gpui::test]
    fn loading_chat_renders_state_selector(cx: &mut TestAppContext) {
        let mut loading = render_status(
            cx,
            ChatStatus::Loading {
                title: "fixture".to_owned(),
            },
        );
        assert!(loading.debug_bounds("chat-empty-or-loading").is_some());
    }

    #[gpui::test]
    fn ready_chat_renders_primary_r6_selectors(cx: &mut TestAppContext) {
        let mut ready = render_status(cx, ChatStatus::Ready(rich_document()));
        for selector in [
            "chat-window",
            "chat-message",
            "chat-minimap",
            "chat-minimap-node",
            "frontmatter-card",
            "image-placeholder",
            "code-block",
            "mermaid-source",
            "tool-card",
            "diff-block",
            "ansi-output",
        ] {
            assert!(
                ready.debug_bounds(selector).is_some(),
                "missing selector {selector}"
            );
        }
    }

    #[gpui::test]
    fn error_chat_renders_error_selector(cx: &mut TestAppContext) {
        let mut error = render_status(
            cx,
            ChatStatus::Error {
                title: "fixture".to_owned(),
                message: "broken".to_owned(),
            },
        );
        assert!(error.debug_bounds("chat-error").is_some());
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
            let panel = cx.new(ChatPanel::new);
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
