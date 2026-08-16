//! R1 风险门禁 spike：只验证 IME、流式 Markdown、跨消息选择与冷启动。

use std::time::Duration;

use gpui::*;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Root,
    button::Button,
    input::{Textarea, TextareaState},
    progress::Progress,
    text::{TextView, TextViewState},
    v_flex,
};
use gpui_component_assets::Assets;
use gpui_fps::{FpsMonitor, FpsOverlay};
use instant::Instant;

#[path = "../spike_data.rs"]
mod spike_data;
use spike_data::{MIN_STREAM_TOKENS, STREAM_INTERVAL, generate_stream_document};

const SAMPLE_MESSAGES: [&str; 5] = [
    "**消息 1 / 普通段落**\n\n从这里开始拖选。第一条包含普通段落、中文标点与 inline `code`。",
    "**消息 2 / 跨消息换行**\n\n本条第一段结束。\n\n本条第二段开始；复制到记事本时，两段之间应保留空行。",
    "**消息 3 / 列表**\n\n- 第一项：alpha\n- 第二项：beta\n  - 嵌套项：gamma",
    "**消息 4 / fenced code block**\n\n```rust\nfn copied_code() {\n    println!(\"line one\\nline two\");\n}\n```",
    "**消息 5 / 结束标记**\n\n拖到这里结束并按 Ctrl+C；记事本应得到渲染文本、正确换行和完整代码块。",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamStatus {
    Ready,
    Streaming,
    Complete,
}

impl StreamStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "就绪（冷启动未生成长文）",
            Self::Streaming => "流式追加中",
            Self::Complete => "完成",
        }
    }
}

struct SpikeView {
    textarea: Entity<TextareaState>,
    markdown_state: Entity<TextViewState>,
    stream_status: StreamStatus,
    target_tokens: usize,
    total_chunks: usize,
    appended_chunks: usize,
    max_schedule_lag: Duration,
    stream_generation: usize,
    stream_task: Task<()>,
    fps_monitor: Entity<FpsMonitor>,
}

