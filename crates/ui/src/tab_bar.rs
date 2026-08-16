use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div,
};
use gpui_component::tab::{Tab, TabBar};

/// 标题栏中的应用级工作区标签。本轮只提供稳定容器，真实会话标签由 R5 接入。
#[derive(IntoElement)]
pub struct WorkspaceTabBar {
    label: SharedString,
}

impl WorkspaceTabBar {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl RenderOnce for WorkspaceTabBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .debug_selector(|| "workspace-tab-bar".into())
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .child(
                TabBar::new("workspace-tabs")
                    .h_full()
                    .max_width(gpui::px(360.))
                    .selected_index(0)
                    .child(Tab::new().label(self.label)),
            )
    }
}
