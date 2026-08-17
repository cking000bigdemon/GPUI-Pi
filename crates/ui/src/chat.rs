use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    App, FontStyle, FontWeight, HighlightStyle, Image, ImageFormat, InteractiveElement as _,
    IntoElement, ListOffset, ListState, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Window, div, img, list,
    prelude::FluentBuilder as _, px,
};
use gpui_base::{Scrollbar, ScrollbarMode};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::DialogButtonProps,
    h_flex,
    scroll::ScrollableElement as _,
    text::TextView,
    v_flex,
};

type TailAttachmentHandler = Arc<dyn Fn(bool, &mut Window, &mut App)>;
type TailDetachHandler = Arc<dyn Fn(&mut Window, &mut App)>;
type DetailToggleHandler = Arc<dyn Fn(String, String, &mut App)>;
type ProcessToggleHandler = Arc<dyn Fn(String, &mut App)>;
type MinimapToggleHandler = Arc<dyn Fn(&mut App)>;

use pi_render::{
    AnsiColor, AnsiStyle, AnsiText, Block, CodeBlock, ConversationDocument, ConversationItem,
    DiffBlock, DiffLineKind, FrontmatterCard, ImageBlock, ImageState, Message, MessageRole,
    ProcessGroup, ToolCard, ToolOutput, ToolStatus,
};

fn detail_key(message_id: &str, block_index: usize, kind: &str) -> String {
    format!("{message_id}:{kind}:{block_index}")
}

#[derive(Clone, IntoElement)]
pub struct ChatWindow {
    document: Arc<ConversationDocument>,
    list_state: ListState,
    selected_message: Option<String>,
    show_minimap: bool,
    expanded_tools: Arc<HashSet<String>>,
    expanded_processes: Arc<HashSet<String>>,
    on_toggle_tool: Option<DetailToggleHandler>,
    on_toggle_process: Option<ProcessToggleHandler>,
    on_toggle_minimap: Option<MinimapToggleHandler>,
    on_tail_attachment_change: Option<TailAttachmentHandler>,
    on_tail_detach: Option<TailDetachHandler>,
}

impl ChatWindow {
    pub fn new(document: Arc<ConversationDocument>, list_state: ListState) -> Self {
        Self {
            document,
            list_state,
            selected_message: None,
            show_minimap: true,
            expanded_tools: Arc::new(HashSet::new()),
            expanded_processes: Arc::new(HashSet::new()),
            on_toggle_tool: None,
            on_toggle_process: None,
            on_toggle_minimap: None,
            on_tail_attachment_change: None,
            on_tail_detach: None,
        }
    }

    pub fn show_minimap(mut self, show: bool) -> Self {
        self.show_minimap = show;
        self
    }

    pub fn expanded_tools(mut self, expanded: Arc<HashSet<String>>) -> Self {
        self.expanded_tools = expanded;
        self
    }

    pub fn expanded_processes(mut self, expanded: Arc<HashSet<String>>) -> Self {
        self.expanded_processes = expanded;
        self
    }

    pub fn on_toggle_tool(mut self, handler: impl Fn(String, String, &mut App) + 'static) -> Self {
        self.on_toggle_tool = Some(Arc::new(handler));
        self
    }

    pub fn on_toggle_process(mut self, handler: impl Fn(String, &mut App) + 'static) -> Self {
        self.on_toggle_process = Some(Arc::new(handler));
        self
    }

    pub fn on_toggle_minimap(mut self, handler: impl Fn(&mut App) + 'static) -> Self {
        self.on_toggle_minimap = Some(Arc::new(handler));
        self
    }

    pub fn on_tail_attachment_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_tail_attachment_change = Some(Arc::new(handler));
        self
    }

    pub fn on_tail_detach(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_tail_detach = Some(Arc::new(handler));
        self
    }

    pub fn selected_message(mut self, id: impl Into<String>) -> Self {
        self.selected_message = Some(id.into());
        self
    }
}

