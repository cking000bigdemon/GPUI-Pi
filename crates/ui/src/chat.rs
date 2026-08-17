use std::sync::Arc;

use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, Image, ImageFormat, InteractiveElement as _,
    IntoElement, ParentElement as _, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledText, Window, div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, StyledExt as _, WindowExt as _, dialog::DialogButtonProps, h_flex,
    scroll::ScrollableElement as _, text::TextView, v_flex,
};
use pi_render::{
    AnsiColor, AnsiStyle, AnsiText, Block, CodeBlock, ConversationDocument, DiffBlock,
    DiffLineKind, FrontmatterCard, ImageBlock, ImageState, Message, MessageRole, ToolCard,
    ToolOutput, ToolStatus,
};

#[derive(Clone, IntoElement)]
pub struct ChatWindow {
    document: Arc<ConversationDocument>,
    scroll_handle: ScrollHandle,
    selected_message: Option<String>,
}

impl ChatWindow {
    pub fn new(document: Arc<ConversationDocument>) -> Self {
        Self {
            document,
            scroll_handle: ScrollHandle::new(),
            selected_message: None,
        }
    }

    pub fn selected_message(mut self, id: impl Into<String>) -> Self {
        self.selected_message = Some(id.into());
        self
    }
}

impl gpui::RenderOnce for ChatWindow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let document = self.document.clone();
        let message_count = document.messages.len();
        let minimap_scroll = self.scroll_handle.clone();
        h_flex()
            .debug_selector(|| "chat-window".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .id("chat-message-scroll-area")
                    .debug_selector(|| "chat-message-scroll".into())
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .gap_3()
                    .track_scroll(&self.scroll_handle)
                    .overflow_y_scrollbar()
                    .p_3()
                    .children(
                        document
                            .messages
                            .iter()
                            .enumerate()
                            .map(|(index, message)| {
                                MessageView::new(index, message.clone())
                                    .selected(self.selected_message.as_deref() == Some(&message.id))
                            }),
                    ),
            )
            .when(!document.minimap.is_empty(), |this| {
                this.child(
                    ChatMinimap::new(document.clone(), minimap_scroll, message_count)
                        .selected(self.selected_message),
                )
            })
    }
}

#[derive(Clone, IntoElement)]
pub struct MessageView {
    index: usize,
    message: Message,
    selected: bool,
}

impl MessageView {
    pub fn new(index: usize, message: Message) -> Self {
        Self {
            index,
            message,
            selected: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl gpui::RenderOnce for MessageView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let role = role_label(self.message.role);
        let role_color = match self.message.role {
            MessageRole::User => cx.theme().accent_foreground,
            MessageRole::Assistant => cx.theme().foreground,
            MessageRole::Compaction | MessageRole::BranchSummary => cx.theme().warning,
            MessageRole::Unknown => cx.theme().danger,
            MessageRole::Custom => cx.theme().muted_foreground,
        };
        v_flex()
            .id(SharedString::from(format!("message-{}", self.message.id)))
            .debug_selector(|| "chat-message".into())
            .w_full()
            .min_w_0()
            .gap_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(if self.selected {
                cx.theme().accent
            } else {
                cx.theme().border
            })
            .bg(match self.message.role {
                MessageRole::User => cx.theme().accent.opacity(0.08),
                _ => cx.theme().background,
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(role_color)
                            .child(role),
                    )
                    .when_some(self.message.label.clone(), |row, label| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(label),
                        )
                    }),
            )
            .children(
                self.message
                    .blocks
                    .into_iter()
                    .enumerate()
                    .map(|(block_index, block)| render_block(self.index, block_index, block, cx)),
            )
    }
}

#[derive(Clone, IntoElement)]
pub struct MarkdownBody {
    id: SharedString,
    source: String,
}

impl MarkdownBody {
    pub fn new(id: impl Into<SharedString>, source: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
        }
    }
}

impl gpui::RenderOnce for MarkdownBody {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        safe_markdown(self.id, self.source).w_full()
    }
}

