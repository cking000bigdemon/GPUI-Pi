//! pi 的本地数据目录（默认 `~/.pi/agent`）读写层。
//!
//! 与终端 `pi`、`pi-web`、pi-web-desktop **共享同一份目录**，因此本 crate 的
//! 写操作必须保守：能只读就只读，必须写时走「临时文件 + rename」。
//!
//! 本 crate 不依赖 GPUI。R0 只提供目录定位；解析实现见 Round 3。

use std::ffi::OsString;
use std::path::PathBuf;

/// pi 用来覆盖数据目录的环境变量，语义与上游一致。
pub const AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";

/// 解析 pi 的 agent 数据目录。
///
/// 优先 `PI_CODING_AGENT_DIR`，否则 `<home>/.pi/agent`。
/// Windows 上 `dirs::home_dir()` 取的是 `USERPROFILE`，不要自己读 `HOME`。
pub fn agent_dir() -> Option<PathBuf> {
    agent_dir_from(std::env::var_os(AGENT_DIR_ENV), dirs::home_dir())
}

/// [`agent_dir`] 的纯函数版本，便于在不改进程环境的前提下单测。
pub fn agent_dir_from(env_override: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match env_override {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => home.map(|h| h.join(".pi").join("agent")),
    }
}

/// 会话文件所在目录。
pub fn sessions_dir() -> Option<PathBuf> {
    agent_dir().map(|d| d.join("sessions"))
}

/// 扩展目录。
pub fn extensions_dir() -> Option<PathBuf> {
    agent_dir().map(|d| d.join("extensions"))
}

/// 技能目录。GPUI-Pi 只读不写 —— 部署归 pi-web-desktop 管，见立项文档 § 一。
pub fn skills_dir() -> Option<PathBuf> {
    agent_dir().map(|d| d.join("skills"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_wins() {
        let got = agent_dir_from(Some(OsString::from("/tmp/custom")), Some("/home/x".into()));
        assert_eq!(got, Some(PathBuf::from("/tmp/custom")));
    }

    #[test]
    fn empty_env_falls_back_to_home() {
        let got = agent_dir_from(Some(OsString::new()), Some("/home/x".into()));
        assert_eq!(got, Some(PathBuf::from("/home/x/.pi/agent")));
    }

    #[test]
    fn no_home_no_dir() {
        assert_eq!(agent_dir_from(None, None), None);
    }
}