impl gpui::RenderOnce for ChatWindow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let document = self.document.clone();
        let item_count = document.items.len();
        let selected_message = self.selected_message.clone();
        let expanded_tools = self.expanded_tools.clone();
        let expanded_processes = self.expanded_processes.clone();
        let on_toggle_tool = self.on_toggle_tool.clone();
        let on_toggle_process = self.on_toggle_process.clone();
        let minimap_detach = self.on_tail_detach.clone();
        let minimap_toggle = self.on_toggle_minimap.clone();
        let list_state = self.list_state.clone();
        let items = document.items.clone();

        h_flex()
            .debug_selector(|| "chat-window".into())
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(cx.theme().background)
            .child(
                div()
                    .id("chat-message-list-area")
                    .debug_selector(|| "chat-message-scroll".into())
                    .relative()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        list(list_state.clone(), move |index, _, _| {
                            items.get(index).map_or_else(
                                || div().into_any_element(),
                                |item| match item {
                                    ConversationItem::Message(message) => {
                                        MessageView::new(index, message.clone())
                                            .selected(
                                                selected_message.as_deref() == Some(&message.id),
                                            )
                                            .expanded_tools(expanded_tools.clone())
                                            .on_toggle_tool(on_toggle_tool.clone())
                                            .into_any_element()
                                    }
                                    ConversationItem::Process(group) => {
                                        ProcessGroupView::new(index, group.clone())
                                            .expanded(
                                                !group.collapsible
                                                    || expanded_processes.contains(&group.id),
                                            )
                                            .expanded_tools(expanded_tools.clone())
                                            .on_toggle_tool(on_toggle_tool.clone())
                                            .on_toggle_process(on_toggle_process.clone())
                                            .into_any_element()
                                    }
                                },
                            )
                        })
                        .with_sizing_behavior(gpui::ListSizingBehavior::Infer)
                        .flex_grow_1()
                        .size_full(),
                    )
                    .child(
                        Scrollbar::vertical(&self.list_state)
                            .id("chat-message-scrollbar")
                            .mode(ScrollbarMode::Hover),
                    ),
            )
            .when(self.show_minimap && !document.minimap.is_empty(), |this| {
                this.child(
                    ChatMinimap::new(document.clone(), self.list_state.clone(), item_count)
                        .selected(self.selected_message)
                        .on_navigate(move |window, cx| {
                            if let Some(handler) = &minimap_detach {
                                handler(window, cx);
                            }
                        })
                        .on_toggle({
                            let minimap_toggle = minimap_toggle.clone();
                            move |cx| {
                                if let Some(handler) = &minimap_toggle {
                                    handler(cx);
                                }
                            }
                        }),
                )
            })
            .when(!self.show_minimap && !document.minimap.is_empty(), |this| {
                let on_toggle = minimap_toggle.clone();
                this.child(
                    v_flex()
                        .debug_selector(|| "chat-minimap-collapsed".into())
                        .h_full()
                        .flex_none()
                        .border_l_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().sidebar)
                        .p_1()
                        .child(
                            Button::new("show-chat-minimap")
                                .ghost()
                                .xsmall()
                                .icon(IconName::PanelRightOpen)
                                .tooltip("显示目录")
                                .on_click(move |_, _, cx| {
                                    if let Some(handler) = &on_toggle {
                                        handler(cx);
                                    }
                                }),
                        ),
                )
            })
    }
}

#[derive(Clone, IntoElement)]
pub struct MessageView {
    index: usize,
    message: Arc<Message>,
    selected: bool,
    expanded_tools: Arc<HashSet<String>>,
    on_toggle_tool: Option<DetailToggleHandler>,
}

