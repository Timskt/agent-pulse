pub mod adapters;
pub mod config;
pub mod detector;
pub mod monitor;
pub mod notify;
pub mod resumer;

use config::{AppConfig, ConfigManager};
use monitor::{EngineEvent, EngineStatus, LogLevel, MonitorEngine, MonitorState};
use std::sync::Arc;
use tauri::{Emitter, State};

/// 应用全局状态
pub struct AppState {
    pub engine: Arc<MonitorEngine>,
    pub config_manager: ConfigManager,
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
                let new_events: Vec<EngineEvent> =
                    state.events[last_len..].to_vec();
                last_len = state.events.len();
                drop(state);
                let _ = app_for_events.emit("engine-events", new_events);
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
    let engine = Arc::new(MonitorEngine::new(config.clone()));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            engine: engine.clone(),
            config_manager,
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
        ])
        .setup(move |_app| {
            // 如果配置了启动时检查，自动开始监控
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
