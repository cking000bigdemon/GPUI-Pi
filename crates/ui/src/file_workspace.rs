use std::rc::Rc;

use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    tab::{Tab, TabBar},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContentTab {
    pub id: SharedString,
    pub label: SharedString,
    pub tooltip: SharedString,
    pub closable: bool,
}

impl WorkspaceContentTab {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        tooltip: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            tooltip: tooltip.into(),
            closable: true,
        }
    }

    pub fn fixed(mut self) -> Self {
        self.closable = false;
        self
    }
}

type TabHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// 中心工作区的内容标签：固定保留「对话」，文件标签可关闭。
#[derive(IntoElement)]
pub struct WorkspaceContentTabs {
    tabs: Vec<WorkspaceContentTab>,
    selected_index: usize,
    on_select: Option<TabHandler>,
    on_close: Option<TabHandler>,
}

impl WorkspaceContentTabs {
    pub fn new(tabs: Vec<WorkspaceContentTab>, selected_index: usize) -> Self {
        Self {
            tabs,
            selected_index,
            on_select: None,
            on_close: None,
        }
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn on_close(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for WorkspaceContentTabs {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let on_close = self.on_close.clone();
        let menu_select = self.on_select.clone();
        div()
            .debug_selector(|| "workspace-content-tabs".into())
            .w_full()
            .min_w_0()
            .child(
                TabBar::new("workspace-content-tab-bar")
                    .menu(true)
                    .selected_index(self.selected_index)
                    .on_click(move |index, window, cx| {
                        if let Some(handler) = &menu_select {
                            handler(*index, window, cx);
                        }
                    })
                    .children(self.tabs.into_iter().enumerate().map(|(index, tab)| {
                        let mut item = Tab::new().label(tab.label.clone());
                        if !tab.closable {
                            item = item.aria_label(tab.tooltip.clone());
                        }
                        if tab.closable {
                            let close = on_close.clone();
                            item = item.suffix(
                                Button::new(format!("close-content-tab-{}", tab.id))
                                    .debug_selector(|| "close-file-tab".into())
                                    .ghost()
                                    .small()
                                    .icon(gpui_component::IconName::Close)
                                    .tooltip(format!("{} · 关闭文件", tab.tooltip))
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        if let Some(handler) = &close {
                                            handler(index, window, cx);
                                        }
                                    }),
                            );
                        }
                        item
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_tab_is_fixed_and_file_tabs_are_closable() {
        let chat = WorkspaceContentTab::new("chat", "对话", "对话").fixed();
        let file = WorkspaceContentTab::new("file:a", "a.rs", "src/a.rs");
        assert!(!chat.closable);
        assert!(file.closable);
    }
}