impl MessageView {
    pub fn new(index: usize, message: Arc<Message>) -> Self {
        Self {
            index,
            message,
            selected: false,
            expanded_tools: Arc::new(HashSet::new()),
            on_toggle_tool: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn expanded_tools(mut self, expanded: Arc<HashSet<String>>) -> Self {
        self.expanded_tools = expanded;
        self
    }

    pub fn on_toggle_tool(mut self, handler: Option<DetailToggleHandler>) -> Self {
        self.on_toggle_tool = handler;
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
        let message_id = self.message.id.clone();
        let expanded_tools = self.expanded_tools.clone();
        v_flex()
            .id(SharedString::from(format!("message-{message_id}")))
            .debug_selector(|| "chat-message".into())
            .w_full()
            .min_w_0()
            .gap_2()
            .m_3()
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
                    .iter()
                    .enumerate()
                    .map(|(block_index, block)| {
                        render_block(
                            self.index,
                            block_index,
                            &message_id,
                            block,
                            &expanded_tools,
                            self.on_toggle_tool.clone(),
                            cx,
                        )
                    }),
            )
    }
}

#[derive(Clone, IntoElement)]
pub struct ProcessGroupView {
    index: usize,
    group: ProcessGroup,
    expanded: bool,
    expanded_tools: Arc<HashSet<String>>,
    on_toggle_tool: Option<DetailToggleHandler>,
    on_toggle_process: Option<ProcessToggleHandler>,
}

impl ProcessGroupView {
    pub fn new(index: usize, group: ProcessGroup) -> Self {
        Self {
            index,
            group,
            expanded: false,
            expanded_tools: Arc::new(HashSet::new()),
            on_toggle_tool: None,
            on_toggle_process: None,
        }
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn expanded_tools(mut self, expanded: Arc<HashSet<String>>) -> Self {
        self.expanded_tools = expanded;
        self
    }

    pub fn on_toggle_tool(mut self, handler: Option<DetailToggleHandler>) -> Self {
        self.on_toggle_tool = handler;
        self
    }

    pub fn on_toggle_process(mut self, handler: Option<ProcessToggleHandler>) -> Self {
        self.on_toggle_process = handler;
        self
    }
}

impl gpui::RenderOnce for ProcessGroupView {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let group_id = self.group.id.clone();
        let toggle_id = group_id.clone();
        let summary = if self.group.tool_call_count == 0 {
            format!("处理详情 · {} 条消息", self.group.message_count)
        } else {
            format!(
                "处理详情 · {} 条消息 · {} 次工具调用",
                self.group.message_count, self.group.tool_call_count
            )
        };
        let toggle = self.on_toggle_process.clone();
        v_flex()
            .id(SharedString::from(format!("process-group-{group_id}")))
            .debug_selector(|| "process-group".into())
            .w_full()
            .min_w_0()
            .gap_2()
            .mx_3()
            .my_1()
            .child(
                h_flex()
                    .id(SharedString::from(format!("process-toggle-{toggle_id}")))
                    .debug_selector(|| "process-group-toggle".into())
                    .gap_2()
                    .py_1()
                    .text_color(cx.theme().muted_foreground)
                    .when(self.group.collapsible, |row| {
                        row.cursor_pointer().on_click(move |_, _, cx| {
                            if let Some(handler) = &toggle {
                                handler(group_id.clone(), cx);
                            }
                        })
                    })
                    .child(if self.expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .child(div().text_xs().child(summary)),
            )
            .when(self.expanded, |view| {
                let expanded_tools = self.expanded_tools.clone();
                let on_toggle_tool = self.on_toggle_tool.clone();
                view.child(
                    v_flex()
                        .debug_selector(|| "process-group-details".into())
                        .gap_2()
                        .children(self.group.messages.iter().enumerate().map(
                            move |(message_index, message)| {
                                MessageView::new(
                                    self.index.saturating_mul(10_000) + message_index,
                                    message.clone(),
                                )
                                .expanded_tools(expanded_tools.clone())
                                .on_toggle_tool(on_toggle_tool.clone())
                            },
                        )),
                )
            })
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
    list_state: ListState,
    message_count: usize,
    selected_message: Option<String>,
    on_navigate: Option<TailDetachHandler>,
    on_toggle: Option<MinimapToggleHandler>,
}

impl ChatMinimap {
    pub fn new(
        document: Arc<ConversationDocument>,
        list_state: ListState,
        message_count: usize,
    ) -> Self {
        Self {
            document,
            list_state,
            message_count,
            selected_message: None,
            on_navigate: None,
            on_toggle: None,
        }
    }

