use std::borrow::Cow;

use gpui::{App, SharedString, Window};
use gpui_component::Theme;

const NOTO_SANS_SC: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Regular.ttf");

#[cfg(target_os = "windows")]
pub const UI_FONT_FAMILY: &str = "Microsoft YaHei UI";
#[cfg(not(target_os = "windows"))]
pub const UI_FONT_FAMILY: &str = "Noto Sans SC";

#[cfg(target_os = "windows")]
pub const MONO_FONT_FAMILY: &str = "Consolas";
#[cfg(not(target_os = "windows"))]
pub const MONO_FONT_FAMILY: &str = "Noto Sans Mono CJK SC";

/// 初始化应用字体。正文使用平台字体策略；内嵌 Noto Sans SC 作为可显式选用的离线 CJK 字体资源。
pub fn init_fonts(cx: &mut App) -> anyhow::Result<()> {
    cx.text_system()
        .add_fonts(vec![Cow::Borrowed(NOTO_SANS_SC)])?;
    apply_font_policy(cx);
    Ok(())
}

/// 同步系统深浅模式，并恢复应用字体策略。
///
/// `Theme::change` 会重新应用主题配置，因此字体策略必须在每次 appearance 变化后重放。
pub fn sync_system_theme(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
    apply_font_policy(cx);
}

fn apply_font_policy(cx: &mut App) {
    let ui_font = SharedString::from(UI_FONT_FAMILY);
    let mono_font = SharedString::from(MONO_FONT_FAMILY);
    let theme = Theme::global_mut(cx);
    theme.font_family = ui_font.clone();
    theme.mono_font_family = mono_font.clone();

    // `Theme::change` 先把 semantic tokens 投影到 gpui-base；重放字体时同步更新投影。
    let base_theme = gpui_base::Theme::global_mut(cx);
    base_theme.tokens.typography.sans = ui_font;
    base_theme.tokens.typography.mono = mono_font;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_cjk_font_is_not_empty() {
        assert!(NOTO_SANS_SC.len() > 1_000_000);
    }

    #[test]
    fn platform_font_policy_is_explicit() {
        assert!(!UI_FONT_FAMILY.is_empty());
        assert!(!MONO_FONT_FAMILY.is_empty());
    }

    #[gpui::test]
    fn font_policy_updates_component_and_base_themes(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            apply_font_policy(cx);
            assert_eq!(Theme::global(cx).font_family.as_ref(), UI_FONT_FAMILY);
            assert_eq!(
                Theme::global(cx).mono_font_family.as_ref(),
                MONO_FONT_FAMILY
            );
            let base = gpui_base::Theme::global(cx);
            assert_eq!(base.tokens.typography.sans.as_ref(), UI_FONT_FAMILY);
            assert_eq!(base.tokens.typography.mono.as_ref(), MONO_FONT_FAMILY);
        });
    }
}