fn safe_markdown(id: impl Into<gpui::ElementId>, source: impl Into<SharedString>) -> TextView {
    TextView::markdown(id, source)
        .selectable(true)
        // 外层 ChatWindow 是唯一纵向滚动容器，避免消息内部再出现滚动条。
        .scrollable(false)
        .on_link_click(|url, _, window, cx| {
            let url: SharedString = url.clone();
            let open_url = url.clone();
            window.open_dialog(cx, move |dialog, _, _| {
                let open_url = open_url.clone();
                dialog
                    .title("打开外部链接？")
                    .child(
                        v_flex()
                            .gap_2()
                            .child("历史会话中的链接可能来自不受信任内容。")
                            .child(div().text_xs().child(url.clone())),
                    )
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text("打开")
                            .cancel_text("取消")
                            .show_cancel(true)
                            .on_ok(move |_, _, cx| {
                                cx.open_url(&open_url);
                                true
                            }),
                    )
            });
        })
}

#[derive(Clone, IntoElement)]
pub struct ChatMinimap {
    document: Arc<ConversationDocument>,
    scroll_handle: ScrollHandle,
    message_count: usize,
    selected_message: Option<String>,
}

impl ChatMinimap {
    pub fn new(
        document: Arc<ConversationDocument>,
        scroll_handle: ScrollHandle,
        message_count: usize,
    ) -> Self {
        Self {
            document,
            scroll_handle,
            message_count,
            selected_message: None,
        }
    }

    pub fn selected(mut self, selected: Option<String>) -> Self {
        self.selected_message = selected;
        self
    }
}

impl gpui::RenderOnce for ChatMinimap {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let message_count = self.message_count;
        v_flex()
            .debug_selector(|| "chat-minimap".into())
            .w(px(176.))
            .h_full()
            .min_h_0()
            .flex_none()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .p_2()
            .gap_1()
            .overflow_y_scrollbar()
            .children(self.document.minimap.iter().map(|node| {
                let id = node.message_id.clone();
                let selected = self.selected_message.as_deref() == Some(id.as_str());
                let message_index = self
                    .document
                    .messages
                    .iter()
                    .position(|message| message.id == id)
                    .unwrap_or(0)
                    .min(message_count.saturating_sub(1));
                let scroll = self.scroll_handle.clone();
                div()
                    .id(SharedString::from(format!("minimap-{id}")))
                    .debug_selector(|| "chat-minimap-node".into())
                    .ml(px(f32::from(node.level.unwrap_or(1).saturating_sub(1)) * 7.))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .truncate()
                    .cursor_pointer()
                    .when(selected, |item| item.bg(cx.theme().accent.opacity(0.16)))
                    .hover(|item| item.bg(cx.theme().muted))
                    .on_click(move |_, _, _| scroll.scroll_to_top_of_item(message_index))
                    .child(node.label.clone())
            }))
    }
}

fn render_block(index: usize, block_index: usize, block: Block, cx: &App) -> gpui::AnyElement {
    match block {
        Block::Markdown(markdown) => {
            MarkdownBody::new(format!("markdown-{index}-{block_index}"), markdown.source)
                .into_any_element()
        }
        Block::Code(code) => render_code(index, block_index, code, cx),
        Block::Thinking(text) => v_flex()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child("思考"),
            )
            .child(safe_markdown(
                format!("thinking-{index}-{block_index}"),
                text,
            ))
            .into_any_element(),
        Block::Tool(tool) => render_tool(tool, cx),
        Block::Diff(diff) => render_diff(diff, cx),
        Block::Ansi(ansi) => render_ansi(ansi, cx),
        Block::Image(image) => render_image(image, cx),
        Block::Frontmatter(frontmatter) => render_frontmatter(frontmatter, cx),
        Block::Notice(notice) => v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(div().text_sm().font_semibold().child(notice.title))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(notice.text),
            )
            .into_any_element(),
        Block::Unknown(unknown) => v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().warning)
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().warning)
                    .child(format!("未知内容 · {}", unknown.kind)),
            )
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .child(unknown.text),
            )
            .into_any_element(),
    }
}

