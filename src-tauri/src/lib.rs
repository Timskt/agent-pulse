pub mod adapters;
pub mod ai_judge;
pub mod config;
pub mod cost;
pub mod detector;
pub mod i18n;
pub mod monitor;
pub mod notify;
pub mod remote;
pub mod resumer;
pub mod storage;
pub mod webhook;

use config::{AppConfig, ConfigManager};
use i18n::I18n;
use monitor::{EngineEvent, EngineStatus, LogLevel, MonitorEngine, MonitorState};
use notify::Notifier;
use remote::RemoteService;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, Wry};

/// 应用全局状态
pub struct AppState {
    pub engine: Arc<MonitorEngine>,
    /// 用 Arc 是为了让引擎和命令层共享同一个管理器——
    /// 配置改完引擎下一轮就能读到，不必重启监控
    pub config_manager: Arc<ConfigManager>,
    pub storage: Arc<storage::Storage>,
    pub notifier: Arc<Notifier>,
    /// 手机看板；配置里没开时它就是个空壳，不占端口
    pub remote: Arc<RemoteService>,
}

impl AppState {
    /// 当前语言的查表器（命令返回给用户的文字都要过这里）
    fn i18n(&self) -> I18n {
        I18n::from_code(&self.config_manager.get().language)
    }
}

// ==================== Tauri Commands ====================

/// 获取当前监控状态
#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<MonitorState, String> {
    Ok(state.engine.state.lock().await.clone())
}

/// 获取引擎状态摘要
#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<EngineStatus, String> {
    Ok(state.engine.state.lock().await.status.clone())
}

/// 启动监控
///
/// 事件推送泵常驻在 `setup` 里，所以从托盘启动也能看到日志。
#[tauri::command]
async fn start_monitoring(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    {
        let s = state.engine.state.lock().await;
        if s.running {
            return Err(state.i18n().t("err.already_running").to_string());
        }
    }

    let engine = state.engine.clone();
    tauri::async_runtime::spawn(async move {
        engine.start().await;
        let _ = app.emit("engine-stopped", ());
    });
    Ok(())
}

/// 停止监控
#[tauri::command]
async fn stop_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    // 停止日志由引擎自己写（它才知道要不要顺手清角标）
    state.engine.stop().await;
    Ok(())
}

/// 立即执行一次扫描
#[tauri::command]
async fn scan_now(state: State<'_, AppState>) -> Result<MonitorState, String> {
    state.engine.scan_once().await;
    Ok(state.engine.state.lock().await.clone())
}

/// 获取配置
#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config_manager.get())
}

/// 更新配置
///
/// 语言变了要顺手把托盘菜单重建一遍，否则会出现「界面英文、托盘中文」
/// 那种最扎眼的搭配。看板同理：端口或「允许局域网访问」改了要立刻生效，
/// 不能等下次开应用。
#[tauri::command]
async fn update_config(
    state: State<'_, AppState>,
    app: AppHandle,
    config: AppConfig,
) -> Result<(), String> {
    let old_lang = state.config_manager.get().language;
    let new_lang = config.language.clone();
    state.config_manager.update(config)?;

    if old_lang != new_lang {
        if let Some(tray) = app.tray_by_id(notify::TRAY_ID) {
            if let Ok(menu) = build_tray_menu(&app, &new_lang) {
                let _ = tray.set_menu(Some(menu));
            }
            let _ = tray.set_tooltip(Some(I18n::from_code(&new_lang).t("tray.tooltip")));
        }
    }

    // 令牌为空时 sync 会补一个并回写配置，所以前端保存后要重新拉一次配置
    state.remote.sync().await;
    Ok(())
}

