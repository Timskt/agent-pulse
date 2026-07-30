pub mod adapters;
pub mod ai_judge;
pub mod config;
pub mod detector;
pub mod i18n;
pub mod monitor;
pub mod notify;
pub mod resumer;
pub mod storage;
pub mod webhook;

use config::{AppConfig, ConfigManager};
use monitor::{EngineEvent, EngineStatus, LogLevel, MonitorEngine, MonitorState};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::menu::{Menu, MenuItem};

/// 应用全局状态
pub struct AppState {
    pub engine: Arc<MonitorEngine>,
    pub config_manager: ConfigManager,
    pub storage: Arc<storage::Storage>,
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
#[tauri::command]
async fn start_monitoring(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<(), String> {
    let engine = state.engine.clone();

    // 检查是否已在运行
    {
        let s = engine.state.lock().await;
        if s.running {
            return Err("监控已在运行中".to_string());
        }
    }

    // 在后台 tokio 任务中启动监控循环
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        engine.start().await;
        let _ = app_clone.emit("engine-stopped", ());
    });

    // 启动事件推送定时器：每秒将新事件推送到前端
    let engine_for_events = state.engine.clone();
    let app_for_events = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_len = 0usize;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            let state = engine_for_events.state.lock().await;
            if !state.running && state.events.len() == last_len {
                break;
            }
            if state.events.len() != last_len {
                // 事件被截断时 last_len 可能越界，取 min 防止 panic
                let start = last_len.min(state.events.len());
                let new_events: Vec<EngineEvent> =
                    state.events[start..].to_vec();
                last_len = state.events.len();
                drop(state);
                if !new_events.is_empty() {
                    let _ = app_for_events.emit("engine-events", new_events);
                }
            }
        }
    });

    Ok(())
}

/// 停止监控
#[tauri::command]
async fn stop_monitoring(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.stop().await;
    state
        .engine
        .push_event_public(EngineEvent::new(
            LogLevel::Warn,
            None,
            "监控引擎已停止",
        ))
        .await;
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
#[tauri::command]
async fn update_config(state: State<'_, AppState>, config: AppConfig) -> Result<(), String> {
    state.config_manager.update(config)
}

/// 手动对指定会话触发续跑
#[tauri::command]
async fn manual_resume(state: State<'_, AppState>, session_id: String, use_goal_prompt: Option<bool>) -> Result<String, String> {
    let session = {
        let s = state.engine.state.lock().await;
        s.sessions
            .iter()
            .find(|sess| sess.id == session_id)
            .cloned()
            .ok_or_else(|| format!("未找到会话: {session_id}"))?
    };

    let config = state.config_manager.get();
    let resumer = resumer::Resumer::new(config);
    let result = resumer.resume(&session, use_goal_prompt.unwrap_or(false)).await?;

    state
        .engine
        .push_event_public(EngineEvent::new(
            LogLevel::Success,
            Some(session_id.clone()),
            format!("手动续跑: {result}"),
        ))
        .await;

    Ok(result)
}

/// 测试发送通知（验证通道）
#[tauri::command]
async fn test_notify() -> Result<String, String> {
    Ok("通知通道正常（本地测试）".to_string())
}

/// 获取当前平台信息
#[tauri::command]
async fn get_platform_info() -> Result<String, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    Ok(format!("{} ({})", os, arch))
}

/// 获取统计数据
#[tauri::command]
async fn get_stats(state: State<'_, AppState>, days: Option<u32>) -> Result<Vec<storage::DailyStats>, String> {
    Ok(state.storage.get_stats(days.unwrap_or(30)))
}

/// 获取最近续跑记录
#[tauri::command]
async fn get_resume_history(state: State<'_, AppState>, limit: Option<u32>) -> Result<Vec<storage::ResumeRecord>, String> {
    Ok(state.storage.get_recent_resumes(limit.unwrap_or(50)))
}

/// 获取总体统计
#[tauri::command]
async fn get_totals(state: State<'_, AppState>) -> Result<(u32, u32, u32), String> {
    Ok(state.storage.get_totals())
}

/// 测试 Webhook 连接
#[tauri::command]
async fn test_webhook(state: State<'_, AppState>) -> Result<String, String> {
    let config = state.config_manager.get();
    let notifier = webhook::WebhookNotifier::new(config.webhook);
    notifier.test().await
}

/// AI 分析指定会话
#[tauri::command]
async fn ai_analyze(state: State<'_, AppState>, session_id: String) -> Result<ai_judge::AiVerdict, String> {
    let config = state.config_manager.get();
    let judge = ai_judge::AiJudge::new(config.ai_judge);

    let session = {
        let s = state.engine.state.lock().await;
        s.sessions.iter().find(|sess| sess.id == session_id).cloned()
            .ok_or_else(|| format!("未找到会话: {session_id}"))?
    };

    // 获取最近输出
    let output = session.command.clone(); // 简化：使用 command 作为上下文
    judge.analyze(&session.agent_name, &output).await
}

/// 获取 i18n 翻译
#[tauri::command]
async fn get_translations(state: State<'_, AppState>) -> Result<std::collections::HashMap<&'static str, &'static str>, String> {
    let config = state.config_manager.get();
    let lang = i18n::Lang::from_code(&config.language);
    let i18n = i18n::I18n::new(lang);
    Ok(i18n.all())
}

// 为 MonitorEngine 添加公开的事件推送方法
impl MonitorEngine {
    pub async fn push_event_public(&self, event: EngineEvent) {
        let mut state = self.state.lock().await;
        state.events.push(event);
        if state.events.len() > 500 {
            let drain_count = state.events.len() - 500;
            state.events.drain(0..drain_count);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("agent_pulse=debug,info")
        .init();

    let config_manager = ConfigManager::new();
    let config = config_manager.get();
    let storage = Arc::new(storage::Storage::new());
    let engine = Arc::new(MonitorEngine::new(config.clone(), storage.clone()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            engine: engine.clone(),
            config_manager,
            storage,
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_status,
            start_monitoring,
            stop_monitoring,
            scan_now,
            get_config,
            update_config,
            manual_resume,
            test_notify,
            get_platform_info,
            get_stats,
            get_resume_history,
            get_totals,
            test_webhook,
            ai_analyze,
            get_translations,
        ])
        .setup(move |app| {
            // ===== 系统托盘 =====
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let start_item = MenuItem::with_id(app, "start_monitor", "开始监控", true, None::<&str>)?;
            let stop_item = MenuItem::with_id(app, "stop_monitor", "停止监控", true, None::<&str>)?;
            let scan_item = MenuItem::with_id(app, "scan", "立即扫描", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 AgentPulse", true, None::<&str>)?;

            let tray_menu = Menu::with_items(
                app,
                &[&show_item, &start_item, &stop_item, &scan_item, &quit_item],
            )?;

            let engine_for_tray = engine.clone();
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AgentPulse - AI Agent 守护")
                .title("AgentPulse")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app_handle, event| {
                    let engine = engine_for_tray.clone();
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
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
                        "quit" => {
                            app_handle.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    // 双击托盘图标显示/隐藏窗口
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app_handle = tray.app_handle();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // ===== 窗口关闭时最小化到托盘 =====
            let window = app.get_webview_window("main").unwrap();
            let window_clone = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    // 隐藏窗口而不是关闭
                    if let Some(w) = tauri::WebviewWindow::app_handle(&window_clone).get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            });

            // ===== 启动时自动监控 =====
            if config.check_on_startup {
                let engine_clone = engine.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    engine_clone.start().await;
                });
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AgentPulse");
}
