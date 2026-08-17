use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    tab::{Tab, TabBar},
    tooltip::Tooltip,
};

/// 标题栏中的应用级工作区标签，只显示短标题，完整 identity 放在 tooltip。
#[derive(IntoElement)]
pub struct WorkspaceTabBar {
    label: SharedString,
    tooltip: Option<SharedString>,
}

impl WorkspaceTabBar {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tooltip: None,
        }
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

impl RenderOnce for WorkspaceTabBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id("workspace-tab-bar-tooltip")
            .debug_selector(|| "workspace-tab-bar".into())
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .when_some(self.tooltip, |this, tooltip| {
                this.tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            })
            .child(
                TabBar::new("workspace-tabs")
                    .h_full()
                    .max_width(gpui::px(360.))
                    .selected_index(0)
                    .child(Tab::new().label(self.label)),
            )
    }
}
