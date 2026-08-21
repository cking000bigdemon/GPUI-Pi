use std::path::PathBuf;

use gpui::{
    App, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogClose, DialogFooter},
    h_flex,
    notification::Notification,
    switch::Switch,
    v_flex,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceTab {
    Skills,
    Plugins,
}

#[derive(Debug, Clone)]
enum LoadState {
    Loading,
    Ready {
        skills: pi_data::SkillScan,
        plugins: pi_data::PluginScan,
    },
    Error(String),
}

pub struct ResourceConfig {
    cwd: PathBuf,
    agent_dir: Option<PathBuf>,
    tab: ResourceTab,
    state: LoadState,
    load_generation: u64,
    busy_skill: Option<PathBuf>,
}

impl ResourceConfig {
    pub fn new(cwd: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            cwd,
            agent_dir: pi_data::agent_dir(),
            tab: ResourceTab::Skills,
            state: LoadState::Loading,
            load_generation: 0,
            busy_skill: None,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(agent_dir) = self.agent_dir.clone() else {
            self.state = LoadState::Error("无法解析 pi agent 目录".to_owned());
            return;
        };
        self.load_generation = self.load_generation.wrapping_add(1);
        let generation = self.load_generation;
        self.state = LoadState::Loading;
        let cwd = self.cwd.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move {
                    Ok::<_, String>((
                        pi_data::scan_skills(&agent_dir, &cwd, pi_data::home_dir().as_deref()),
                        pi_data::scan_plugin_packages(
                            &agent_dir,
                            &cwd,
                            pi_data::home_dir().as_deref(),
                        ),
                    ))
                })
                .await;
            let _ = entity.update(cx, |entity, cx| {
                if entity.finish_reload(generation, result) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn finish_reload(
        &mut self,
        generation: u64,
        result: Result<(pi_data::SkillScan, pi_data::PluginScan), String>,
    ) -> bool {
        if generation != self.load_generation {
            return false;
        }
        self.state = match result {
            Ok((skills, plugins)) => LoadState::Ready { skills, plugins },
            Err(error) => LoadState::Error(error),
        };
        true
    }

    fn select_tab(&mut self, tab: ResourceTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    fn toggle_skill(
        &mut self,
        path: PathBuf,
        revision: pi_data::FileRevision,
        disable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy_skill.is_some() {
            return;
        }
        let Some(agent_dir) = self.agent_dir.clone() else {
            return;
        };
        self.busy_skill = Some(path.clone());
        let cwd = self.cwd.clone();
        let executor = cx.background_executor().clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |entity, cx| {
            let result = executor
                .spawn(async move {
                    let home = pi_data::home_dir();
                    let allowed_roots =
                        pi_data::skill_allowed_roots(&agent_dir, &cwd, home.as_deref()).map_err(
                            |error| format!("读取项目 trust 失败，未修改 Skill：{error}"),
                        )?;
                    pi_data::set_skill_disable_model_invocation(
                        &path,
                        &allowed_roots,
                        &revision,
                        disable,
                    )
                    .map_err(|error| format!("Skill 开关修改失败：{error}"))
                })
                .await;
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = entity.update(cx, |entity, cx| {
                    entity.busy_skill = None;
                    match result {
                        Ok(_) => {
                            window.push_notification(
                                Notification::success(if disable {
                                    "Skill 已从模型自动调用中隐藏"
                                } else {
                                    "Skill 已允许模型自动调用"
                                }),
                                cx,
                            );
                            entity.reload(cx);
                        }
                        Err(error) => {
                            window.push_notification(Notification::error(error), cx);
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn render_skills(&self, scan: &pi_data::SkillScan, cx: &mut Context<Self>) -> gpui::AnyElement {
        if scan.skills.is_empty() && scan.diagnostics.is_empty() {
            return empty_state(
                "resource-config-empty-skills",
                "没有发现 Skills",
                "仅扫描用户目录与已信任项目目录",
                cx,
            );
        }
        let entity = cx.entity();
        v_flex()
            .debug_selector(|| "skills-config-list".into())
            .gap_1()
            .when(!scan.diagnostics.is_empty(), |view| {
                view.child(render_diagnostics(&scan.diagnostics, cx))
            })
            .children(scan.skills.iter().enumerate().map(|(index, skill)| {
                let path = skill.path.clone();
                let revision = skill.revision.clone();
                let disable = !skill.disable_model_invocation;
                let busy = self.busy_skill.is_some();
                let scope = match skill.scope {
                    pi_data::ResourceScope::User => "用户",
                    pi_data::ResourceScope::Project => "项目",
                };
                h_flex()
                    .debug_selector(move || format!("skill-row-{index}"))
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .hover(|row| row.bg(cx.theme().muted))
                    .child(
                        div()
                            .size_2()
                            .rounded_full()
                            .bg(if skill.disable_model_invocation {
                                cx.theme().muted_foreground
                            } else {
                                cx.theme().success
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div().text_sm().font_semibold().child(skill.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(scope),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(skill.description.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(cx.theme().muted_foreground)
                                    .child(skill.path.display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("skill-toggle-{index}"))
                            .child(
                                Switch::new(("skill-toggle", index))
                                    .small()
                                    .checked(!skill.disable_model_invocation)
                                    .disabled(busy)
                                    .tooltip(if skill.disable_model_invocation {
                                        "允许模型自动调用"
                                    } else {
                                        "从模型自动调用中隐藏；仍可显式调用"
                                    })
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, window, cx| {
                                            entity.update(cx, |config, cx| {
                                                config.toggle_skill(
                                                    path.clone(),
                                                    revision.clone(),
                                                    disable,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            ),
                    )
            }))
            .into_any_element()
    }

    fn render_plugins(
        &self,
        scan: &pi_data::PluginScan,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if scan.packages.is_empty() && scan.diagnostics.is_empty() {
            return empty_state(
                "resource-config-empty-plugins",
                "没有配置 Plugins",
                "安装与更新请使用终端 pi 或 pi-web-desktop",
                cx,
            );
        }
        v_flex()
            .debug_selector(|| "plugins-config-list".into())
            .gap_1()
            .when(!scan.diagnostics.is_empty(), |view| {
                view.child(render_diagnostics(&scan.diagnostics, cx))
            })
            .children(scan.packages.iter().enumerate().map(|(index, package)| {
                let scope = match package.scope {
                    pi_data::ResourceScope::User => "用户",
                    pi_data::ResourceScope::Project => "项目",
                };
                let state = if package.filters.disabled() {
                    "已禁用"
                } else if package.filters.filtered() {
                    "有过滤"
                } else {
                    "全部资源"
                };
                h_flex()
                    .debug_selector(move || format!("plugin-row-{index}"))
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .hover(|row| row.bg(cx.theme().muted))
                    .child(
                        div()
                            .size_2()
                            .rounded_full()
                            .bg(if package.filters.disabled() {
                                cx.theme().muted_foreground
                            } else if package.filters.filtered() {
                                cx.theme().warning
                            } else {
                                cx.theme().success
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(package.source.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(scope)
                                    .child(state),
                            ),
                    )
            }))
            .into_any_element()
    }
}

impl Render for ResourceConfig {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let body = match &self.state {
            LoadState::Loading => empty_state(
                "resource-config-loading",
                "正在加载资源配置…",
                "只读扫描共享配置目录",
                cx,
            ),
            LoadState::Error(error) => empty_state(
                "resource-config-error",
                "资源配置加载失败",
                error.clone(),
                cx,
            ),
            LoadState::Ready { skills, plugins } => match self.tab {
                ResourceTab::Skills => self.render_skills(skills, cx),
                ResourceTab::Plugins => self.render_plugins(plugins, cx),
            },
        };
        let trust_notice = match &self.state {
            LoadState::Ready { skills, plugins } => skills
                .trust_error
                .as_deref()
                .or(plugins.trust_error.as_deref())
                .map(|error| format!("读取项目 trust 失败：{error}"))
                .or_else(|| {
                    (!skills.project_resources_loaded)
                        .then(|| "项目未信任：项目 Skills / Plugins 未加载".to_owned())
                }),
            _ => None,
        };
        let trust_error = matches!(
            &self.state,
            LoadState::Ready { skills, plugins }
                if skills.trust_error.is_some() || plugins.trust_error.is_some()
        );
        v_flex()
            .debug_selector(|| "resource-config".into())
            .gap_3()
            .child(
                h_flex()
                    .debug_selector(|| "resource-config-header".into())
                    .gap_2()
                    .child(
                        Button::new("resource-tab-skills")
                            .small()
                            .when(self.tab == ResourceTab::Skills, |button| button.primary())
                            .when(self.tab != ResourceTab::Skills, |button| button.secondary())
                            .label("Skills")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |config, cx| {
                                        config.select_tab(ResourceTab::Skills, cx)
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("resource-tab-plugins")
                            .small()
                            .when(self.tab == ResourceTab::Plugins, |button| button.primary())
                            .when(self.tab != ResourceTab::Plugins, |button| {
                                button.secondary()
                            })
                            .label("Plugins")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |config, cx| {
                                    config.select_tab(ResourceTab::Plugins, cx)
                                });
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("只展示/启停 Skill；Plugins 只读"),
                    ),
            )
            .when_some(trust_notice, |view, notice| {
                view.child(
                    h_flex()
                        .debug_selector(move || {
                            if trust_error {
                                "resource-config-trust-error".into()
                            } else {
                                "resource-config-untrusted".into()
                            }
                        })
                        .gap_2()
                        .text_xs()
                        .text_color(if trust_error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(div().size_2().rounded_full().bg(if trust_error {
                            cx.theme().danger
                        } else {
                            cx.theme().warning
                        }))
                        .child(notice),
                )
            })
            // Dialog 已持有唯一外层滚动区；这里保持内容驱动高度，避免嵌套 scrollbar owner。
            .child(
                div()
                    .debug_selector(|| "resource-config-body".into())
                    .child(body),
            )
            .child(
                div()
                    .debug_selector(|| "resource-config-note".into())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("安装、搜索、更新与删除不在本应用执行"),
            )
    }
}

pub fn open_resource_config(cwd: PathBuf, window: &mut Window, cx: &mut App) {
    let config = cx.new(|cx| ResourceConfig::new(cwd, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title("Skills / Plugins")
            .w(gpui::px(640.))
            .h(gpui::px(420.))
            .overlay_closable(true)
            .child(config.clone())
            .footer(
                div()
                    .debug_selector(|| "resource-config-footer".into())
                    .child(
                        DialogFooter::new().child(
                            DialogClose::new().child(
                                Button::new("close-resource-config").primary().label("关闭"),
                            ),
                        ),
                    ),
            )
    });
}

fn render_diagnostics(diagnostics: &[pi_data::ResourceDiagnostic], cx: &App) -> gpui::AnyElement {
    v_flex()
        .debug_selector(|| "resource-diagnostics".into())
        .gap_1()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(cx.theme().warning)
                .child(format!("资源扫描诊断（{}）", diagnostics.len())),
        )
        .children(
            diagnostics
                .iter()
                .take(8)
                .enumerate()
                .map(|(index, diagnostic)| {
                    div()
                        .debug_selector(move || format!("resource-diagnostic-{index}"))
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{}：{}",
                            diagnostic.path.display(),
                            diagnostic.message
                        ))
                }),
        )
        .into_any_element()
}

fn empty_state(
    selector: &'static str,
    title: impl Into<gpui::SharedString>,
    detail: impl Into<gpui::SharedString>,
    cx: &App,
) -> gpui::AnyElement {
    let title_selector = format!("{selector}-title");
    let detail_selector = format!("{selector}-detail");
    v_flex()
        .debug_selector(move || selector.into())
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .child(
            div()
                .debug_selector(move || title_selector.clone())
                .text_sm()
                .font_semibold()
                .child(title.into()),
        )
        .child(
            div()
                .debug_selector(move || detail_selector.clone())
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(detail.into()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui::{ScrollDelta, ScrollWheelEvent, point, px, size};

    use super::*;

    fn draw_frames(visual: &mut gpui::VisualTestContext, count: usize) {
        for _ in 0..count {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
    }

    fn bounds_overlap(left: gpui::Bounds<gpui::Pixels>, right: gpui::Bounds<gpui::Pixels>) -> bool {
        left.left() < right.right()
            && left.right() > right.left()
            && left.top() < right.bottom()
            && left.bottom() > right.top()
    }

    fn assert_inside_window(bounds: gpui::Bounds<gpui::Pixels>) {
        assert!(bounds.left() >= px(0.));
        assert!(bounds.top() >= px(0.));
        assert!(bounds.right() <= px(800.));
        assert!(bounds.bottom() <= px(560.));
    }

    fn fixture_skill(index: usize) -> pi_data::SkillInfo {
        pi_data::SkillInfo {
            name: format!("skill-{index}"),
            description: format!("fixture skill {index}"),
            path: PathBuf::from(format!("C:/fixture/skill-{index}/SKILL.md")),
            scope: pi_data::ResourceScope::Project,
            disable_model_invocation: false,
            revision: pi_data::FileRevision {
                len: index as u64,
                modified_nanos: index as u128,
            },
        }
    }

    fn fixture_plugin(index: usize) -> pi_data::PluginPackageInfo {
        pi_data::PluginPackageInfo {
            source: format!("npm:fixture-{index}"),
            scope: pi_data::ResourceScope::User,
            filters: pi_data::PackageFilters {
                autoload: None,
                extensions: None,
                skills: None,
                prompts: None,
                themes: None,
            },
        }
    }

    struct ResourceDialogHarness {
        config: gpui::Entity<ResourceConfig>,
        opened: bool,
    }

    impl Render for ResourceDialogHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if !self.opened {
                self.opened = true;
                let config = self.config.clone();
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog
                        .title("Skills / Plugins")
                        .w(gpui::px(640.))
                        .h(gpui::px(420.))
                        .child(config.clone())
                        .footer(
                            div()
                                .debug_selector(|| "resource-config-footer".into())
                                .child(DialogFooter::new().child("footer")),
                        )
                });
            }
            div()
                .size_full()
                .children(gpui_component::Root::render_dialog_layer(window, cx))
        }
    }

    #[gpui::test]
    fn resource_dialog_keeps_rows_body_and_footer_visible_at_minimum_window(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let agent = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let skill_path = project.path().join(".pi/skills/fixture/SKILL.md");
        std::fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
        std::fs::write(
            &skill_path,
            "---\nname: fixture\ndescription: Visible fixture skill\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(
            agent.path().join("settings.json"),
            r#"{"packages":["./package-fixture"]}"#,
        )
        .unwrap();
        pi_data::trust_project(agent.path(), project.path()).unwrap();
        let cwd = project.path().to_path_buf();
        let agent_path = agent.path().to_path_buf();
        let handle = cx.open_window(
            gpui::size(gpui::px(800.), gpui::px(560.)),
            move |window, cx| {
                let config = cx.new(|_| ResourceConfig {
                    cwd: cwd.clone(),
                    agent_dir: Some(agent_path.clone()),
                    tab: ResourceTab::Skills,
                    state: LoadState::Ready {
                        skills: pi_data::scan_skills(&agent_path, &cwd, None),
                        plugins: pi_data::scan_plugin_packages(&agent_path, &cwd, None),
                    },
                    load_generation: 0,
                    busy_skill: None,
                });
                let harness = cx.new(|_| ResourceDialogHarness {
                    config,
                    opened: false,
                });
                gpui_component::Root::new(harness, window, cx)
            },
        );
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        draw_frames(&mut visual, 5);
        let body = visual
            .debug_bounds("resource-config-body")
            .expect("resource body must be laid out");
        let row = visual
            .debug_bounds("skill-row-0")
            .expect("skill row must be visible");
        let footer = visual
            .debug_bounds("resource-config-footer")
            .expect("footer must remain visible");
        assert!(body.size.height > gpui::px(0.));
        assert!(row.top() >= body.top() && row.bottom() <= body.bottom());
        assert!(!bounds_overlap(body, footer));
        assert!(!bounds_overlap(row, footer));
    }

    #[gpui::test]
    fn resource_dialog_outer_body_is_the_only_scroll_owner_across_all_states(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        let handle = cx.open_window(size(px(800.), px(560.)), move |window, cx| {
            let config = cx.new(|_| ResourceConfig {
                cwd: PathBuf::from("C:/fixture/project"),
                agent_dir: Some(PathBuf::from("C:/fixture/agent")),
                tab: ResourceTab::Skills,
                state: LoadState::Loading,
                load_generation: 0,
                busy_skill: None,
            });
            *output.borrow_mut() = Some(config.clone());
            let harness = cx.new(|_| ResourceDialogHarness {
                config,
                opened: false,
            });
            gpui_component::Root::new(harness, window, cx)
        });
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        draw_frames(&mut visual, 4);
        assert!(visual.debug_bounds("resource-config-loading").is_some());
        let config = captured.borrow().clone().unwrap();

        config.update(cx, |config, cx| {
            config.state = LoadState::Ready {
                skills: pi_data::SkillScan {
                    project_resources_loaded: true,
                    ..Default::default()
                },
                plugins: pi_data::PluginScan {
                    project_resources_loaded: true,
                    ..Default::default()
                },
            };
            cx.notify();
        });
        draw_frames(&mut visual, 3);
        assert!(
            visual
                .debug_bounds("resource-config-empty-skills")
                .is_some()
        );

        config.update(cx, |config, cx| {
            config.state = LoadState::Error("fixture error".into());
            cx.notify();
        });
        draw_frames(&mut visual, 3);
        assert!(visual.debug_bounds("resource-config-error").is_some());

        config.update(cx, |config, cx| {
            config.state = LoadState::Ready {
                skills: pi_data::SkillScan::default(),
                plugins: pi_data::PluginScan::default(),
            };
            cx.notify();
        });
        draw_frames(&mut visual, 3);
        assert!(visual.debug_bounds("resource-config-untrusted").is_some());

        config.update(cx, |config, cx| {
            config.state = LoadState::Ready {
                skills: pi_data::SkillScan {
                    trust_error: Some("trust fixture".into()),
                    ..Default::default()
                },
                plugins: pi_data::PluginScan::default(),
            };
            cx.notify();
        });
        draw_frames(&mut visual, 3);
        assert!(visual.debug_bounds("resource-config-trust-error").is_some());

        let diagnostic = pi_data::ResourceDiagnostic {
            path: PathBuf::from("C:/fixture/diagnostic"),
            message: "fixture diagnostic".into(),
        };
        config.update(cx, |config, cx| {
            config.state = LoadState::Ready {
                skills: pi_data::SkillScan {
                    skills: (0..30).map(fixture_skill).collect(),
                    diagnostics: vec![diagnostic.clone()],
                    project_resources_loaded: true,
                    trust_error: None,
                },
                plugins: pi_data::PluginScan {
                    packages: (0..30).map(fixture_plugin).collect(),
                    diagnostics: vec![diagnostic],
                    project_resources_loaded: true,
                    trust_error: None,
                },
            };
            cx.notify();
        });
        draw_frames(&mut visual, 4);
        assert!(visual.debug_bounds("resource-diagnostics").is_some());
        assert!(visual.debug_bounds("skill-row-0").is_some());
        assert!(visual.debug_bounds("skill-row-29").is_some());
        let dialog = visual
            .update(|window, _| gpui::Bounds::new(gpui::Point::default(), window.viewport_size()));
        let dialog_body = visual
            .debug_bounds("scrollbar-overlay")
            .expect("dialog body scrollbar viewport missing");
        let footer = visual
            .debug_bounds("resource-config-footer")
            .expect("dialog footer missing");
        assert_inside_window(dialog);
        assert_inside_window(dialog_body);
        assert_inside_window(footer);
        assert!(!bounds_overlap(dialog_body, footer));
        let first_before = visual.debug_bounds("skill-row-0").unwrap().origin.y;
        visual.simulate_event(ScrollWheelEvent {
            position: dialog_body.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-320.))),
            ..Default::default()
        });
        draw_frames(&mut visual, 3);
        assert!(visual.debug_bounds("skill-row-0").unwrap().origin.y < first_before);
        let footer_after = visual.debug_bounds("resource-config-footer").unwrap();
        assert!((footer_after.top() - footer.top()).abs() <= px(2.));
        assert_eq!(footer_after.size, footer.size);

        config.update(cx, |config, cx| config.select_tab(ResourceTab::Plugins, cx));
        draw_frames(&mut visual, 4);
        assert!(visual.debug_bounds("plugins-config-list").is_some());
        assert!(visual.debug_bounds("plugin-row-0").is_some());
        assert!(visual.debug_bounds("plugin-row-29").is_some());
        assert!(visual.debug_bounds("resource-diagnostics").is_some());
        let dialog_body = visual.debug_bounds("scrollbar-overlay").unwrap();
        let footer = visual.debug_bounds("resource-config-footer").unwrap();
        assert_inside_window(dialog_body);
        assert_inside_window(footer);
        assert!(!bounds_overlap(dialog_body, footer));
    }

    #[gpui::test]
    fn all_skill_switches_are_disabled_while_any_skill_write_is_busy(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        let second_path = PathBuf::from("C:/fixture/skill-1/SKILL.md");
        let handle = cx.open_window(size(px(640.), px(480.)), move |window, cx| {
            let config = cx.new(|_| ResourceConfig {
                cwd: PathBuf::from("C:/fixture/project"),
                agent_dir: Some(PathBuf::from("C:/fixture/agent")),
                tab: ResourceTab::Skills,
                state: LoadState::Ready {
                    skills: pi_data::SkillScan {
                        skills: vec![fixture_skill(0), fixture_skill(1)],
                        project_resources_loaded: true,
                        ..Default::default()
                    },
                    plugins: pi_data::PluginScan {
                        project_resources_loaded: true,
                        ..Default::default()
                    },
                },
                load_generation: 0,
                busy_skill: Some(PathBuf::from("C:/fixture/skill-0/SKILL.md")),
            });
            *output.borrow_mut() = Some(config.clone());
            gpui_component::Root::new(config, window, cx)
        });
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        draw_frames(&mut visual, 3);
        let second_toggle = visual
            .debug_bounds("skill-toggle-1")
            .expect("second skill switch missing");
        visual.simulate_click(second_toggle.center(), Default::default());
        draw_frames(&mut visual, 2);
        let config = captured.borrow().clone().unwrap();
        config.update(cx, |config, _| {
            assert_eq!(
                config.busy_skill,
                Some(PathBuf::from("C:/fixture/skill-0/SKILL.md"))
            );
            let LoadState::Ready { skills, .. } = &config.state else {
                panic!("ready state expected");
            };
            assert_eq!(skills.skills[1].path, second_path);
            assert!(!skills.skills[1].disable_model_invocation);
        });
    }

    #[gpui::test]
    fn resource_config_renders_loading_empty_error_and_untrusted_states(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let agent = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".pi/skills/fixture")).unwrap();
        std::fs::write(
            project.path().join(".pi/skills/fixture/SKILL.md"),
            "---\nname: fixture\ndescription: Fixture\n---\n",
        )
        .unwrap();
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        let cwd = project.path().to_path_buf();
        let agent_path = agent.path().to_path_buf();
        let handle = cx.open_window(
            gpui::size(gpui::px(640.), gpui::px(480.)),
            move |window, cx| {
                let config = cx.new(|_| ResourceConfig {
                    cwd: cwd.clone(),
                    agent_dir: Some(agent_path.clone()),
                    tab: ResourceTab::Skills,
                    state: LoadState::Loading,
                    load_generation: 0,
                    busy_skill: None,
                });
                *output.borrow_mut() = Some(config.clone());
                gpui_component::Root::new(config, window, cx)
            },
        );
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..2 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert!(visual.debug_bounds("resource-config-loading").is_some());
        assert!(
            visual
                .debug_bounds("resource-config-loading-title")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("resource-config-loading-detail")
                .is_some()
        );
        let config = captured.borrow().clone().unwrap();
        config.update(cx, |config, cx| {
            config.state = LoadState::Ready {
                skills: pi_data::scan_skills(config.agent_dir.as_ref().unwrap(), &config.cwd, None),
                plugins: pi_data::scan_plugin_packages(
                    config.agent_dir.as_ref().unwrap(),
                    &config.cwd,
                    None,
                ),
            };
            cx.notify();
        });
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("resource-config").is_some());
        assert!(
            visual
                .debug_bounds("resource-config-empty-skills")
                .is_some(),
            "未信任项目的列表为空时应保留独立 empty selector"
        );
        assert!(visual.debug_bounds("resource-config-untrusted").is_some());
        assert!(visual.debug_bounds("skills-config-list").is_none());
        assert_eq!(visual.debug_bounds("skill-row-0"), None);
        config.update(cx, |config, cx| {
            config.state = LoadState::Error("fixture error".to_owned());
            cx.notify();
        });
        for _ in 0..2 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert!(visual.debug_bounds("resource-config-error").is_some());
        assert!(visual.debug_bounds("resource-config-error-title").is_some());
        assert!(
            visual
                .debug_bounds("resource-config-error-detail")
                .is_some()
        );

        config.update(cx, |config, cx| {
            config.state = LoadState::Ready {
                skills: pi_data::SkillScan {
                    project_resources_loaded: true,
                    ..Default::default()
                },
                plugins: pi_data::PluginScan {
                    project_resources_loaded: true,
                    ..Default::default()
                },
            };
            cx.notify();
        });
        for _ in 0..2 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
        }
        assert!(
            visual
                .debug_bounds("resource-config-empty-skills")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("resource-config-empty-skills-title")
                .is_some()
        );
        assert!(
            visual
                .debug_bounds("resource-config-empty-skills-detail")
                .is_some()
        );
    }

    #[gpui::test]
    fn resource_config_renders_trusted_skills_plugins_diagnostics_and_toggles_frontmatter(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).expect("font init failed");
        });
        let agent = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let skill_path = project.path().join(".pi/skills/fixture/SKILL.md");
        std::fs::create_dir_all(skill_path.parent().unwrap()).unwrap();
        std::fs::write(
            &skill_path,
            "---\nname: fixture\ndescription: Fixture skill\nunknown: keep\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(
            agent.path().join("settings.json"),
            r#"{"packages":["npm:fixture"]}"#,
        )
        .unwrap();
        pi_data::trust_project(agent.path(), project.path()).unwrap();
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let output = captured.clone();
        let agent_path = agent.path().to_path_buf();
        let cwd = project.path().to_path_buf();
        let handle = cx.open_window(
            gpui::size(gpui::px(720.), gpui::px(560.)),
            move |window, cx| {
                let skills = pi_data::scan_skills(&agent_path, &cwd, None);
                let mut plugins = pi_data::scan_plugin_packages(&agent_path, &cwd, None);
                plugins.diagnostics.push(pi_data::ResourceDiagnostic {
                    path: agent_path.join("settings.json"),
                    message: "fixture diagnostic".to_owned(),
                });
                let config = cx.new(|_| ResourceConfig {
                    cwd: cwd.clone(),
                    agent_dir: Some(agent_path.clone()),
                    tab: ResourceTab::Skills,
                    state: LoadState::Ready { skills, plugins },
                    load_generation: 0,
                    busy_skill: None,
                });
                *output.borrow_mut() = Some(config.clone());
                gpui_component::Root::new(config, window, cx)
            },
        );
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("skills-config-list").is_some());
        assert!(visual.debug_bounds("skill-row-0").is_some());
        assert!(visual.debug_bounds("skill-toggle-0").is_some());
        let config = captured.borrow().clone().unwrap();
        let revision = config.read_with(cx, |config, _| match &config.state {
            LoadState::Ready { skills, .. } => skills.skills[0].revision.clone(),
            _ => panic!("ready state expected"),
        });
        visual.update(|window, cx| {
            config.update(cx, |config, cx| {
                config.toggle_skill(skill_path.clone(), revision, true, window, cx);
            });
        });
        visual.run_until_parked();
        let updated = std::fs::read_to_string(&skill_path).unwrap();
        assert!(updated.contains("disable-model-invocation: true"));
        assert!(updated.contains("unknown: keep"));

        let malformed_path = project.path().join(".pi/skills/malformed/SKILL.md");
        std::fs::create_dir_all(malformed_path.parent().unwrap()).unwrap();
        let malformed_content = "---\nname: malformed\ndescription: Missing close\nbody\n";
        std::fs::write(&malformed_path, malformed_content).unwrap();
        let metadata = std::fs::metadata(&malformed_path).unwrap();
        let malformed_revision = pi_data::FileRevision {
            len: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos()),
        };
        visual.update(|window, cx| {
            config.update(cx, |config, cx| {
                config.toggle_skill(malformed_path.clone(), malformed_revision, true, window, cx);
            });
        });
        visual.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(&malformed_path).unwrap(),
            malformed_content
        );
        config.update(cx, |config, cx| {
            if let LoadState::Ready { plugins, .. } = &mut config.state {
                plugins.diagnostics.push(pi_data::ResourceDiagnostic {
                    path: config.agent_dir.as_ref().unwrap().join("settings.json"),
                    message: "fixture diagnostic".to_owned(),
                });
            }
            config.select_tab(ResourceTab::Plugins, cx);
        });
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        assert!(visual.debug_bounds("plugins-config-list").is_some());
        assert!(visual.debug_bounds("plugin-row-0").is_some());
        assert!(visual.debug_bounds("resource-diagnostics").is_some());
        assert!(visual.debug_bounds("resource-diagnostic-0").is_some());
    }

    #[gpui::test]
    fn resource_config_reload_generation_rejects_stale_scan(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let agent = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut config = ResourceConfig {
            cwd: project.path().to_path_buf(),
            agent_dir: Some(agent.path().to_path_buf()),
            tab: ResourceTab::Skills,
            state: LoadState::Loading,
            load_generation: 4,
            busy_skill: None,
        };
        let empty = || {
            Ok((
                pi_data::SkillScan::default(),
                pi_data::PluginScan::default(),
            ))
        };
        assert!(!config.finish_reload(3, empty()));
        assert!(matches!(config.state, LoadState::Loading));
        assert!(config.finish_reload(4, empty()));
        assert!(matches!(config.state, LoadState::Ready { .. }));
    }
}