/// 手动对指定会话触发续跑
#[tauri::command]
async fn manual_resume(
    state: State<'_, AppState>,
    session_id: String,
    use_goal_prompt: Option<bool>,
) -> Result<String, String> {
    let i18n = state.i18n();
    let session = {
        let s = state.engine.state.lock().await;
        s.sessions
            .iter()
            .find(|sess| sess.id == session_id)
            .cloned()
            .ok_or_else(|| i18n.t("err.session_not_found").to_string())?
    };

    let use_goal = use_goal_prompt.unwrap_or(false);
    let resumer = resumer::Resumer::new(state.config_manager.get());
    let outcome = resumer.resume(&session, use_goal).await;

    // 手动续跑也要进统计，否则「成功率」只反映自动续跑
    let prompt_type = if use_goal { "goal" } else { "generic" };
    let (ok, detail) = match &outcome {
        Ok(msg) => (true, msg.clone()),
        Err(e) => (false, e.clone()),
    };
    state.storage.record_resume(
        &session.id,
        &session.agent_name,
        &session.working_dir,
        prompt_type,
        ok,
        &detail,
    );

    state
        .engine
        .push_event_public(EngineEvent::new(
            if ok { LogLevel::Success } else { LogLevel::Error },
            Some(session_id),
            i18n.tf("log.resume_manual", &[("detail", &detail)]),
        ))
        .await;

    outcome
}

/// 跳到会话所在的终端标签页
///
/// 这是「通知点了之后去哪」的收口：桌面三平台都没有可靠统一的通知点击回调，
/// 所以动作放在应用内——前端拿提醒事件里的 `session_id` 调这里。
#[tauri::command]
async fn focus_terminal(state: State<'_, AppState>, session_id: String) -> Result<String, String> {
    let i18n = state.i18n();
    let session = {
        let s = state.engine.state.lock().await;
        s.sessions
            .iter()
            .find(|sess| sess.id == session_id)
            .cloned()
            .ok_or_else(|| i18n.t("err.session_not_found").to_string())?
    };

    let lang = state.config_manager.get().language;
    let detail = resumer::focus_session(&session, &lang).await?;
    state
        .engine
        .push_event_public(EngineEvent::new(
            LogLevel::Info,
            Some(session_id),
            i18n.tf("log.focused", &[("detail", &detail)]),
        ))
        .await;
    Ok(detail)
}