fn render_code(index: usize, block_index: usize, code: CodeBlock, cx: &App) -> gpui::AnyElement {
    let language = code.language.as_deref().unwrap_or("text");
    let display_language = if code.mermaid_source {
        "mermaid · 源码"
    } else {
        language
    };
    v_flex()
        .debug_selector(move || {
            if code.mermaid_source {
                "mermaid-source".into()
            } else {
                "code-block".into()
            }
        })
        .min_w_0()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.42))
        .child(
            h_flex()
                .justify_between()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(display_language.to_owned()),
                )
                .when(code.truncated, |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child("已截断"),
                    )
                }),
        )
        // TextView 的 fenced-code 路径由启用的 gpui-component tree-sitter features 高亮。
        .child(
            safe_markdown(
                format!("code-{index}-{block_index}"),
                format!("```{language}\n{}\n```", code.source),
            )
            .p_2(),
        )
        .into_any_element()
}

fn render_tool(tool: ToolCard, cx: &App) -> gpui::AnyElement {
    let (status, color) = match tool.status {
        ToolStatus::Pending => ("pending", cx.theme().warning),
        ToolStatus::Success => ("success", cx.theme().success),
        ToolStatus::Error => ("error", cx.theme().danger),
        ToolStatus::Empty => ("empty", cx.theme().muted_foreground),
    };
    v_flex()
        .debug_selector(|| "tool-card".into())
        .min_w_0()
        .gap_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(color.opacity(0.7))
        .child(
            h_flex()
                .gap_2()
                .child(div().text_sm().font_semibold().child(tool.name))
                .child(div().text_xs().text_color(color).child(status))
                .when(tool.orphan, |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child("orphan"),
                    )
                }),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(tool.preview),
        )
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Input JSON"),
                )
                .child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_xs()
                        .child(tool.input_json),
                ),
        )
        .children(tool.output.into_iter().map(|output| match output {
            ToolOutput::Text(text) => div().text_sm().child(text).into_any_element(),
            ToolOutput::Ansi(ansi) => render_ansi(ansi, cx),
            ToolOutput::Image(image) => render_image(image, cx),
            ToolOutput::Diff(diff) => render_diff(diff, cx),
        }))
        .into_any_element()
}

fn render_diff(diff: DiffBlock, cx: &App) -> gpui::AnyElement {
    let lines = if diff.parsed {
        diff.files
            .iter()
            .flat_map(|file| {
                let path = match (&file.old_path, &file.new_path) {
                    (Some(old), Some(new)) if old != new => format!("{old} → {new}"),
                    (_, Some(new)) => new.clone(),
                    (Some(old), None) => old.clone(),
                    (None, None) => "未命名文件".to_owned(),
                };
                std::iter::once((DiffLineKind::Header, path)).chain(file.hunks.iter().flat_map(
                    |hunk| {
                        std::iter::once((DiffLineKind::Header, hunk.header.clone()))
                            .chain(hunk.lines.iter().map(|line| (line.kind, line.text.clone())))
                    },
                ))
            })
            .collect::<Vec<_>>()
    } else {
        diff.raw
            .lines()
            .map(|line| (DiffLineKind::Context, line.to_owned()))
            .collect()
    };
    v_flex()
        .debug_selector(|| "diff-block".into())
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted.opacity(0.3))
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .font_semibold()
                .child(if diff.parsed {
                    "Unified diff"
                } else {
                    "Raw diff fallback"
                }),
        )
        .children(lines.into_iter().map(|(kind, text)| {
            let (foreground, background) = match kind {
                DiffLineKind::Added => (cx.theme().success, cx.theme().success.opacity(0.08)),
                DiffLineKind::Removed => (cx.theme().danger, cx.theme().danger.opacity(0.08)),
                DiffLineKind::Header => (cx.theme().accent, cx.theme().accent.opacity(0.08)),
                DiffLineKind::Context => (cx.theme().foreground, cx.theme().transparent),
            };
            div()
                .px_2()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .text_color(foreground)
                .bg(background)
                .child(text)
        }))
        .into_any_element()
}

