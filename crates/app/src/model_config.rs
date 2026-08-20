use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Placement, Sizable as _, StyledExt as _,
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputContentType, InputState},
    notification::Notification,
    scroll::ScrollableElement as _,
    v_flex,
};
use pi_data::{
    AuthKind, AuthSummary, ModelApi, ModelConfigDocument, ProviderConfig, ProviderDescriptor,
    ProviderDraft, SecretString, merge_provider_directory, read_auth_summaries, remove_api_key,
    write_api_key,
};

use crate::{
    live_session::official_binary,
    model_service::{
        AuthCheckStatus, CancellationToken, ConnectivityStatus, ModelService, ModelServiceError,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ModelAction {
    Refresh,
    Save,
    ApiKey,
    Discover,
    Test,
    Login,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderBoundAction {
    Discover,
    Test,
    Login,
    ApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectivityNotice {
    Success(&'static str),
    Warning(&'static str),
}

const PROVIDER_INPUT_MISMATCH_MESSAGE: &str = "Provider ID 已修改；请先保存配置再操作";

#[derive(Debug, Default)]
struct ModelConfigState {
    open: bool,
    selected_provider: Option<String>,
    busy: HashSet<ModelAction>,
    generation: u64,
    error: Option<String>,
}

impl ModelConfigState {
    fn begin(&mut self, action: ModelAction) -> Option<u64> {
        if !self.busy.insert(action) {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        Some(self.generation)
    }

    fn finish(&mut self, action: ModelAction, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.busy.remove(&action);
        true
    }

    fn can_select_provider(&self) -> bool {
        self.busy.is_empty()
    }

    fn select(&mut self, provider: String, preserve_refresh: bool) {
        self.selected_provider = Some(provider);
        self.error = None;
        if !preserve_refresh {
            self.generation = self.generation.wrapping_add(1);
        }
        self.busy
            .retain(|action| preserve_refresh && matches!(action, ModelAction::Refresh));
    }

    fn close(&mut self) {
        self.open = false;
        self.generation = self.generation.wrapping_add(1);
        self.busy.clear();
        self.error = None;
    }
}

pub struct ModelConfigPanel {
    focus_handle: FocusHandle,
    state: ModelConfigState,
    agent_dir: Option<PathBuf>,
    service: Option<Arc<ModelService>>,
    providers: Vec<ProviderDescriptor>,
    configured: BTreeMap<String, ProviderConfig>,
    auth: BTreeMap<String, AuthSummary>,
    cli_auth: Option<AuthCheckStatus>,
    discover_cancel: Option<CancellationToken>,
    test_cancel: Option<CancellationToken>,
    login_cancel: Option<CancellationToken>,
    provider_id: Entity<InputState>,
    base_url: Entity<InputState>,
    model_ids: Entity<InputState>,
    api_key: Entity<InputState>,
    api: Option<ModelApi>,
    api_raw: Option<String>,
    api_modified: bool,
    models_rewrite_warning: bool,
}

impl ModelConfigPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let agent_dir = pi_data::agent_dir();
        let service = agent_dir
            .clone()
            .map(|dir| Arc::new(ModelService::new(official_binary(), dir)));
        let provider_id = cx.new(|cx| InputState::new(window, cx).placeholder("provider-id"));
        let base_url =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/v1"));
        let model_ids = cx.new(|cx| InputState::new(window, cx).placeholder("model-a, model-b"));
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("输入新 API Key（不会回显已有值）")
        });
        Self {
            focus_handle: cx.focus_handle(),
            state: ModelConfigState::default(),
            agent_dir,
            service,
            providers: pi_data::built_in_providers(),
            configured: BTreeMap::new(),
            auth: BTreeMap::new(),
            cli_auth: None,
            discover_cancel: None,
            test_cancel: None,
            login_cancel: None,
            provider_id,
            base_url,
            model_ids,
            api_key,
            api: None,
            api_raw: None,
            api_modified: false,
            models_rewrite_warning: false,
        }
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_sheet(cx) {
            return;
        }
        self.state.open = true;
        self.refresh(window, cx);
        let panel = cx.entity();
        window.open_sheet_at(Placement::Right, cx, move |sheet, _, _| {
            sheet
                .size(px(720.))
                .resizable(true)
                .title("模型与认证")
                .on_close({
                    let panel = panel.clone();
                    move |_, window, cx| {
                        panel.update(cx, |panel, cx| panel.close(window, cx));
                    }
                })
                .child(panel.clone())
        });
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(generation) = self.state.begin(ModelAction::Refresh) else {
            return;
        };
        self.state.error = None;
        self.cli_auth = None;
        let Some(agent_dir) = self.agent_dir.clone() else {
            self.state.error = Some("无法定位 pi agent 数据目录".into());
            self.state.busy.remove(&ModelAction::Refresh);
            cx.notify();
            return;
        };
        let document = ModelConfigDocument::load(&agent_dir);
        self.models_rewrite_warning = document
            .as_ref()
            .is_ok_and(ModelConfigDocument::has_rewrite_trivia);
        match (
            document.and_then(|document| document.providers()),
            read_auth_summaries(&agent_dir),
        ) {
            (Ok(configured), Ok(auth)) => {
                self.configured = configured
                    .into_iter()
                    .map(|provider| (provider.id.clone(), provider))
                    .collect();
                self.auth = auth
                    .into_iter()
                    .map(|summary| (summary.provider_id.clone(), summary))
                    .collect();
                self.providers = merge_provider_directory(
                    &self.configured.values().cloned().collect::<Vec<_>>(),
                    std::iter::empty(),
                );
                if self.state.selected_provider.is_none() {
                    let selected = self
                        .configured
                        .keys()
                        .next()
                        .cloned()
                        .or_else(|| Some("anthropic".into()));
                    if let Some(selected) = selected {
                        self.select_provider_inner(selected, true, window, cx);
                    }
                } else if let Some(selected) = self.state.selected_provider.clone() {
                    self.populate_form(&selected, window, cx);
                }
            }
            (Err(error), _) | (_, Err(error)) => {
                self.state.error = Some(error.to_string());
                self.state.busy.remove(&ModelAction::Refresh);
                cx.notify();
                return;
            }
        }

        let Some(service) = self.service.clone() else {
            self.state.busy.remove(&ModelAction::Refresh);
            cx.notify();
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor.spawn(async move { service.list_models() }).await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::Refresh, generation) {
                        return;
                    }
                    match result {
                        Ok(models) => {
                            panel.providers = merge_provider_directory(
                                &panel.configured.values().cloned().collect::<Vec<_>>(),
                                models.into_iter().map(|model| model.provider),
                            );
                            panel.refresh_selected_auth(window, cx);
                        }
                        Err(error) => {
                            panel.state.error = Some(error.to_string());
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn refresh_selected_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(provider) = self.state.selected_provider.clone() else {
            return;
        };
        let Some(service) = self.service.clone() else {
            return;
        };
        let generation = self.state.generation;
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move { service.check_auth(&provider) })
                .await;
            let _ = panel.update(cx, |panel, cx| {
                if panel.state.open && generation == panel.state.generation {
                    match result {
                        Ok(status) => panel.cli_auth = Some(status),
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn select_provider(&mut self, provider: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.can_select_provider() {
            return;
        }
        self.select_provider_inner(provider, false, window, cx);
    }

    fn select_provider_inner(
        &mut self,
        provider: String,
        preserve_refresh: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_operations();
        self.state.select(provider.clone(), preserve_refresh);
        self.cli_auth = None;
        self.clear_secret(window, cx);
        self.populate_form(&provider, window, cx);
        self.refresh_selected_auth(window, cx);
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_operations();
        self.state.close();
        self.clear_secret(window, cx);
        cx.notify();
    }

    fn cancel_operations(&mut self) {
        for token in [
            self.discover_cancel.take(),
            self.test_cancel.take(),
            self.login_cancel.take(),
        ]
        .into_iter()
        .flatten()
        {
            token.cancel();
        }
    }

    fn populate_form(&mut self, provider: &str, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.configured.get(provider);
        self.provider_id.update(cx, |input, cx| {
            input.set_value(provider, window, cx);
        });
        self.base_url.update(cx, |input, cx| {
            input.set_value(
                config
                    .and_then(|config| config.base_url.as_deref())
                    .unwrap_or(""),
                window,
                cx,
            );
        });
        self.model_ids.update(cx, |input, cx| {
            let ids = config
                .map(|config| {
                    config
                        .models
                        .iter()
                        .map(|model| model.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            input.set_value(ids, window, cx);
        });
        self.api = config.and_then(|config| config.api);
        self.api_raw = config.and_then(|config| config.api_raw.clone());
        self.api_modified = false;
    }

    fn clear_secret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.api_key
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn save_provider(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(generation) = self.state.begin(ModelAction::Save) else {
            return;
        };
        let draft = ProviderDraft {
            id: self.provider_id.read(cx).value().trim().to_owned(),
            base_url: self.base_url.read(cx).value().trim().to_owned(),
            api: self.api_modified.then_some(self.api).flatten(),
            model_ids: split_model_ids(self.model_ids.read(cx).value()),
        };
        let Some(agent_dir) = self.agent_dir.clone() else {
            self.finish_error(ModelAction::Save, generation, "无法定位数据目录".into(), cx);
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let mut document = ModelConfigDocument::load(&agent_dir)?;
                    document.upsert_provider(&draft)?;
                    document.save()?;
                    Ok::<_, pi_data::ModelConfigError>(draft.id)
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::Save, generation) {
                        return;
                    }
                    match result {
                        Ok(provider) => {
                            panel.state.selected_provider = Some(provider);
                            window.push_notification(Notification::success("模型配置已保存"), cx);
                            panel.refresh(window, cx);
                        }
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn save_api_key(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(generation) = self.state.begin(ModelAction::ApiKey) else {
            return;
        };
        let Some(provider) = self.selected_provider_for_action(ProviderBoundAction::ApiKey, cx)
        else {
            self.finish_error(
                ModelAction::ApiKey,
                generation,
                PROVIDER_INPUT_MISMATCH_MESSAGE.into(),
                cx,
            );
            return;
        };
        let value = self.api_key.read(cx).value().to_string();
        // 在发起后台写入前立即从可视 UI state 清空；SecretString 自身不可 Debug/Display。
        self.api_key
            .update(cx, |input, cx| input.set_value("", window, cx));
        let key = match SecretString::new(value) {
            Ok(key) => key,
            Err(error) => {
                self.finish_error(ModelAction::ApiKey, generation, error.to_string(), cx);
                return;
            }
        };
        let Some(agent_dir) = self.agent_dir.clone() else {
            self.finish_error(
                ModelAction::ApiKey,
                generation,
                "无法定位数据目录".into(),
                cx,
            );
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move { write_api_key(agent_dir, &provider, key) })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::ApiKey, generation) {
                        return;
                    }
                    match result {
                        Ok(()) => {
                            window
                                .push_notification(Notification::success("API Key 已安全保存"), cx);
                            panel.refresh(window, cx);
                        }
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn remove_api_key(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(generation) = self.state.begin(ModelAction::ApiKey) else {
            return;
        };
        let Some(provider) = self.selected_provider_for_action(ProviderBoundAction::ApiKey, cx)
        else {
            self.finish_error(
                ModelAction::ApiKey,
                generation,
                PROVIDER_INPUT_MISMATCH_MESSAGE.into(),
                cx,
            );
            return;
        };
        let Some(agent_dir) = self.agent_dir.clone() else {
            self.finish_error(
                ModelAction::ApiKey,
                generation,
                "无法定位数据目录".into(),
                cx,
            );
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move { remove_api_key(agent_dir, &provider) })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::ApiKey, generation) {
                        return;
                    }
                    match result {
                        Ok(true) => {
                            window.push_notification(Notification::success("API Key 已移除"), cx);
                            panel.refresh(window, cx);
                        }
                        Ok(false) => {
                            window.push_notification(
                                Notification::warning("未移除：当前没有可移除的已保存 API Key"),
                                cx,
                            );
                            panel.refresh(window, cx);
                        }
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn discover(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(token) = self.discover_cancel.take() {
            token.cancel();
            self.state.busy.remove(&ModelAction::Discover);
        }
        let Some(generation) = self.state.begin(ModelAction::Discover) else {
            return;
        };
        let Some(provider) = self.selected_provider_for_action(ProviderBoundAction::Discover, cx)
        else {
            self.finish_error(
                ModelAction::Discover,
                generation,
                PROVIDER_INPUT_MISMATCH_MESSAGE.into(),
                cx,
            );
            return;
        };
        let base_url = self.base_url.read(cx).value().trim().to_owned();
        let Some(api) = self.api else {
            self.finish_error(
                ModelAction::Discover,
                generation,
                "当前 API 类型不受发现功能支持；请显式选择受支持类型后重试".into(),
                cx,
            );
            return;
        };
        let (Some(agent_dir), Some(service)) = (self.agent_dir.clone(), self.service.clone())
        else {
            self.finish_error(
                ModelAction::Discover,
                generation,
                "模型服务不可用".into(),
                cx,
            );
            return;
        };
        let cancel = CancellationToken::default();
        self.discover_cancel = Some(cancel.clone());
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let key = pi_data::read_api_key(agent_dir, &provider)?;
                    service.discover_models(&base_url, api, key.as_ref(), &cancel)
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::Discover, generation) {
                        return;
                    }
                    panel.discover_cancel = None;
                    match result {
                        Ok(models) => {
                            panel.model_ids.update(cx, |input, cx| {
                                input.set_value(models.join(", "), window, cx)
                            });
                            window.push_notification(
                                Notification::success(format!("发现 {} 个模型", models.len())),
                                cx,
                            );
                        }
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn test_connectivity(
        &mut self,
        _: &gpui::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(token) = self.test_cancel.take() {
            token.cancel();
            self.state.busy.remove(&ModelAction::Test);
        }
        let Some(generation) = self.state.begin(ModelAction::Test) else {
            return;
        };
        let Some(provider) = self.selected_provider_for_action(ProviderBoundAction::Test, cx)
        else {
            self.finish_error(
                ModelAction::Test,
                generation,
                PROVIDER_INPUT_MISMATCH_MESSAGE.into(),
                cx,
            );
            return;
        };
        let base_url = self.base_url.read(cx).value().trim().to_owned();
        let model = split_model_ids(self.model_ids.read(cx).value())
            .into_iter()
            .next()
            .unwrap_or_default();
        let Some(api) = self.api else {
            self.finish_error(
                ModelAction::Test,
                generation,
                "当前 API 类型不受连通性测试支持；请显式选择受支持类型后重试".into(),
                cx,
            );
            return;
        };
        let (Some(agent_dir), Some(service)) = (self.agent_dir.clone(), self.service.clone())
        else {
            self.finish_error(ModelAction::Test, generation, "模型服务不可用".into(), cx);
            return;
        };
        let cancel = CancellationToken::default();
        self.test_cancel = Some(cancel.clone());
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let result = executor
                .spawn(async move {
                    let key = pi_data::read_api_key(agent_dir, &provider)?;
                    service.test_connectivity(&base_url, api, &model, key.as_ref(), &cancel)
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::Test, generation) {
                        return;
                    }
                    panel.test_cancel = None;
                    match result {
                        Ok(result) => match connectivity_notice(result.status) {
                            ConnectivityNotice::Success(message) => {
                                window.push_notification(Notification::success(message), cx)
                            }
                            ConnectivityNotice::Warning(message) => {
                                window.push_notification(Notification::warning(message), cx)
                            }
                        },
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn login(&mut self, _: &gpui::ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(token) = self.login_cancel.take() {
            token.cancel();
            self.state.busy.remove(&ModelAction::Login);
        }
        let Some(generation) = self.state.begin(ModelAction::Login) else {
            return;
        };
        let Some(provider) = self.selected_provider_for_action(ProviderBoundAction::Login, cx)
        else {
            self.finish_error(
                ModelAction::Login,
                generation,
                PROVIDER_INPUT_MISMATCH_MESSAGE.into(),
                cx,
            );
            return;
        };
        let instruction = login_instruction(&provider);
        let Some(service) = self.service.clone() else {
            self.finish_error(ModelAction::Login, generation, "官方 pi 不可用".into(), cx);
            return;
        };
        window.push_notification(
            Notification::warning(format!(
                "新终端只会启动官方 pi。进入终端后请手动输入：{instruction}"
            )),
            cx,
        );
        let cancel = CancellationToken::default();
        self.login_cancel = Some(cancel.clone());
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |panel, cx| {
            let login_result = executor
                .spawn({
                    let service = service.clone();
                    let provider = provider.clone();
                    let login_cancel = cancel.clone();
                    async move { service.run_login(&provider, &login_cancel) }
                })
                .await;
            if cancel.is_cancelled() {
                return;
            }
            let auth_result = executor
                .spawn({
                    let service = service.clone();
                    let provider = provider.clone();
                    async move { service.check_auth(&provider) }
                })
                .await;
            let _ = cx.update(|window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    if !panel.state.open || !panel.state.finish(ModelAction::Login, generation) {
                        return;
                    }
                    panel.login_cancel = None;
                    match auth_result {
                        Ok(status) => panel.cli_auth = Some(status),
                        Err(error) => panel.state.error = Some(error.to_string()),
                    }
                    match login_result {
                        Ok(status) if status.success() => window.push_notification(
                            Notification::success(format!(
                                "官方 pi 已返回，认证状态已校准。若尚未登录，请重新打开终端并手动输入：{instruction}"
                            )),
                            cx,
                        ),
                        Ok(status) => window.push_notification(
                            Notification::warning(format!(
                                "官方 pi 以 exit {:?} 返回；认证状态已重新校准。登录命令需在终端内手动输入：{instruction}",
                                status.code()
                            )),
                            cx,
                        ),
                        Err(ModelServiceError::Cancelled) => return,
                        Err(error) => window.push_notification(
                            Notification::warning(format!(
                                "官方 pi 登录未正常完成：{error}；认证状态已重新校准"
                            )),
                            cx,
                        ),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn set_api(&mut self, api: ModelApi, cx: &mut Context<Self>) {
        self.api = Some(api);
        self.api_raw = Some(api.as_str().into());
        self.api_modified = true;
        cx.notify();
    }

    fn finish_error(
        &mut self,
        action: ModelAction,
        generation: u64,
        error: String,
        cx: &mut Context<Self>,
    ) {
        if self.state.finish(action, generation) {
            self.state.error = Some(error);
            cx.notify();
        }
    }

    fn selected_provider_for_action(
        &self,
        action: ProviderBoundAction,
        cx: &App,
    ) -> Option<String> {
        let selected = self.state.selected_provider.as_ref()?;
        provider_action_allowed(action, selected, self.provider_id.read(cx).value().trim())
            .then(|| selected.clone())
    }

    fn selected_descriptor(&self) -> Option<&ProviderDescriptor> {
        let selected = self.state.selected_provider.as_deref()?;
        self.providers
            .iter()
            .find(|provider| provider.id == selected)
    }

    fn auth_label(&self) -> (&'static str, AuthKind) {
        match &self.cli_auth {
            Some(AuthCheckStatus::Ready { auth_type }) => (
                "已认证（pi 校准）",
                if auth_type.as_deref() == Some("oauth") {
                    AuthKind::OAuth
                } else {
                    AuthKind::ApiKey
                },
            ),
            Some(AuthCheckStatus::NotReady { .. }) => ("未认证", AuthKind::Unknown),
            Some(AuthCheckStatus::Invalid { .. }) => ("认证状态无效", AuthKind::Unknown),
            None => self
                .state
                .selected_provider
                .as_ref()
                .and_then(|provider| self.auth.get(provider))
                .map_or(("未配置", AuthKind::Unknown), |summary| {
                    if summary.configured {
                        if summary.external_env && !summary.has_key {
                            return ("由外部环境配置", AuthKind::ApiKey);
                        }
                        if summary.external_reference {
                            return ("由外部引用配置", AuthKind::ApiKey);
                        }
                        match summary.kind {
                            AuthKind::ApiKey => ("API Key 已保存", AuthKind::ApiKey),
                            AuthKind::OAuth => ("OAuth 已保存", AuthKind::OAuth),
                            AuthKind::Unknown => ("未知凭据", AuthKind::Unknown),
                        }
                    } else {
                        ("未配置", AuthKind::Unknown)
                    }
                }),
        }
    }
}

impl Focusable for ModelConfigPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ModelConfigPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.state.selected_provider.clone();
        let busy = !self.state.busy.is_empty();
        let (auth_label, auth_kind) = self.auth_label();
        let descriptor = self.selected_descriptor();
        let provider_id_matches_selection = self
            .selected_provider_for_action(ProviderBoundAction::Discover, cx)
            .is_some();
        let can_key = descriptor.is_none_or(|provider| provider.auth.accepts_api_key());
        let can_login = descriptor.is_some_and(|provider| provider.auth.accepts_login());
        let selected_has_key = selected
            .as_ref()
            .and_then(|provider| self.auth.get(provider))
            .is_some_and(|summary| summary.has_key);
        let provider_list_height = (self.providers.len() as f32 * 48.).clamp(96., 240.);
        let provider_rows = self.providers.iter().map(|provider| {
            let id = provider.id.clone();
            let is_selected = selected.as_deref() == Some(id.as_str());
            let auth = self.auth.get(&id).cloned();
            let panel = cx.entity();
            div()
                .id(SharedString::from(format!("provider-{id}")))
                .debug_selector(|| "model-provider-row".into())
                .w_full()
                .p_2()
                .rounded_md()
                .when(!busy, |row| row.cursor_pointer())
                .when(busy, |row| row.opacity(0.55))
                .when(is_selected, |row| row.bg(cx.theme().accent.opacity(0.16)))
                .when(!busy, |row| row.hover(|row| row.bg(cx.theme().muted)))
                .on_click(move |_, window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.select_provider(id.clone(), window, cx)
                    });
                })
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().size_2().rounded_full().bg(
                            if auth.as_ref().is_some_and(|auth| auth.configured) {
                                cx.theme().success
                            } else {
                                cx.theme().muted_foreground
                            },
                        ))
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_sm()
                                        .truncate()
                                        .child(provider.display_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(provider.id.clone()),
                                ),
                        ),
                )
        });
        let status_color = match auth_kind {
            AuthKind::ApiKey | AuthKind::OAuth => cx.theme().success,
            AuthKind::Unknown => cx.theme().warning,
        };
        let api_buttons = ModelApi::ALL.into_iter().map(|api| {
            let label = match api {
                ModelApi::OpenAiCompletions => "OpenAI Chat",
                ModelApi::OpenAiResponses => "OpenAI Responses",
                ModelApi::AnthropicMessages => "Anthropic",
                ModelApi::GoogleGenerativeAi => "Google",
            };
            let panel = cx.entity();
            Button::new(SharedString::from(format!("api-{}", api.as_str())))
                .small()
                .label(label)
                .when(self.api == Some(api), |button| button.primary())
                .when(self.api != Some(api), |button| button.secondary())
                .on_click(move |_, _, cx| panel.update(cx, |panel, cx| panel.set_api(api, cx)))
        });

        v_flex()
            .debug_selector(|| "model-config-sheet".into())
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h_0()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().size_2().rounded_full().bg(status_color))
                    .child(div().text_sm().child(auth_label))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("凭据正文从不读取到界面"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("refresh-model-config")
                            .ghost()
                            .small()
                            .icon(IconName::Redo)
                            .tooltip("刷新模型与认证状态")
                            .disabled(busy)
                            .on_click(
                                cx.listener(|panel, _, window, cx| panel.refresh(window, cx)),
                            ),
                    ),
            )
            .when_some(self.state.error.clone(), |view, error| {
                view.child(
                    div()
                        .debug_selector(|| "model-config-error".into())
                        .border_l_2()
                        .border_color(cx.theme().danger)
                        .pl_3()
                        .py_2()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .gap_4()
                    .child(
                        v_flex()
                            .w(px(190.))
                            .flex_none()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("PROVIDERS"),
                            )
                            .child(
                                v_flex()
                                    .h(px(provider_list_height))
                                    .overflow_y_scrollbar()
                                    .gap_1()
                                    .children(provider_rows),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_4()
                            .child(section_label("Provider 配置", cx))
                            .when(self.models_rewrite_warning, |view| {
                                view.child(
                                    div()
                                        .debug_selector(|| "models-rewrite-warning".into())
                                        .border_l_2()
                                        .border_color(cx.theme().warning)
                                        .pl_3()
                                        .py_2()
                                        .text_sm()
                                        .text_color(cx.theme().warning)
                                        .child("保存会重写 models.json 并移除注释及尾逗号。"),
                                )
                            })
                            .child(field("Provider ID", Input::new(&self.provider_id), cx))
                            .when(!provider_id_matches_selection, |view| {
                                view.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().warning)
                                        .child("Provider ID 已修改；请先保存配置再操作。"),
                                )
                            })
                            .child(field("Base URL", Input::new(&self.base_url), cx))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(field_label("API 类型", cx))
                                    .when(self.api.is_none(), |view| {
                                        view.child(
                                            div()
                                                .debug_selector(|| "unsupported-api-warning".into())
                                                .text_xs()
                                                .text_color(cx.theme().warning)
                                                .child(unsupported_api_message(
                                                    self.api_raw.as_deref(),
                                                )),
                                        )
                                    })
                                    .child(h_flex().flex_wrap().gap_1().children(api_buttons)),
                            )
                            .child(field(
                                "模型 ID（逗号分隔）",
                                Input::new(&self.model_ids),
                                cx,
                            ))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("save-provider")
                                            .primary()
                                            .label("保存配置")
                                            .disabled(busy)
                                            .on_click(cx.listener(Self::save_provider)),
                                    )
                                    .child(
                                        Button::new("discover-models")
                                            .secondary()
                                            .label("发现模型")
                                            .tooltip("调用 provider 的模型列表端点")
                                            .disabled(busy || !provider_id_matches_selection)
                                            .on_click(cx.listener(Self::discover)),
                                    )
                                    .child(
                                        Button::new("test-model-connectivity")
                                            .secondary()
                                            .label("测试连通性")
                                            .tooltip("会向第一个模型发送一次最小请求，可能产生计费")
                                            .disabled(busy || !provider_id_matches_selection)
                                            .on_click(cx.listener(Self::test_connectivity)),
                                    ),
                            )
                            .child(section_label("认证", cx))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(field_label("新 API Key", cx))
                                    .child(
                                        Input::new(&self.api_key)
                                            .content_type(InputContentType::Password)
                                            .mask_toggle()
                                            .cleanable(true),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("保存后立即清空输入；已有密钥只显示配置状态。"),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Button::new("save-api-key")
                                            .secondary()
                                            .label("保存 / 替换 Key")
                                            .disabled(
                                                busy || !can_key || !provider_id_matches_selection,
                                            )
                                                    .on_click(cx.listener(Self::save_api_key)),
                                            )
                                            .child(
                                                Button::new("remove-api-key")
                                            .danger()
                                            .label("移除 Key")
                                            .tooltip("仅删除 API Key，不会删除 OAuth 凭据")
                                            .disabled(
                                                busy
                                                    || !can_key
                                                    || !provider_id_matches_selection
                                                    || !selected_has_key,
                                            )
                                                    .on_click(cx.listener(Self::remove_api_key)),
                                            )
                                            .when(can_login, |row| {
                                                row.child(
                                                    Button::new("official-pi-login")
                                                .secondary()
                                                .icon(IconName::ExternalLink)
                                                .label("官方 pi 登录")
                                                .tooltip("新终端仅启动官方 pi；进入终端后按面板指引手动输入登录命令")
                                                .disabled(busy || !provider_id_matches_selection)
                                                        .on_click(cx.listener(Self::login)),
                                                )
                                            }),
                                    )
                                    .when(can_login, |row| {
                                        let command = selected
                                            .as_deref()
                                            .map(login_instruction)
                                            .unwrap_or_else(|| "/login <provider>".into());
                                        row.child(
                                            div()
                                                .debug_selector(|| "login-manual-instruction".into())
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!(
                                                    "新终端打开后，请手动输入精确命令：{command}"
                                                )),
                                        )
                                    }),
                            ),
                    ),
            )
    }
}

fn provider_action_allowed(
    _action: ProviderBoundAction,
    selected: &str,
    provider_input: &str,
) -> bool {
    selected == provider_input
}

fn connectivity_notice(status: ConnectivityStatus) -> ConnectivityNotice {
    match status {
        ConnectivityStatus::Reachable => ConnectivityNotice::Success("连通性测试成功"),
        ConnectivityStatus::AuthenticationRequired => {
            ConnectivityNotice::Warning("服务可达，但认证被拒绝")
        }
        ConnectivityStatus::RateLimited => ConnectivityNotice::Warning("服务可达，但触发限流"),
        ConnectivityStatus::ServerError => ConnectivityNotice::Warning("服务可达，但服务端出错"),
    }
}

fn login_instruction(provider: &str) -> String {
    format!("/login {provider}")
}

fn unsupported_api_message(api_raw: Option<&str>) -> String {
    match api_raw {
        Some(api) => format!("当前配置值 `{api}` 不受本面板编辑；不选择下方类型时保存会原样保留。"),
        None => "当前 provider 未设置 API 类型；保存前必须选择一个受支持的 API 类型。".into(),
    }
}

fn field<T: IntoElement>(label: &'static str, input: T, cx: &App) -> impl IntoElement + use<T> {
    v_flex().gap_2().child(field_label(label, cx)).child(input)
}

fn field_label(label: &'static str, cx: &App) -> impl IntoElement + use<> {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

fn section_label(label: &'static str, cx: &App) -> impl IntoElement + use<> {
    h_flex()
        .gap_2()
        .child(Icon::new(IconName::Settings).small())
        .child(div().text_base().font_semibold().child(label))
        .child(div().flex_1().h_px().bg(cx.theme().border.opacity(0.6)))
}

fn split_model_ids(value: SharedString) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext, size};
    use gpui_component::Root;

    use super::*;

    #[test]
    fn state_prevents_reentry_and_stale_generation() {
        let mut state = ModelConfigState::default();
        let first = state.begin(ModelAction::Discover).unwrap();
        assert!(state.begin(ModelAction::Discover).is_none());
        state.select("openai".into(), false);
        assert!(!state.finish(ModelAction::Discover, first));
        assert!(state.busy.is_empty());
    }

    #[test]
    fn first_selection_preserves_refresh_generation_and_busy_state() {
        let mut state = ModelConfigState::default();
        let refresh = state.begin(ModelAction::Refresh).unwrap();
        state.select("anthropic".into(), true);
        assert_eq!(state.generation, refresh);
        assert!(state.busy.contains(&ModelAction::Refresh));
        assert!(state.finish(ModelAction::Refresh, refresh));
        assert!(state.busy.is_empty());
    }

    #[test]
    fn busy_state_blocks_user_provider_selection_without_mutation() {
        let mut state = ModelConfigState {
            selected_provider: Some("openai".into()),
            ..Default::default()
        };
        let generation = state.begin(ModelAction::Save).unwrap();
        let busy = state.busy.clone();

        if state.can_select_provider() {
            state.select("anthropic".into(), false);
        }

        assert_eq!(state.selected_provider.as_deref(), Some("openai"));
        assert_eq!(state.generation, generation);
        assert_eq!(state.busy, busy);
    }

    #[test]
    fn cancellation_token_cancels_repeated_and_closed_operations() {
        let discover = CancellationToken::default();
        let test = CancellationToken::default();
        let login = CancellationToken::default();
        let mut panel_tokens = [
            Some(discover.clone()),
            Some(test.clone()),
            Some(login.clone()),
        ];
        for token in panel_tokens.iter_mut().filter_map(Option::take) {
            token.cancel();
        }
        assert!(discover.is_cancelled());
        assert!(test.is_cancelled());
        assert!(login.is_cancelled());
    }

    #[test]
    fn model_ids_are_trimmed_and_empty_values_removed() {
        assert_eq!(split_model_ids(" a, b\n\n c ".into()), vec!["a", "b", "c"]);
    }

    #[test]
    fn login_ui_instruction_contains_exact_manual_command() {
        assert_eq!(login_instruction("openai-codex"), "/login openai-codex");
    }

    #[test]
    fn unsupported_api_message_distinguishes_unknown_from_missing() {
        assert!(unsupported_api_message(Some("google-vertex")).contains("原样保留"));
        let missing = unsupported_api_message(None);
        assert!(missing.contains("保存前必须选择"));
        assert!(!missing.contains("原样保留"));
    }

    #[test]
    fn provider_input_mismatch_blocks_all_provider_bound_actions() {
        for action in [
            ProviderBoundAction::Discover,
            ProviderBoundAction::Test,
            ProviderBoundAction::Login,
            ProviderBoundAction::ApiKey,
        ] {
            assert!(provider_action_allowed(
                action,
                "openai-codex",
                "openai-codex"
            ));
            assert!(!provider_action_allowed(
                action,
                "openai-codex",
                "edited-provider"
            ));
        }
    }

    #[test]
    fn connectivity_status_maps_to_accurate_notification_level() {
        assert_eq!(
            connectivity_notice(ConnectivityStatus::Reachable),
            ConnectivityNotice::Success("连通性测试成功")
        );
        for (status, message) in [
            (
                ConnectivityStatus::AuthenticationRequired,
                "服务可达，但认证被拒绝",
            ),
            (ConnectivityStatus::RateLimited, "服务可达，但触发限流"),
            (ConnectivityStatus::ServerError, "服务可达，但服务端出错"),
        ] {
            assert_eq!(
                connectivity_notice(status),
                ConnectivityNotice::Warning(message)
            );
        }
    }

    #[gpui::test]
    fn minimum_window_opens_real_sheet_and_close_clears_secret(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            gpui_pi_ui::theme::init_fonts(cx).unwrap();
        });
        let mut panel_entity = None;
        let handle = cx.open_window(size(px(800.), px(560.)), |window, cx| {
            let panel = cx.new(|cx| ModelConfigPanel::new(window, cx));
            panel_entity = Some(panel.clone());
            Root::new(panel, window, cx)
        });
        let panel = panel_entity.unwrap();
        cx.update_window(handle.into(), |_, window, cx| {
            panel.update(cx, |panel, cx| {
                panel.agent_dir = None;
                panel.models_rewrite_warning = true;
                panel.api_raw = Some("google-vertex".into());
                panel.api = None;
                panel
                    .api_key
                    .update(cx, |input, cx| input.set_value("secret", window, cx));
                panel.open(window, cx);
            });
            assert!(window.has_active_sheet(cx));
        })
        .unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        for _ in 0..3 {
            visual.update(|window, cx| window.draw(cx).clear(cx));
            visual.run_until_parked();
        }
        let sheet = visual
            .debug_bounds("model-config-sheet")
            .expect("model config missing");
        assert!(sheet.size.width > px(500.));
        assert!(sheet.size.height > px(400.));
        assert!(visual.debug_bounds("models-rewrite-warning").is_some());
        assert!(visual.debug_bounds("unsupported-api-warning").is_some());
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.close(window, cx));
            window.close_sheet(cx);
        });
        visual.run_until_parked();
        assert!(panel.read_with(&visual, |panel, cx| {
            !panel.state.open && panel.api_key.read(cx).value().is_empty()
        }));
    }
}
