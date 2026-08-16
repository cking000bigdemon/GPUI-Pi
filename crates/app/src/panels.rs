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

pub struct PlaceholderPanel {
    kind: PanelKind,
    focus_handle: FocusHandle,
    probe: Option<LayoutProbe>,
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
struct LayoutProbe;

impl LayoutProbe {
    #[cfg(test)]
    fn record_sidebar(&self, bounds: gpui::Bounds<gpui::Pixels>) {
        self.sidebar.set(bounds);
        self.sidebar_prepaints
            .set(self.sidebar_prepaints.get().saturating_add(1));
    }

    #[cfg(not(test))]
    fn record_sidebar(&self, _: gpui::Bounds<gpui::Pixels>) {}

    #[cfg(test)]
    fn record_workspace(&self, bounds: gpui::Bounds<gpui::Pixels>) {
        self.workspace.set(bounds);
    }

    #[cfg(not(test))]
    fn record_workspace(&self, _: gpui::Bounds<gpui::Pixels>) {}
}

#[derive(Clone, Copy)]
enum PanelKind {
    Sidebar,
    Workspace,
}

impl PlaceholderPanel {
    pub fn sidebar(cx: &mut Context<Self>) -> Self {
        Self {
            kind: PanelKind::Sidebar,
            focus_handle: cx.focus_handle(),
            probe: None,
        }
    }

    pub fn workspace(cx: &mut Context<Self>) -> Self {
        Self {
            kind: PanelKind::Workspace,
            focus_handle: cx.focus_handle(),
            probe: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_probe(mut self, probe: LayoutProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    fn panel_title(&self) -> &'static str {
        match self.kind {
            PanelKind::Sidebar => "会话",
            PanelKind::Workspace => "对话",
        }
    }
}

impl EventEmitter<PanelEvent> for PlaceholderPanel {}

impl Focusable for PlaceholderPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for PlaceholderPanel {
    fn panel_name(&self) -> &'static str {
        match self.kind {
            PanelKind::Sidebar => "gpui-pi-sidebar",
            PanelKind::Workspace => "gpui-pi-workspace",
        }
    }

    fn tab_name(&self, _: &App) -> Option<SharedString> {
        Some(self.panel_title().into())
    }

    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.panel_title()
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

impl Render for PlaceholderPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        let probe = self.probe.clone();
        #[cfg(not(test))]
        let probe = self.probe;
        match self.kind {
            PanelKind::Sidebar => div()
                .id("session-sidebar")
                .debug_selector(|| "session-sidebar".into())
                .when_some(probe, |this, probe| {
                    this.on_prepaint(move |bounds, _, _| probe.record_sidebar(bounds))
                })
                .track_focus(&self.focus_handle)
                .size_full()
                .min_w_0()
                .p_3()
                .bg(cx.theme().sidebar)
                .child(
                    v_flex()
                        .size_full()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .text_sm()
                                .font_semibold()
                                .child("项目与会话"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .rounded_lg()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().background)
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child("会话列表将在 R5 接入"),
                        ),
                ),
            PanelKind::Workspace => div()
                .id("chat-workspace")
                .debug_selector(|| "chat-workspace".into())
                .when_some(probe, |this, probe| {
                    this.on_prepaint(move |bounds, _, _| probe.record_workspace(bounds))
                })
                .track_focus(&self.focus_handle)
                .size_full()
                .min_w_0()
                .bg(cx.theme().background)
                .child(
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_6()
                        .child(Icon::new(IconName::Bot).size(gpui::px(32.)))
                        .child(div().font_semibold().child("GPUI-Pi 主界面已就绪"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("历史消息与活会话将在 R6–R7 接入"),
                        ),
                ),
        }
    }
}