    pub fn selected(mut self, selected: Option<String>) -> Self {
        self.selected_message = selected;
        self
    }

    pub fn on_navigate(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_navigate = Some(Arc::new(handler));
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(&mut App) + 'static) -> Self {
        self.on_toggle = Some(Arc::new(handler));
        self
    }
}

impl gpui::RenderOnce for ChatMinimap {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let message_count = self.message_count;
        let message_indexes = self
            .document
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.id(), index))
            .collect::<HashMap<_, _>>();
        let on_toggle = self.on_toggle.clone();
        v_flex()
            .debug_selector(|| "chat-minimap".into())
            .w(px(176.))
            .h_full()
            .min_h_0()
            .flex_none()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                h_flex().justify_end().p_1().child(
                    Button::new("hide-chat-minimap")
                        .ghost()
                        .xsmall()
                        .icon(IconName::PanelRight)
                        .tooltip("隐藏目录")
                        .on_click(move |_, _, cx| {
                            if let Some(handler) = &on_toggle {
                                handler(cx);
                            }
                        }),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .gap_1()
                    .overflow_y_scrollbar()
                    .children(self.document.minimap.iter().map(|node| {
                        let id = node.message_id.clone();
                        let selected = self.selected_message.as_deref() == Some(id.as_str());
                        let message_index = message_indexes
                            .get(id.as_str())
                            .copied()
                            .unwrap_or(0)
                            .min(message_count.saturating_sub(1));
                        let list_state = self.list_state.clone();
                        let on_navigate = self.on_navigate.clone();
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
                            .on_click(move |_, window, cx| {
                                list_state.scroll_to(ListOffset {
                                    item_ix: message_index,
                                    offset_in_item: px(0.),
                                });
                                if let Some(handler) = &on_navigate {
                                    handler(window, cx);
                                }
                                window.refresh();
                            })
                            .child(node.label.clone())
                    })),
            )
    }
}

fn render_block(
    index: usize,
    block_index: usize,
    message_id: &str,
    block: &Block,
    expanded_tools: &HashSet<String>,
    on_toggle_tool: Option<DetailToggleHandler>,
    cx: &App,
) -> gpui::AnyElement {
    match block {
        Block::Markdown(markdown) => MarkdownBody::new(
            format!("markdown-{index}-{block_index}"),
            markdown.source.clone(),
        )
        .into_any_element(),
        Block::Code(code) => render_code(index, block_index, code.clone(), cx),
        Block::Thinking(text) => {
            let key = detail_key(message_id, block_index, "thinking");
            render_thinking(
                index,
                block_index,
                text.clone(),
                key.clone(),
                expanded_tools.contains(&key),
                on_toggle_tool,
                cx,
            )
        }
        Block::Tool(tool) => {
            let key = tool_key(message_id, block_index, &tool.id);
            render_tool(
                tool.clone(),
                key.clone(),
                expanded_tools.contains(&key),
                on_toggle_tool,
                cx,
            )
        }
        Block::Diff(diff) => render_diff(diff.clone(), cx),
        Block::Ansi(ansi) => render_ansi(ansi.clone(), cx),
        Block::Image(image) => render_image(image.clone(), cx),
        Block::Frontmatter(frontmatter) => render_frontmatter(frontmatter.clone(), cx),
        Block::Notice(notice) => v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(div().text_sm().font_semibold().child(notice.title.clone()))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(notice.text.clone()),
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
                    .child(unknown.text.clone()),
            )
            .into_any_element(),
    }
}

fn message_item_id(detail_key: &str) -> String {
    detail_key.split(':').next().unwrap_or_default().to_owned()
}