/// 续跑演练：走完全部定位流程，但**一个字都不敲**
///
/// 「按下去才知道会发生什么」是这个功能最大的心理负担——尤其在 IDE 里开的
/// 终端上，敲错窗口的代价是把提示词打进别人的代码。演练把这件事变成零风险：
/// 它回答「现在按续跑，字会落到哪儿」，以及「为什么落不到」。
#[tauri::command]
async fn probe_resume(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<resumer::ResumeProbe, String> {
    let session = {
        let s = state.engine.state.lock().await;
        s.sessions
            .iter()
            .find(|sess| sess.id == session_id)
            .cloned()
            .ok_or_else(|| state.i18n().t("err.session_not_found").to_string())?
    };
    Ok(resumer::probe_resume(&session, &state.config_manager.get()).await)
}

/// 一键跳到「辅助功能」设置页（macOS 专用）
#[tauri::command]
async fn open_accessibility_settings(state: State<'_, AppState>) -> Result<String, String> {
    resumer::open_accessibility_settings(&state.config_manager.get().language).await
}

/// 测试发送通知（验证整条提醒通道）
#[tauri::command]
async fn test_notify(state: State<'_, AppState>) -> Result<String, String> {
    let lang = state.config_manager.get().language;
    state.notifier.notify_test(&lang)
}

/// 获取当前平台信息
#[tauri::command]
async fn get_platform_info() -> Result<String, String> {
    Ok(format!(
        "{} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    ))
}

/// 获取每日检测/续跑统计（已补齐空缺日期，前端可直接画图）
#[tauri::command]
async fn get_stats(
    state: State<'_, AppState>,
    days: Option<u32>,
) -> Result<Vec<storage::DailyStats>, String> {
    Ok(state.storage.get_stats(days.unwrap_or(30)))
}

/// 获取最近续跑记录
#[tauri::command]
async fn get_resume_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<storage::ResumeRecord>, String> {
    Ok(state.storage.get_recent_resumes(limit.unwrap_or(50)))
}

/// 获取总体统计：(检测数, 续跑数, 成功续跑数)
#[tauri::command]
async fn get_totals(state: State<'_, AppState>) -> Result<(u32, u32, u32), String> {
    Ok(state.storage.get_totals())
}

/// 每日花费趋势
#[tauri::command]
async fn get_cost_daily(
    state: State<'_, AppState>,
    days: Option<u32>,
) -> Result<Vec<cost::DailyCost>, String> {
    Ok(state.storage.daily_costs(days.unwrap_or(14)))
}

/// 项目花费排行
#[tauri::command]
async fn get_cost_projects(
    state: State<'_, AppState>,
    days: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<cost::ProjectCost>, String> {
    Ok(state
        .storage
        .project_costs(days.unwrap_or(30), limit.unwrap_or(8)))
}

/// 限流窗口预测
#[tauri::command]
async fn get_rate_forecast(state: State<'_, AppState>) -> Result<cost::RateLimitForecast, String> {
    let cfg = state.config_manager.get().cost;
    let window = cfg.rate_limit_window_hours.max(1);
    Ok(cost::forecast_rate_limit(
        window,
        cfg.rate_limit_token_budget,
        state.storage.tokens_in_last_hours(window),
        state.storage.tokens_in_last_hours(1),
    ))
}

/// 会话历史时间线（可按项目/终端搜索）
#[tauri::command]
async fn get_session_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
    query: Option<String>,
) -> Result<Vec<storage::SessionHistoryEntry>, String> {
    Ok(state
        .storage
        .session_history(limit.unwrap_or(50), query.unwrap_or_default().trim()))
}

/// 测试外部推送通道（Slack / Discord / ntfy / Bark）
#[tauri::command]
async fn test_webhook(state: State<'_, AppState>) -> Result<String, String> {
    let config = state.config_manager.get();
    let lang = i18n::Lang::from_code(&config.language);
    webhook::WebhookNotifier::new(config.webhook, lang)
        .test()
        .await
}

/// AI 分析指定会话
#[tauri::command]
async fn ai_analyze(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<ai_judge::AiVerdict, String> {
    let config = state.config_manager.get();
    let judge = ai_judge::AiJudge::new(config.ai_judge);

    let session = {
        let s = state.engine.state.lock().await;
        s.sessions
            .iter()
            .find(|sess| sess.id == session_id)
            .cloned()
            .ok_or_else(|| state.i18n().t("err.session_not_found").to_string())?
    };

    let output = session.command.clone();
    judge.analyze(&session.agent_name, &output).await
}

/// 获取后端 i18n 词条（前端调试用；界面文案在前端自己的字典里）
#[tauri::command]
async fn get_translations(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<&'static str, &'static str>, String> {
    Ok(state.i18n().all())
}

// 引擎的事件推送入口：命令层也要能往同一条日志里写
impl MonitorEngine {
    pub async fn push_event_public(&self, event: EngineEvent) {
        let mut state = self.state.lock().await;
        tracing::info!("[AgentPulse] {}", event.message);
        state.events.push(event);
        if state.events.len() > 500 {
            let drain_count = state.events.len() - 500;
            state.events.drain(0..drain_count);
        }
    }
}

/// 构建托盘菜单
///
/// 单独抽出来是因为语言切换时要整体重建：菜单项文字一旦创建就不能改，
/// 只能换一整个 `Menu`。
fn build_tray_menu(app: &AppHandle, lang: &str) -> tauri::Result<Menu<Wry>> {
    let i18n = I18n::from_code(lang);
    let show = MenuItem::with_id(app, "show", i18n.t("tray.show"), true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start_monitor", i18n.t("tray.start"), true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_monitor", i18n.t("tray.stop"), true, None::<&str>)?;
    let scan = MenuItem::with_id(app, "scan", i18n.t("tray.scan"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", i18n.t("tray.quit"), true, None::<&str>)?;
    Menu::with_items(app, &[&show, &start, &stop, &scan, &quit])
}

/// 把主窗口拉到前台（托盘的两个入口共用）
fn reveal_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("agent_pulse=debug,info")
        .init();

    // 配置管理器进 Arc：引擎和命令层共享同一份，改完下一轮扫描即生效
    let config_manager = Arc::new(ConfigManager::new());
    let storage = Arc::new(storage::Storage::new());
    let engine = Arc::new(MonitorEngine::new(
        config_manager.clone(),
        storage.clone(),
    ));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_status,
            start_monitoring,
            stop_monitoring,
            scan_now,
            get_config,
            update_config,
            manual_resume,
            probe_resume,
            open_accessibility_settings,
            focus_terminal,
            test_notify,
            get_platform_info,
            get_stats,
            get_resume_history,
            get_totals,
            get_cost_daily,
            get_cost_projects,
            get_rate_forecast,
            get_session_history,
            test_webhook,
            ai_analyze,
            get_translations,
        ])
        .setup(move |app| {
            let config = config_manager.get();
            let lang = config.language.clone();
            let i18n = I18n::from_code(&lang);

            // Notifier 只能在这里造：它要 AppHandle 才能发通知、改托盘角标
            let notifier = Arc::new(Notifier::new(app.handle().clone()));
            engine.attach_notifier(notifier.clone());

            let remote = Arc::new(RemoteService::new(
                engine.clone(),
                config_manager.clone(),
            ));

            app.manage(AppState {
                engine: engine.clone(),
                config_manager: config_manager.clone(),
                storage: storage.clone(),
                notifier,
                remote: remote.clone(),
            });

            // ===== 手机看板 =====
            //
            // 默认关着。开了也只听 127.0.0.1，且必须带令牌；勾了「允许局域网
            // 访问」才会换成 0.0.0.0，那时日志里会明确警告一句。
            let remote_for_setup = remote.clone();
            tauri::async_runtime::spawn(async move {
                remote_for_setup.sync().await;
            });

            // ===== 系统托盘 =====
            let tray_menu = build_tray_menu(app.handle(), &lang)?;
            let engine_for_tray = engine.clone();
            let mut tray = TrayIconBuilder::with_id(notify::TRAY_ID)
                // 不设 .title()：托盘标题留给角标（等待中的会话数）用
                .tooltip(i18n.t("tray.tooltip"))
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app_handle, event| {
                    let engine = engine_for_tray.clone();
                    match event.id.as_ref() {
                        "show" => reveal_main_window(app_handle),
                        "start_monitor" => {
                            let app_clone = app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                engine.start().await;
                                let _ = app_clone.emit("engine-stopped", ());
                            });
                        }
                        "stop_monitor" => {
                            tauri::async_runtime::spawn(async move {
                                engine.stop().await;
                            });
                        }
                        "scan" => {
                            tauri::async_runtime::spawn(async move {
                                engine.scan_once().await;
                            });
                        }
                        "quit" => app_handle.exit(0),
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        reveal_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // ===== 事件推送泵（常驻） =====
            //
            // 以前这个泵挂在 `start_monitoring` 命令里，于是从托盘启动监控时
            // 前端一条日志都收不到。放在这里就跟「谁点的启动」无关了。
            let engine_for_events = engine.clone();
            let app_for_events = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut last_len = 0usize;
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                    let new_events = {
                        let state = engine_for_events.state.lock().await;
                        let total = state.events.len();
                        if total == last_len {
                            continue;
                        }
                        // 事件环被裁剪后 total 会小于 last_len，取 min 防止切片越界
                        let start = last_len.min(total);
                        last_len = total;
                        state.events[start..].to_vec()
                    };
                    if !new_events.is_empty() {
                        let _ = app_for_events.emit("engine-events", new_events);
                    }
                }
            });

            // ===== 关闭窗口 = 收进托盘，而不是退出 =====
            if let Some(window) = app.get_webview_window("main") {
                let hide_target = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = hide_target.hide();
                    }
                });
            }

            // ===== 启动即守护 =====
            if config.check_on_startup {
                let engine_for_startup = engine.clone();
                let app_for_startup = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // 等前端挂载完，否则最初几条日志会打在没人听的频道上
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    engine_for_startup.start().await;
                    let _ = app_for_startup.emit("engine-stopped", ());
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AgentPulse");
}
