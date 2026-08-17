use std::path::Path;

use gpui::{ParentElement as _, Styled as _, div};
use gpui_component::{
    ActiveTheme as _, Icon, IconName,
    button::ButtonVariant,
    dialog::{Dialog, DialogButtonProps},
    v_flex,
};

/// 项目信任确认的统一原生 Dialog。`on_trust` 返回 false 时保留对话框供重试。
pub fn project_trust_dialog(
    dialog: Dialog,
    project: &Path,
    on_trust: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) -> bool + 'static,
) -> Dialog {
    let project = project.display().to_string();
    dialog
        .title("信任此项目？")
        .overlay_closable(false)
        .close_button(false)
        .keyboard(false)
        .content(move |content, _, cx| {
            content.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_color(cx.theme().warning)
                            .child(Icon::new(IconName::TriangleAlert))
                            .child("该目录包含可执行或可改变 pi 行为的项目资源。"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("信任后，pi 可加载项目 .pi 配置、extensions、skills、prompts、themes 以及 .agents/skills。"),
                    )
                    .child(
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(cx.theme().muted)
                            .text_sm()
                            .child(project.clone()),
                    ),
            )
        })
        .button_props(
            DialogButtonProps::default()
                .ok_text("信任项目")
                .ok_variant(ButtonVariant::Primary)
                .cancel_text("暂不信任")
                .show_cancel(true)
                .on_ok(on_trust),
        )
}