fn tool_key(message_id: &str, block_index: usize, tool_id: &str) -> String {
    if tool_id.is_empty() {
        detail_key(message_id, block_index, "tool")
    } else {
        format!("{message_id}:tool:{tool_id}")
    }
}

fn render_thinking(
    index: usize,
    block_index: usize,
    text: String,
    key: String,
    expanded: bool,
    on_toggle: Option<DetailToggleHandler>,
    cx: &App,
) -> gpui::AnyElement {
    let toggle_key = key.clone();
    let item_id = message_item_id(&toggle_key);
    v_flex()
        .id(SharedString::from(format!("thinking-card-{key}")))
        .debug_selector(|| "thinking-card".into())
        .min_w_0()
        .gap_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .id(SharedString::from(format!("thinking-toggle-{key}")))
                .debug_selector(|| "thinking-card-toggle".into())
                .gap_2()
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    if let Some(handler) = &on_toggle {
                        handler(toggle_key.clone(), item_id.clone(), cx);
                    }
                })
                .child(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .child(div().text_sm().font_semibold().child("思考"))
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if expanded { "收起" } else { "展开" }),
                ),
        )
        .when(expanded, |view| {
            view.child(
                div()
                    .debug_selector(|| "thinking-card-details".into())
                    .child(safe_markdown(
                        format!("thinking-{index}-{block_index}"),
                        text,
                    )),
            )
        })
        .into_any_element()
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
        .child(
            safe_markdown(
                format!("code-{index}-{block_index}"),
                format!("```{language}\n{}\n```", code.source),
            )
            .p_2(),
        )
        .into_any_element()
}

fn render_tool(
    tool: ToolCard,
    key: String,
    expanded: bool,
    on_toggle_tool: Option<DetailToggleHandler>,
    cx: &App,
) -> gpui::AnyElement {
    let (status, color) = match tool.status {
        ToolStatus::Pending => ("pending", cx.theme().warning),
        ToolStatus::Success => ("success", cx.theme().success),
        ToolStatus::Error => ("error", cx.theme().danger),
        ToolStatus::Empty => ("empty", cx.theme().muted_foreground),
    };
    let toggle_key = key.clone();
    let item_id = message_item_id(&toggle_key);
    v_flex()
        .id(SharedString::from(format!("tool-card-{key}")))
        .debug_selector(|| "tool-card".into())
        .min_w_0()
        .gap_2()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(color.opacity(0.7))
        .child(
            h_flex()
                .id(SharedString::from(format!("tool-toggle-{key}")))
                .debug_selector(|| "tool-card-toggle".into())
                .gap_2()
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    if let Some(handler) = &on_toggle_tool {
                        handler(toggle_key.clone(), item_id.clone(), cx);
                    }
                })
                .child(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .child(div().text_sm().font_semibold().child(tool.name.clone()))
                .child(div().text_xs().text_color(color).child(status))
                .when(tool.orphan, |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child("orphan"),
                    )
                })
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if expanded { "收起" } else { "展开" }),
                ),
        )
        .child(
            div()
                .text_xs()
                .truncate()
                .text_color(cx.theme().muted_foreground)
                .child(tool.preview),
        )
        .when(expanded, |view| {
            view.child(
                v_flex()
                    .debug_selector(|| "tool-card-details".into())
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
                    )
                    .children(tool.output.into_iter().map(|output| match output {
                        ToolOutput::Text(text) => div().text_sm().child(text).into_any_element(),
                        ToolOutput::Ansi(ansi) => render_ansi(ansi, cx),
                        ToolOutput::Image(image) => render_image(image, cx),
                        ToolOutput::Diff(diff) => render_diff(diff, cx),
                    })),
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_keys_are_stable_and_separate_thinking_from_tools() {
        assert_eq!(tool_key("m", 2, "call"), "m:tool:call");
        assert_eq!(tool_key("m", 2, ""), "m:tool:2");
        assert_eq!(detail_key("m", 2, "thinking"), "m:thinking:2");
    }
}
