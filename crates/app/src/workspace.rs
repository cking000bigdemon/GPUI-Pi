use std::{path::PathBuf, sync::Arc};

use gpui::{
    AppContext as _, Context, Edges, InteractiveElement as _, IntoElement, ParentElement as _,
    PathPromptOptions, Render, SharedString, Styled as _, Subscription, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Root, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    dock::{DockArea, DockItem, DockPlacement},
    h_flex,
};
use gpui_pi_ui::{AppShell, WorkspaceTabBar, theme};

#[cfg(test)]
use crate::panels::LayoutProbe;
use crate::panels::PlaceholderPanel;

const MAIN_DOCK_ID: &str = "gpui-pi-main-dock";
const MAIN_DOCK_VERSION: usize = 1;
pub(crate) const SIDEBAR_WIDTH: f32 = 280.;

pub struct Workspace {
    dock_area: gpui::Entity<DockArea>,
    selected_directory: Option<PathBuf>,
    _appearance_subscription: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(window, cx, None)
    }

    #[cfg(test)]
    pub(crate) fn new_with_probe(
        window: &mut Window,
        cx: &mut Context<Self>,
        probe: LayoutProbe,
    ) -> Self {
        Self::build(window, cx, Some(probe))
    }

    fn build(
        window: &mut Window,
        cx: &mut Context<Self>,
        #[cfg(test)] probe: Option<LayoutProbe>,
        #[cfg(not(test))] _probe: Option<()>,
    ) -> Self {
        theme::sync_system_theme(window, cx);

        let dock_area =
            cx.new(|cx| DockArea::new(MAIN_DOCK_ID, Some(MAIN_DOCK_VERSION), window, cx));
        let sidebar = cx.new(|cx| {
            let panel = PlaceholderPanel::sidebar(cx);
            #[cfg(test)]
            if let Some(probe) = probe.clone() {
                return panel.with_probe(probe);
            }
            panel
        });
        let workspace = cx.new(|cx| {
            let panel = PlaceholderPanel::workspace(cx);
            #[cfg(test)]
            if let Some(probe) = probe.clone() {
                return panel.with_probe(probe);
            }
            panel
        });

        dock_area.update(cx, |dock_area, cx| {
            dock_area.set_center(DockItem::panel(Arc::new(workspace)), window, cx);
            dock_area.set_left_dock(
                DockItem::panel(Arc::new(sidebar)),
                Some(px(SIDEBAR_WIDTH)),
                true,
                window,
                cx,
            );
            dock_area.set_dock_collapsible(
                Edges {
                    left: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            // 收展入口由应用工具栏统一承载，避免同时显示组件库内置按钮。
            dock_area.set_toggle_button_visible(false, cx);
        });

        let appearance_subscription = cx.observe_window_appearance(window, |_, window, cx| {
            theme::sync_system_theme(window, cx);
            cx.notify();
        });

        Self {
            dock_area,
            selected_directory: None,
            _appearance_subscription: appearance_subscription,
        }
    }

    fn toggle_sidebar(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dock_area.update(cx, |dock_area, cx| {
            dock_area.toggle_dock(DockPlacement::Left, window, cx);
        });
        // Dock 自身的 notify 不会重绘 Dock 外的工具栏；这里同步刷新图标和 tooltip。
        cx.notify();
    }

    fn choose_directory(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择项目目录".into()),
        });

        cx.spawn_in(window, async move |workspace, cx| {
            let Some(paths) = receiver.await.ok().and_then(Result::ok).flatten() else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.selected_directory = Some(path);
                cx.notify();
            });
        })
        .detach();
    }

    fn tab_label(&self) -> SharedString {
        self.selected_directory
            .as_deref()
            .map_or_else(|| "未选择项目".into(), tab_label_for_path)
    }

    fn directory_tooltip(&self) -> SharedString {
        self.selected_directory.as_deref().map_or_else(
            || "选择项目目录".into(),
            |path| path.display().to_string().into(),
        )
    }
}

