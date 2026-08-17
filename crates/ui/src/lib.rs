//! GPUI-Pi 的应用级 UI 组件层。
//!
//! 只放跨面板复用的外壳、主题和组件封装；窗口状态与 Dock 编排留在 `crates/app`。

mod project_trust_dialog;
mod shell;
mod tab_bar;
pub mod theme;

pub use project_trust_dialog::project_trust_dialog;
pub use shell::AppShell;
pub use tab_bar::WorkspaceTabBar;
