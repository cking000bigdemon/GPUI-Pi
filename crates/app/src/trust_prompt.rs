use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{App, Window};
use gpui_component::{WindowExt as _, notification::Notification};
use pi_data::{ProjectTrustStatus, TrustError, project_trust_status, trust_project};

/// 会话选择与手动目录选择共用同一套项目 trust 门禁。
pub(crate) fn prompt_project_trust(
    agent_dir: Option<PathBuf>,
    cwd: &Path,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(agent_dir) = agent_dir else {
        return;
    };
    let cwd = cwd.to_path_buf();
    let executor = cx.background_executor().clone();
    let window_handle = window.window_handle();
    cx.spawn(async move |cx| {
        let status = executor
            .spawn({
                let agent_dir = agent_dir.clone();
                let cwd = cwd.clone();
                async move { project_trust_status(agent_dir, cwd, None) }
            })
            .await;
        let _ = window_handle.update(cx, |_, window, cx| {
            finish_project_trust_check(status, agent_dir, cwd, window, cx);
        });
    })
    .detach();
}

fn finish_project_trust_check(
    status: Result<ProjectTrustStatus, TrustError>,
    agent_dir: PathBuf,
    cwd: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            window.push_notification(
                Notification::error(format!("读取项目 trust 状态失败：{error}")),
                cx,
            );
            return;
        }
    };
    if !status.requires_trust || status.trusted {
        return;
    }

    let busy = Rc::new(Cell::new(false));
    window.open_dialog(cx, move |dialog, _, _| {
        let trust_cwd = cwd.clone();
        let trust_agent = agent_dir.clone();
        let write_busy = busy.clone();
        let cancel_busy = busy.clone();
        gpui_pi_ui::project_trust_dialog(dialog, &cwd, move |_, window, cx| {
            if write_busy.replace(true) {
                return false;
            }
            let trust_cwd = trust_cwd.clone();
            let trust_agent = trust_agent.clone();
            let busy = write_busy.clone();
            let executor = cx.background_executor().clone();
            let window_handle = window.window_handle();
            cx.spawn(async move |cx| {
                let result = executor
                    .spawn(async move { trust_project(trust_agent, trust_cwd) })
                    .await;
                let _ = window_handle.update(cx, |_, window, cx| {
                    busy.set(false);
                    match result {
                        Ok(()) => {
                            window.close_dialog(cx);
                            window.push_notification(
                                Notification::success("项目已写入共享 trust store"),
                                cx,
                            );
                        }
                        Err(error) => {
                            window.push_notification(Notification::error(error.to_string()), cx);
                        }
                    }
                });
            })
            .detach();
            false
        })
        .on_cancel(move |_, _, _| !cancel_busy.get())
    });
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::{
        AppContext as _, Context, IntoElement, ParentElement as _, Render, TestAppContext,
        VisualTestContext, div, size,
    };
    use gpui_component::{Root, WindowExt as _, dialog::Confirm};
    use tempfile::tempdir;

    use super::*;

    struct EmptyView;

    impl Render for EmptyView {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().children(Root::render_dialog_layer(window, cx))
        }
    }

    #[gpui::test]
    fn trust_detection_and_write_are_deferred_to_background_executor(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let agent = tempdir().unwrap();
        let project = tempdir().unwrap();
        fs::create_dir_all(project.path().join(".pi").join("extensions")).unwrap();
        let trust_path = agent.path().join("trust.json");

        let handle = cx.open_window(size(gpui::px(480.), gpui::px(320.)), |window, cx| {
            let view = cx.new(|_| EmptyView);
            Root::new(view, window, cx)
        });
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.update(|window, cx| {
            prompt_project_trust(Some(agent.path().to_path_buf()), project.path(), window, cx);
            assert!(!window.has_active_dialog(cx));
            assert!(!trust_path.exists());
        });

        assert!(visual.executor().tick(), "trust 检测任务未入后台 executor");
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        visual.run_until_parked();
        visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
        assert!(!trust_path.exists());

        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
            window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
            assert!(window.has_active_dialog(cx));
            assert!(!trust_path.exists());
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        visual.run_until_parked();
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        assert!(trust_path.exists());
    }
}
