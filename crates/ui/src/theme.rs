use std::borrow::Cow;

use gpui::{App, Hsla, SharedString, Window};
use gpui_component::{ActiveTheme as _, Theme};

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

/// 同步系统深浅模式，并恢复应用侧的主题投影。
///
/// `Theme::change` 每次都会重放主题配置，把应用侧的定制冲掉，
/// 因此字体策略与深色面板层级投影都必须在每次 appearance 变化后重放。
pub fn sync_system_theme(window: &mut Window, cx: &mut App) {
    Theme::sync_system_appearance(Some(window), cx);
    apply_font_policy(cx);
    apply_panel_elevation(cx);
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

/// 深色面板层级投影（规范 S-15）：深色下面板必须比画布亮，方向与浅色相反。
///
/// gpui-component 默认深色主题里 `sidebar` 与 `background` 同为一档（#0a0a0a），
/// 吃默认值会丢掉「面板浮在画布之上」的观感；而 `title_bar` 在默认主题里已经是
/// 抬升一档的面板色（#171717）。把侧栏对齐到 title_bar 档即可满足方向要求，
/// 全程只做 token 到 token 的投影，不引入硬编码色（红线 1）。
pub fn apply_panel_elevation(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    if !theme.is_dark() {
        // 浅色默认面板本来就比画布暗（方向正确），无需投影。
        return;
    }
    theme.colors.sidebar = theme.colors.title_bar;
    theme.colors.sidebar_border = theme.colors.title_bar_border;
    // 组件库 Sidebar 读的是 legacy tokens（`cx.theme().tokens.sidebar`），必须与 colors 同步。
    theme.tokens.sidebar = theme.colors.sidebar.into();
    theme.tokens.sidebar_border = theme.colors.sidebar_border.into();
}

/// 第三级弱文本（dim，规范 S-16）：时间戳、行号、模型名、耗时等最弱一级的信息。
///
/// 全应用只允许通过本函数取该色；组件里禁止各写各的派生（守卫见本文件测试）。
pub fn dim_foreground(cx: &App) -> Hsla {
    cx.theme().muted_foreground.opacity(0.7)
}

/// 第四级禁用文本（规范 S-16）：仅用于禁用态，除此之外不允许再造新的文本档位。
pub fn disabled_foreground(cx: &App) -> Hsla {
    cx.theme().muted_foreground.opacity(0.5)
}

#[cfg(test)]
mod tests {
    use gpui_component::ThemeMode;

    use super::*;

    /// 递归访问目录下所有 `.rs` 文件，供源码级守卫断言使用。
    fn scan_rs_files(dir: &std::path::Path, visit: &mut dyn FnMut(&std::path::Path, &str)) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_rs_files(&path, visit);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                visit(&path, &content);
            }
        }
    }

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

    /// § 5.8 S-16：dim / disabled 两档必须由封装统一派生，取值固定。
    #[gpui::test]
    fn dim_and_disabled_foregrounds_are_single_source(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let muted = cx.theme().muted_foreground;
            assert_eq!(dim_foreground(cx), muted.opacity(0.7));
            assert_eq!(disabled_foreground(cx), muted.opacity(0.5));
            assert_ne!(dim_foreground(cx), muted, "dim 档必须弱于次文本");
            assert_ne!(
                dim_foreground(cx),
                disabled_foreground(cx),
                "dim 与禁用是两档，不许合并"
            );
        });
    }

    /// § 5.8 S-16（守卫）：除本文件的集中定义外，组件里不允许出现手写的
    /// muted_foreground 透明度派生——需要弱文本一律走 `dim_foreground` /
    /// `disabled_foreground`。
    #[test]
    fn muted_foreground_opacity_is_centralized() {
        // 拼接构造检索串，避免守卫被自己的源码命中。
        let needle = format!("muted_foreground.opacity{}", "(");
        let crates_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ui 必有父目录")
            .to_path_buf();
        let mut offenders = Vec::new();
        for root in ["ui", "app"] {
            scan_rs_files(&crates_dir.join(root).join("src"), &mut |path, content| {
                if path.file_name().is_some_and(|name| name == "theme.rs") {
                    return;
                }
                if content.contains(&needle) {
                    offenders.push(path.to_path_buf());
                }
            });
        }
        assert!(
            offenders.is_empty(),
            "以下文件绕过了 S-16 的集中封装，请改用 dim_foreground / disabled_foreground：{offenders:?}"
        );
    }

    /// § 5.8 S-15：深色下面板必须比画布亮一档；浅色方向本来正确，不投影。
    #[gpui::test]
    fn dark_panels_are_elevated_above_canvas(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            apply_panel_elevation(cx);
            let theme = Theme::global(cx);
            assert_ne!(theme.sidebar, theme.background, "深色下侧栏不得与画布同色");
            assert!(
                theme.sidebar.l > theme.background.l,
                "深色下面板必须比画布亮（S-15）"
            );
            assert_eq!(
                theme.tokens.sidebar.color, theme.colors.sidebar,
                "legacy tokens 必须与 colors 同步，组件库 Sidebar 走的是 tokens"
            );
            assert_eq!(theme.sidebar_border, theme.title_bar_border);

            Theme::change(ThemeMode::Light, None, cx);
            apply_panel_elevation(cx);
            let theme = Theme::global(cx);
            assert!(
                theme.sidebar.l <= theme.background.l,
                "浅色下面板不应比画布亮"
            );
        });
    }
}