fn render_ansi(ansi: AnsiText, cx: &App) -> gpui::AnyElement {
    let highlights = ansi
        .spans
        .iter()
        .map(|span| (span.range.clone(), ansi_highlight(&span.style, cx)))
        .collect::<Vec<_>>();
    div()
        .debug_selector(|| "ansi-output".into())
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .child(StyledText::new(ansi.text).with_highlights(highlights))
        .into_any_element()
}

fn ansi_highlight(style: &AnsiStyle, cx: &App) -> HighlightStyle {
    HighlightStyle {
        color: style.foreground.map(|color| ansi_color(color, cx)),
        background_color: style.background.map(|color| ansi_color(color, cx)),
        font_weight: style.bold.then_some(FontWeight::BOLD),
        font_style: style.italic.then_some(FontStyle::Italic),
        underline: style.underline.then_some(gpui::UnderlineStyle {
            color: None,
            thickness: px(1.),
            wavy: false,
        }),
        fade_out: style.dim.then_some(0.45),
        ..HighlightStyle::default()
    }
}

fn ansi_color(color: AnsiColor, cx: &App) -> gpui::Hsla {
    match color {
        AnsiColor::Indexed(index) => match index % 16 {
            0 => cx.theme().muted_foreground,
            1 | 9 => cx.theme().danger,
            2 | 10 => cx.theme().success,
            3 | 11 => cx.theme().warning,
            4 | 12 => cx.theme().accent,
            5 | 13 => cx.theme().secondary_foreground,
            6 | 14 => cx.theme().info,
            _ => cx.theme().foreground,
        },
        AnsiColor::Rgb(red, green, blue) => {
            // ANSI 任意色先按感知亮度/主色投影到 theme semantic token，避免硬编码 RGB。
            let max = red.max(green).max(blue);
            let min = red.min(green).min(blue);
            if max.saturating_sub(min) < 24 {
                if max < 96 {
                    cx.theme().muted_foreground
                } else {
                    cx.theme().foreground
                }
            } else if red == max && green > blue {
                cx.theme().warning
            } else if red == max {
                cx.theme().danger
            } else if green == max {
                cx.theme().success
            } else {
                cx.theme().accent
            }
        }
    }
}

fn render_image(image: ImageBlock, cx: &App) -> gpui::AnyElement {
    if image.state == ImageState::Inline
        && let (Some(bytes), Some(mime)) = (image.bytes, image.mime_type.as_deref())
        && let Some(format) = ImageFormat::from_mime_type(mime)
    {
        return div()
            .debug_selector(|| "inline-image".into())
            .w_full()
            .max_h(px(320.))
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(img(Arc::new(Image::from_bytes(format, bytes))).max_h(px(320.)))
            .into_any_element();
    }
    v_flex()
        .debug_selector(|| "image-placeholder".into())
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(div().text_sm().font_semibold().child("图片占位"))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(image.description),
        )
        .when_some(image.remote_url, |view, url| {
            view.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(url),
            )
        })
        .into_any_element()
}

fn render_frontmatter(frontmatter: FrontmatterCard, cx: &App) -> gpui::AnyElement {
    v_flex()
        .debug_selector(|| "frontmatter-card".into())
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .when_some(frontmatter.title, |view, title| {
            view.child(div().text_sm().font_semibold().child(title))
        })
        .when(!frontmatter.tags.is_empty(), |view| {
            view.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().accent)
                    .child(frontmatter.tags.join(" · ")),
            )
        })
        .children(frontmatter.rows.into_iter().map(|(key, value)| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .w(px(96.))
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(key),
                )
                .child(div().flex_1().min_w_0().text_xs().child(value))
        }))
        .into_any_element()
}

fn role_label(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "User",
        MessageRole::Assistant => "Assistant",
        MessageRole::Custom => "Custom",
        MessageRole::Compaction => "Compaction",
        MessageRole::BranchSummary => "Branch summary",
        MessageRole::Unknown => "Unknown",
    }
}