fn tab_label_for_path(path: &std::path::Path) -> SharedString {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
        .into()
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sheet_layer = Root::render_sheet_layer(window, cx);
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        // TitleBar 的内容区在 Windows 会被标为原生 Drag hitbox，交互控件必须放在其下方。
        let title_bar = TitleBar::new().child(
            h_flex()
                .size_full()
                .min_w_0()
                .child(div().flex_none().font_semibold().child("GPUI-Pi")),
        );
        let sidebar_open = self
            .dock_area
            .read(cx)
            .is_dock_open(DockPlacement::Left, cx);
        let sidebar_toggle_icon = if sidebar_open {
            IconName::PanelLeft
        } else {
            IconName::PanelLeftOpen
        };
        let sidebar_toggle_tooltip = if sidebar_open {
            "收起侧栏"
        } else {
            "展开侧栏"
        };
        let toolbar = h_flex()
            .debug_selector(|| "workspace-toolbar".into())
            .h(gpui::px(38.))
            // 150% DPI 下仍给左侧控制留出明确边距，避免图标贴住窗口边缘。
            .px_3()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().title_bar)
            .child(
                div().flex_none().child(
                    Button::new("toggle-left-sidebar")
                        .debug_selector(|| "sidebar-toggle".into())
                        .ghost()
                        .small()
                        .icon(sidebar_toggle_icon)
                        .tooltip(sidebar_toggle_tooltip)
                        .on_click(cx.listener(Self::toggle_sidebar)),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(WorkspaceTabBar::new(self.tab_label())),
            )
            .child(
                Button::new("choose-project-directory")
                    .debug_selector(|| "choose-project-directory".into())
                    .ghost()
                    .small()
                    .icon(IconName::FolderOpen)
                    .label("打开目录")
                    .tooltip(self.directory_tooltip())
                    .on_click(cx.listener(Self::choose_directory)),
            );

        div()
            .size_full()
            .child(AppShell::new(title_bar, toolbar, self.dock_area.clone()))
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, size};

    fn render_workspace(
        cx: &mut TestAppContext,
        window_size: gpui::Size<gpui::Pixels>,
        probe: LayoutProbe,
    ) -> VisualTestContext {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("test font init failed");
        });
        let handle = cx.open_window(window_size, move |window, cx| {
            let workspace = cx.new(|cx| Workspace::new_with_probe(window, cx, probe));
            Root::new(workspace, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..4 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        visual
    }

    #[gpui::test]
    fn default_layout_places_sidebar_left_of_workspace(cx: &mut TestAppContext) {
        let probe = LayoutProbe::default();
        let mut cx = render_workspace(cx, size(px(1200.), px(800.)), probe.clone());

        let shell = cx.debug_bounds("app-shell").expect("app shell missing");
        let title = cx.debug_bounds("app-title-bar").expect("title bar missing");
        let sidebar = probe.sidebar.get();
        let workspace = probe.workspace.get();

        assert!(shell.size.width > px(0.) && shell.size.height > px(0.));
        assert!(sidebar.size.width > px(0.) && workspace.size.width > px(0.));
        assert!(title.origin.y <= sidebar.origin.y);
        assert!(sidebar.origin.x < workspace.origin.x);
        assert!(sidebar.size.width >= px(240.) && sidebar.size.width <= px(360.));
        assert!(workspace.size.width > px(500.));
        let toolbar = cx.debug_bounds("app-toolbar").expect("toolbar missing");
        assert!(sidebar.origin.y >= toolbar.origin.y + toolbar.size.height);
        assert!(workspace.origin.y >= toolbar.origin.y + toolbar.size.height);
    }

    #[gpui::test]
    fn minimum_window_keeps_both_dock_regions_usable(cx: &mut TestAppContext) {
        let probe = LayoutProbe::default();
        let mut cx = render_workspace(cx, size(px(800.), px(560.)), probe.clone());
        let shell = cx.debug_bounds("app-shell").expect("app shell missing");
        let title = cx.debug_bounds("app-title-bar").expect("title bar missing");
        let toolbar = cx.debug_bounds("app-toolbar").expect("toolbar missing");
        let body = cx.debug_bounds("app-dock-body").expect("dock body missing");
        let sidebar = probe.sidebar.get();
        let workspace = probe.workspace.get();

        assert!(shell.size.width > px(0.) && shell.size.height > px(0.));
        assert!(title.size.height > px(0.) && toolbar.size.height > px(0.));
        assert!(body.size.width > px(0.) && body.size.height > px(0.));
        assert!(sidebar.size.height > px(0.) && workspace.size.height > px(0.));
        assert!(sidebar.size.width >= px(200.));
        assert!(workspace.size.width >= px(360.));
        // 左 Dock 的 panel 会包含 tab/title 内边距，布局 contract 只要求中心起点在其右侧。
        assert!(sidebar.origin.x < workspace.origin.x);
        assert!(toolbar.origin.y >= title.origin.y + title.size.height);
        assert!(body.origin.y >= toolbar.origin.y + toolbar.size.height);
        assert!(sidebar.origin.y >= body.origin.y && workspace.origin.y >= body.origin.y);
    }

    #[gpui::test]
    fn sidebar_toggle_collapses_and_reopens_left_dock(cx: &mut TestAppContext) {
        let probe = LayoutProbe::default();
        let mut visual = render_workspace(cx, size(px(1200.), px(800.)), probe.clone());
        let toggle = visual
            .debug_bounds("sidebar-toggle")
            .expect("sidebar toggle missing");
        let expanded_workspace_width = probe.workspace.get().size.width;

        visual.simulate_click(toggle.center(), Default::default());
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("session-sidebar").is_none());
        assert!(probe.workspace.get().size.width > expanded_workspace_width);

        let toggle = visual
            .debug_bounds("sidebar-toggle")
            .expect("sidebar expand toggle missing");
        let prepaints_before_reopen = probe.sidebar_prepaints.get();
        visual.simulate_click(toggle.center(), Default::default());
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(probe.sidebar_prepaints.get() > prepaints_before_reopen);
        let reopened_workspace = probe.workspace.get();
        assert!(reopened_workspace.origin.x >= px(200.));
        assert!(reopened_workspace.size.width < px(1000.));
    }

    #[gpui::test]
    fn directory_button_opens_native_directory_prompt(cx: &mut TestAppContext) {
        let probe = LayoutProbe::default();
        let mut visual = render_workspace(cx, size(px(800.), px(560.)), probe);
        let button = visual
            .debug_bounds("choose-project-directory")
            .expect("directory button missing");

        visual.simulate_click(button.center(), Default::default());
        assert!(visual.did_prompt_for_paths());
        visual.simulate_path_prompt_response(|options| {
            assert!(!options.files);
            assert!(options.directories);
            assert!(!options.multiple);
            None
        });
        visual.run_until_parked();
        assert!(!visual.did_prompt_for_paths());
    }

    #[test]
    fn directory_labels_cover_roots_and_unicode_names() {
        assert_eq!(
            tab_label_for_path(std::path::Path::new("C:\\")).as_ref(),
            "C:\\"
        );
        assert_eq!(
            tab_label_for_path(std::path::Path::new("C:/项目/会话")).as_ref(),
            "会话"
        );
    }
}
