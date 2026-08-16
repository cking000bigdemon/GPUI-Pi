use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    Styled as _, Window, div,
};
use gpui_component::ActiveTheme as _;

/// 主窗口的稳定外壳：标题栏固定在顶端，Dock 内容占据剩余空间。
#[derive(IntoElement)]
pub struct AppShell {
    title_bar: AnyElement,
    toolbar: AnyElement,
    body: AnyElement,
}

impl AppShell {
    pub fn new(
        title_bar: impl IntoElement,
        toolbar: impl IntoElement,
        body: impl IntoElement,
    ) -> Self {
        Self {
            title_bar: title_bar.into_any_element(),
            toolbar: toolbar.into_any_element(),
            body: body.into_any_element(),
        }
    }
}

impl RenderOnce for AppShell {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .debug_selector(|| "app-shell".into())
            .font_family(cx.theme().font_family.clone())
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .debug_selector(|| "app-title-bar".into())
                    .flex_none()
                    .child(self.title_bar),
            )
            .child(
                div()
                    .debug_selector(|| "app-toolbar".into())
                    .flex_none()
                    .child(self.toolbar),
            )
            .child(
                div()
                    .debug_selector(|| "app-dock-body".into())
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.body),
            )
    }
}
