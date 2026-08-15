//! 把 pi 的消息/事件转换成「可渲染中间模型」。
//!
//! 这一层刻意**不依赖 GPUI**：Markdown 分块、ANSI 解析、diff 归类、工具调用
//! 卡片的形态判定全是纯数据变换，可以用真实会话 fixture 做快照测试，UI 层只
//! 负责把中间模型画出来。
//!
//! R0 只立骨架，实现见 Round 6。

/// 一条消息在界面上的呈现形态。
///
/// 变体清单来自 pi-web 0.8.9 `components/MessageView.tsx` 的分支，Round 6 落实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockKind {
    /// 普通 Markdown 段落。
    Markdown,
    /// 代码块（含 ```mermaid —— v1 按立项文档 § 一 直出源码，不渲染图）。
    Code,
    /// 工具调用卡片。
    ToolCall,
    /// bash 执行输出（需 ANSI 解析）。
    BashOutput,
    /// 统一 diff。
    Diff,
    /// 图片附件。
    Image,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_kinds_are_distinct() {
        assert_ne!(BlockKind::Markdown, BlockKind::Code);
        assert_ne!(BlockKind::ToolCall, BlockKind::BashOutput);
    }
}