impl SpikeView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let textarea = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(6)
                .placeholder("在此用微软拼音/搜狗连续输入 200 字；Enter 只应确认候选或换行")
        });
        textarea.update(cx, |state, cx| state.focus(window, cx));

        Self {
            textarea,
            markdown_state: cx.new(|cx| {
                TextViewState::markdown(
                    "# 8000 token 流式 Markdown\n\n点击按钮后才生成并开始追加；冷启动不加载长文。\n",
                    cx,
                )
            }),
            stream_status: StreamStatus::Ready,
            target_tokens: 0,
            total_chunks: 0,
            appended_chunks: 0,
            max_schedule_lag: Duration::ZERO,
            stream_generation: 0,
            stream_task: Task::ready(()),
            fps_monitor: cx.new(|cx| FpsMonitor::new(window, cx).show_resources(false)),
        }
    }

    fn start_stream(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.stream_status == StreamStatus::Streaming {
            return;
        }

        // 长文刻意只在点击后生成，避免冷启动门禁被假数据构造污染。
        let document = generate_stream_document();
        debug_assert_eq!(document.markdown, document.chunks.concat());

        self.stream_generation = self.stream_generation.wrapping_add(1);
        let generation = self.stream_generation;
        self.target_tokens = document.token_count;
        self.total_chunks = document.chunks.len();
        self.appended_chunks = 0;
        self.max_schedule_lag = Duration::ZERO;
        self.stream_status = StreamStatus::Streaming;
        self.markdown_state.update(cx, |state, cx| {
            state.set_text("# 8000 token 流式 Markdown\n\n", cx);
        });
        cx.notify();

        self.stream_task = cx.spawn(async move |weak_self, cx| {
            let started_at = Instant::now();
            for (index, chunk) in document.chunks.into_iter().enumerate() {
                // 以统一起点计算每个 deadline；处理落后时不再额外等 30ms，避免慢渲染反而降载。
                let deadline = started_at + STREAM_INTERVAL * (index as u32 + 1);
                let now = Instant::now();
                if deadline > now {
                    cx.background_executor().timer(deadline - now).await;
                }
                let delivered_at = Instant::now();
                let schedule_lag = delivered_at.saturating_duration_since(deadline);
                let result = weak_self.update(cx, |this, cx| {
                    if this.stream_generation != generation {
                        return;
                    }
                    this.markdown_state.update(cx, |state, cx| {
                        state.push_str(&chunk, cx);
                    });
                    this.appended_chunks += 1;
                    this.max_schedule_lag = this.max_schedule_lag.max(schedule_lag);
                    if this.appended_chunks == this.total_chunks {
                        this.stream_status = StreamStatus::Complete;
                    }
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        });
    }

    fn sample_message(&self, index: usize, cx: &App) -> AnyElement {
        div()
            .w_full()
            .p_3()
            .rounded_lg()
            .bg(if index.is_multiple_of(2) {
                cx.theme().muted
            } else {
                cx.theme().primary.opacity(0.08)
            })
            .child(
                TextView::markdown(("sample-message", index), SAMPLE_MESSAGES[index])
                    .selectable(true),
            )
            .into_any_element()
    }

    fn render_header(&self, cx: &App) -> AnyElement {
        v_flex()
            .w(px(1000.))
            .max_w_full()
            .gap_1()
            .child("GPUI-Pi R1 风险门禁 Spike")
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("步骤：① 微软拼音/搜狗各输入 200 字；② 点击流式按钮，记录灌注期间最低 FPS，阈值 ≥50；③ 从消息 1 拖到消息 5，Ctrl+C 粘贴记事本；④ 冷启动连续 5 次取中位数，阈值 <1500ms（脚本只测 MainWindowHandle+Responding 近似，不是精确首帧）。"),
            )
            .into_any_element()
    }

    fn render_input_panel(&self) -> AnyElement {
        v_flex()
            .w(px(1000.))
            .max_w_full()
            .min_w(px(0.))
            .items_stretch()
            .gap_2()
            .child(
                "1. 中文 IME 多行输入（启动后已聚焦；应用不拦截 Enter；滚动到页面底部做选择测试）",
            )
            .child(
                div()
                    .relative()
                    .max_w_full()
                    .h(px(84.))
                    .overflow_hidden()
                    .child(
                        Textarea::new(&self.textarea)
                            .absolute()
                            .inset_0()
                            .min_w(px(0.)),
                    ),
            )
            .into_any_element()
    }

    fn render_selection_panel(&self, cx: &App) -> AnyElement {
        v_flex()
            .w(px(1000.))
            .max_w_full()
            .min_w(px(0.))
            .gap_2()
            .child("3. 跨 5 条消息拖选（默认 Plain selection）")
            .child(
                v_flex().gap_2().children(
                    (0..SAMPLE_MESSAGES.len()).map(|index| self.sample_message(index, cx)),
                ),
            )
            .into_any_element()
    }

    fn render_stream_panel(
        &self,
        progress: f32,
        token_label: String,
        chunk_label: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .w(px(1000.))
            .max_w_full()
            .min_w(px(0.))
            .h(px(420.))
            .items_stretch()
            .gap_2()
            .child("2. TextViewState 流式 Markdown")
            .child(
                Button::new("start-stream")
                    .w(px(300.))
                    .max_w_full()
                    .label("灌 8000+ token 长文")
                    .disabled(self.stream_status == StreamStatus::Streaming)
                    .on_click(cx.listener(Self::start_stream)),
            )
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .child(format!("状态: {}", self.stream_status.label()))
                    .child(token_label)
                    .child(chunk_label),
            )
            .child(Progress::new("stream-progress").value(progress))
            .child(
                div()
                    .id("stream-markdown")
                    .max_w_full()
                    .flex_1()
                    .min_h(px(0.))
                    .p_3()
                    .rounded_lg()
                    .bg(cx.theme().muted)
                    .child(TextView::new(&self.markdown_state).scrollable(true)),
            )
            .into_any_element()
    }
}

impl Render for SpikeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let progress = if self.total_chunks == 0 {
            0.0
        } else {
            self.appended_chunks as f32 / self.total_chunks as f32 * 100.0
        };
        let token_label = if self.target_tokens == 0 {
            format!("whitespace token proxy: ≥{MIN_STREAM_TOKENS}（点击后计算）")
        } else {
            format!(
                "whitespace token proxy: {}（阈值 ≥{MIN_STREAM_TOKENS}）",
                self.target_tokens
            )
        };
        let chunk_label = format!(
            "chunk: {}/{} · wall-clock 节拍: {}ms · 最大调度落后: {}ms",
            self.appended_chunks,
            self.total_chunks,
            STREAM_INTERVAL.as_millis(),
            self.max_schedule_lag.as_millis()
        );

        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .id("spike-scroll")
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .items_center()
                            .p_4()
                            .gap_3()
                            .child(self.render_header(cx))
                            .child(self.render_input_panel())
                            .child(self.render_stream_panel(progress, token_label, chunk_label, cx))
                            .child(self.render_selection_panel(cx)),
                    ),
            )
            .child(FpsOverlay::new(&self.fps_monitor).anchor(Anchor::BottomRight))
    }
}

fn main() {
    let app = gpui_platform::application().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1100.), px(900.)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                window.activate_window();
                window.set_window_title(
                    "GPUI-Pi R1 Spike — IME / 8000 token@30ms / FPS≥50 / selection / cold<1500ms",
                );
                let view = cx.new(|cx| SpikeView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("无法打开 R1 spike 窗口");
        })
        .detach();
    });
}
