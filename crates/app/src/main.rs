//! GPUI-Pi 入口。
//!
//! R0 **刻意不开窗口** —— 本轮的验收目标是「工程骨架能编译、依赖链能对上」，
//! 起窗口是 Round 1 风险门禁 spike 的事。这里只做环境自检并打印，方便在
//! CI 与空机器上确认二进制真的能跑起来。

use std::process::ExitCode;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let agent_dir = pi_data::agent_dir();
    let pi_binary = std::path::Path::new("vendor")
        .join("pi")
        .join(pi_rpc::pi_binary_name());

    println!("GPUI-Pi {}", env!("CARGO_PKG_VERSION"));
    println!("  pi 内核钉死版本 : {}", pi_rpc::PINNED_PI_VERSION);
    println!("  发布包目标      : pi-{}", pi_rpc::pi_release_target());
    println!("  期望二进制路径  : {}", pi_binary.display());
    println!(
        "  agent 数据目录  : {}",
        agent_dir
            .as_deref()
            .map_or_else(|| "<未解析出 home>".into(), |p| p.display().to_string())
    );
    println!("  UI 层标记       : {}", gpui_pi_ui::theme_marker());
    println!();
    println!("R0 骨架：窗口尚未实现，见 rounds/round-01/round-01.md。");

    ExitCode::SUCCESS
}
