use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    tooltip::Tooltip,
    v_flex,
};
use pi_render::{DiffBlock, DiffLineKind, WrittenFile};

pub type PathHandler = Arc<dyn Fn(PathBuf, &mut App)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl GitChangeKind {
    pub const fn code(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Untracked => '?',
            Self::Conflict => 'U',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitChangeItem {
    pub relative_path: PathBuf,
    pub kind: GitChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitChangesModel {
    pub is_git_repository: bool,
    pub files: Vec<GitChangeItem>,
    pub total_files: usize,
    pub files_truncated: bool,
    pub additions: u64,
    pub deletions: u64,
    pub line_stats_truncated: bool,
}

/// 文件面板的 Git changes 区。状态色只用于代码字符，不铺边框或整行背景。
#[derive(IntoElement)]
pub struct GitChangesView {
    snapshot: Arc<GitChangesModel>,
    on_open_diff: Option<PathHandler>,
}

impl GitChangesView {
    pub fn new(snapshot: Arc<GitChangesModel>) -> Self {
        Self {
            snapshot,
            on_open_diff: None,
        }
    }

    pub fn on_open_diff(mut self, handler: impl Fn(PathBuf, &mut App) + 'static) -> Self {
        self.on_open_diff = Some(Arc::new(handler));
        self
    }
}

impl RenderOnce for GitChangesView {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let header = h_flex()
            .debug_selector(|| "git-changes-header".into())
            .px_2()
            .py_1()
            .gap_2()
            .child(div().text_sm().font_semibold().child("更改"))
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().success)
                    .child(format!("+{}", self.snapshot.additions)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(format!("-{}", self.snapshot.deletions)),
            );
        let content = if !self.snapshot.is_git_repository {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("当前目录不是 Git 仓库")
                .into_any_element()
        } else if self.snapshot.files.is_empty() {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("工作区无改动")
                .into_any_element()
        } else {
            let handler = self.on_open_diff.clone();
            v_flex()
                .debug_selector(|| "git-changes-scroll".into())
                .max_h(px(240.))
                .overflow_y_scrollbar()
                .gap_1()
                .children(self.snapshot.files.iter().enumerate().map(|(index, file)| {
                    let path = file.relative_path.clone();
                    let click_path = path.clone();
                    let color = match file.kind {
                        GitChangeKind::Added | GitChangeKind::Untracked => cx.theme().success,
                        GitChangeKind::Deleted | GitChangeKind::Conflict => cx.theme().danger,
                        GitChangeKind::Modified => cx.theme().warning,
                        GitChangeKind::Renamed => cx.theme().info,
                    };
                    let handler = handler.clone();
                    h_flex()
                        .id(("git-change-row", index))
                        .debug_selector(|| "git-change-row".into())
                        .px_2()
                        .py_1()
                        .gap_2()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|row| row.bg(cx.theme().muted))
                        .on_click(move |_, _, cx| {
                            if let Some(handler) = &handler {
                                handler(click_path.clone(), cx);
                            }
                        })
                        .child(
                            div()
                                .id(("git-change-code", index))
                                .w_4()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(color)
                                .tooltip(|window, cx| {
                                    Tooltip::new("Git 状态：? 未跟踪 / U 冲突").build(window, cx)
                                })
                                .child(file.kind.code().to_string()),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(path.to_string_lossy().into_owned()),
                        )
                }))
                .when(self.snapshot.files_truncated, |list| {
                    list.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "仅显示前 {} 项，另有 {} 项",
                                self.snapshot.files.len(),
                                self.snapshot
                                    .total_files
                                    .saturating_sub(self.snapshot.files.len())
                            )),
                    )
                })
                .when(self.snapshot.line_stats_truncated, |list| {
                    list.child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("未跟踪文件行数为预算内统计"),
                    )
                })
                .into_any_element()
        };
        v_flex()
            .debug_selector(|| "git-changes".into())
            .flex_none()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(header)
            .child(content)
    }
}

#[derive(IntoElement)]
pub struct DiffView {
    diff: Arc<DiffBlock>,
}

impl DiffView {
    pub fn new(diff: Arc<DiffBlock>) -> Self {
        Self { diff }
    }
}

impl RenderOnce for DiffView {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        render_diff_block(self.diff, cx)
    }
}

const MAX_RENDERED_DIFF_LINES: usize = 2_000;

pub fn render_diff_block(diff: Arc<DiffBlock>, cx: &App) -> gpui::AnyElement {
    let total_lines = diff_line_count(&diff);
    let lines = limited_diff_lines(&diff, MAX_RENDERED_DIFF_LINES);
    v_flex()
        .debug_selector(|| "diff-block".into())
        .min_w_0()
        .child(
            div()
                .py_1()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().muted_foreground)
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
                .px_1()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .text_color(foreground)
                .bg(background)
                .child(text)
        }))
        .when(total_lines > MAX_RENDERED_DIFF_LINES, |view| {
            view.child(
                div()
                    .py_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "diff 已截断：显示前 {MAX_RENDERED_DIFF_LINES} 行，共 {total_lines} 行"
                    )),
            )
        })
        .into_any_element()
}

fn diff_line_count(diff: &DiffBlock) -> usize {
    if diff.parsed {
        diff.files
            .iter()
            .map(|file| {
                1 + file
                    .hunks
                    .iter()
                    .map(|hunk| 1 + hunk.lines.len())
                    .sum::<usize>()
            })
            .sum()
    } else {
        diff.raw.lines().count()
    }
}

