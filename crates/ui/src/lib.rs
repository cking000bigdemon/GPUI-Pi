//! GPUI-Pi 的应用级 UI 组件层。
//!
//! 只放「跨面板复用」的封装；单个面板的私有组件留在 `crates/app`。
//! 样式一律走 gpui-component 的 `Theme` / `ThemeColor` 变量，禁止硬编码颜色。
//!
//! R0 只验证依赖链能编译，组件见 Round 4 起。

/// 应用主题的初始化入口（Round 4 填充）。
///
/// 存在的意义是让 R0 的 `cargo build` 真的走一遍 gpui + gpui-component 的编译，
/// 从而证明 `Cargo.lock` 里钉的两个 sha 能互相兼容。
pub fn theme_marker() -> &'static str {
    "gpui-pi-theme"
}

#[cfg(test)]
mod tests {
    #[test]
    fn marker_is_stable() {
        assert_eq!(super::theme_marker(), "gpui-pi-theme");
    }
}