fn limited_diff_lines(diff: &DiffBlock, limit: usize) -> Vec<(DiffLineKind, String)> {
    if diff.parsed {
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
            .take(limit)
            .collect()
    } else {
        diff.raw
            .lines()
            .take(limit)
            .map(|line| (DiffLineKind::Context, line.to_owned()))
            .collect()
    }
}

pub fn written_file_can_open(file: &WrittenFile) -> bool {
    file.safe_relative_path.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WrittenFileChipStyle {
    pub border: gpui::Hsla,
    pub foreground: gpui::Hsla,
}

/// TurnWrittenFiles 只用中性弱边框表达“文件引用”，disabled 仅弱化文本；
/// 不把可打开/不可打开编码成状态色或铺色背景。
pub fn written_file_chip_style(can_open: bool, cx: &App) -> WrittenFileChipStyle {
    WrittenFileChipStyle {
        border: cx.theme().border,
        foreground: if can_open {
            cx.theme().muted_foreground
        } else {
            crate::theme::disabled_foreground(cx)
        },
    }
}

#[derive(IntoElement)]
pub struct TurnWrittenFiles {
    files: Vec<WrittenFile>,
    on_open: Option<PathHandler>,
}

impl TurnWrittenFiles {
    pub fn new(files: Vec<WrittenFile>) -> Self {
        Self {
            files,
            on_open: None,
        }
    }

    pub fn on_open(mut self, handler: impl Fn(PathBuf, &mut App) + 'static) -> Self {
        self.on_open = Some(Arc::new(handler));
        self
    }
}

impl RenderOnce for TurnWrittenFiles {
    fn render(self, _: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        let handler = self.on_open.clone();
        h_flex()
            .debug_selector(|| "turn-written-files".into())
            .flex_wrap()
            .gap_1p5()
            .mt_1p5()
            .children(self.files.into_iter().enumerate().map(|(index, file)| {
                let can_open = written_file_can_open(&file);
                let click_path = file.safe_relative_path.clone();
                let handler = handler.clone();
                let name = file.path.file_name().map_or_else(
                    || file.path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                let tooltip: SharedString = file.path.display().to_string().into();
                let style = written_file_chip_style(can_open, cx);
                Button::new(("written-file", index))
                    .debug_selector(|| "written-file-chip".into())
                    .small()
                    .ghost()
                    .border_1()
                    .border_color(style.border)
                    .rounded_sm()
                    .text_color(style.foreground)
                    .icon(IconName::File)
                    .label(name)
                    .tooltip(tooltip)
                    .disabled(!can_open)
                    .on_click(move |_, _, cx| {
                        if let (Some(handler), Some(path)) = (&handler, click_path.clone()) {
                            handler(path, cx);
                        }
                    })
            }))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui_component::ActiveTheme as _;
    use pi_render::WrittenFile;

    use super::{
        GitChangeKind, MAX_RENDERED_DIFF_LINES, diff_line_count, limited_diff_lines,
        written_file_can_open, written_file_chip_style,
    };

    #[test]
    fn git_change_codes_follow_porcelain_conventions() {
        assert_eq!(GitChangeKind::Modified.code(), 'M');
        assert_eq!(GitChangeKind::Added.code(), 'A');
        assert_eq!(GitChangeKind::Deleted.code(), 'D');
        assert_eq!(GitChangeKind::Renamed.code(), 'R');
        assert_eq!(GitChangeKind::Untracked.code(), '?');
        assert_eq!(GitChangeKind::Conflict.code(), 'U');
    }

    #[test]
    fn diff_component_accepts_render_parser_output() {
        let diff = pi_render::parse_unified_diff("--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new");
        assert!(diff.parsed);
        assert_eq!(diff.files.len(), 1);
    }

    #[test]
    fn diff_line_collection_stops_at_render_limit() {
        let raw = (0..MAX_RENDERED_DIFF_LINES + 10)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let diff = pi_render::parse_unified_diff(&raw);
        assert_eq!(diff_line_count(&diff), MAX_RENDERED_DIFF_LINES + 10);
        assert_eq!(
            limited_diff_lines(&diff, MAX_RENDERED_DIFF_LINES).len(),
            MAX_RENDERED_DIFF_LINES
        );
    }

    #[gpui::test]
    fn written_file_chip_uses_neutral_border_and_disabled_foreground(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::theme::init_fonts(cx).expect("font init failed");
            let enabled = written_file_chip_style(true, cx);
            let disabled = written_file_chip_style(false, cx);
            assert_eq!(enabled.border, cx.theme().border);
            assert_eq!(disabled.border, cx.theme().border);
            assert_eq!(enabled.foreground, cx.theme().muted_foreground);
            assert_eq!(disabled.foreground, crate::theme::disabled_foreground(cx));
            assert_ne!(enabled.foreground, cx.theme().success);
            assert_ne!(enabled.foreground, cx.theme().danger);
        });
    }

    #[test]
    fn written_file_without_safe_relative_path_cannot_open() {
        assert!(!written_file_can_open(&WrittenFile {
            path: PathBuf::from("outside.txt"),
            safe_relative_path: None,
        }));
        assert!(written_file_can_open(&WrittenFile {
            path: PathBuf::from("inside.txt"),
            safe_relative_path: Some(PathBuf::from("inside.txt")),
        }));
    }
}
