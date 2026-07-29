use crate::adapters::{self, AgentSession, SessionStatus};
use crate::config::AppConfig;
use crate::detector::{Detector, DetectionResult, Verdict};
use crate::resumer::Resumer;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

/// 日志级别
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Success,
}

/// 引擎事件（推送到前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineEvent {
    pub timestamp: String,
    pub level: LogLevel,
    pub session_id: Option<String>,
    pub message: String,
}

impl EngineEvent {
    pub fn new(level: LogLevel, session_id: Option<String>, message: impl Into<String>) -> Self {
        Self {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            level,
            session_id,
            message: message.into(),
        }
    }
}

/// 引擎运行状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStatus {
    pub running: bool,
    pub sessions_total: usize,
    pub sessions_active: usize,
    pub sessions_interrupted: usize,
    pub total_resumes: u32,
    pub total_detections: u32,
    pub last_scan_at: Option<String>,
    pub uptime_secs: u64,
}

/// 监控引擎共享状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorState {
    pub running: bool,
    pub sessions: Vec<AgentSession>,
    pub events: Vec<EngineEvent>,
    pub status: EngineStatus,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            running: false,
            sessions: Vec::new(),
            events: Vec::new(),
            status: EngineStatus {
                running: false,
                sessions_total: 0,
                sessions_active: 0,
                sessions_interrupted: 0,
                total_resumes: 0,
                total_detections: 0,
                last_scan_at: None,
                uptime_secs: 0,
            },
        }
    }
}

/// 监控引擎 — 核心调度器
pub struct MonitorEngine {
    pub state: Arc<Mutex<MonitorState>>,
    config: AppConfig,
    started_at: std::sync::Mutex<Option<std::time::Instant>>,
}

impl MonitorEngine {
    pub fn new(config: AppConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(MonitorState::default())),
            config,
            started_at: std::sync::Mutex::new(None),
        }
    }

    /// 添加事件日志
    async fn push_event(&self, event: EngineEvent) {
        let mut state = self.state.lock().await;
        tracing::info!("[AgentPulse] {:?}", event.message);
        state.events.push(event);
        // 保留最近 500 条日志
        if state.events.len() > 500 {
            let drain_count = state.events.len() - 500;
            state.events.drain(0..drain_count);
        }
    }

    /// 启动监控循环
    pub async fn start(&self) {
        *self.started_at.lock().unwrap() = Some(std::time::Instant::now());
        {
            let mut state = self.state.lock().await;
            state.running = true;
            state.status.running = true;
        }

        self.push_event(EngineEvent::new(
            LogLevel::Success,
            None,
            "监控引擎已启动，开始守护 AI Agent 会话",
        ))
        .await;

        let poll_secs = self.config.poll_interval_secs;
        let mut ticker = interval(Duration::from_secs(poll_secs));

        loop
        {
            ticker.tick().await;

            // 检查是否仍在运行
            {
                let state = self.state.lock().await;
                if !state.running {
                    break;
                }
            }

            self.scan_once().await;

            // 更新运行时间（先复制值，避免 guard 跨 await）
            let started_copy = *self.started_at.lock().unwrap();
            if let Some(started) = started_copy {
                let mut state = self.state.lock().await;
                state.status.uptime_secs = started.elapsed().as_secs();
            }
        }
    }

    /// 执行一次完整扫描
    pub async fn scan_once(&self) {
        let enabled = self.config.enabled_adapters.clone();
        let adapters = adapters::all_adapters();
        let mut all_sessions: Vec<AgentSession> = Vec::new();

        // 1. 通过适配器发现会话
        for adapter in &adapters {
            if !enabled.contains(&adapter.id().to_string()) {
                continue;
            }
            let discovered = adapter.discover_sessions();
            if !discovered.is_empty() {
                tracing::debug!(
                    "[AgentPulse] {} 发现 {} 个会话",
                    adapter.name(),
                    discovered.len()
                );
            }
            all_sessions.extend(discovered);
        }

        // 2. 合并已有状态（保留 resume_count 等）
        let existing: HashMap<String, AgentSession> = {
            let state = self.state.lock().await;
            state
                .sessions
                .iter()
                .map(|s| (s.id.clone(), s.clone()))
                .collect()
        };

        for session in &mut all_sessions {
            if let Some(old) = existing.get(&session.id) {
                session.resume_count = old.resume_count;
                session.last_resume_at = old.last_resume_at.clone();
                session.discovered_at = old.discovered_at.clone();
                session.status = old.status.clone();
            }
        }

        // 3. 对每个会话执行检测
        let detector = Detector::new(self.config.clone());
        let mut detections: Vec<(AgentSession, DetectionResult)> = Vec::new();

        for adapter in &adapters {
            for session in &all_sessions {
                if session.adapter_id != adapter.id() {
                    continue;
                }
                let output = adapter.recent_output(session);
                let result = detector.detect(session, output.as_deref());
                detections.push((session.clone(), result));
            }
        }

        // 4. 根据检测结果更新状态 & 触发续跑
        let mut resume_actions: Vec<(AgentSession, bool)> = Vec::new();

        for (session, detection) in &detections {
            match detection.verdict {
                Verdict::ConfirmInterrupt => {
                    self.push_event(EngineEvent::new(
                        LogLevel::Warn,
                        Some(session.id.clone()),
                        format!(
                            "[{}] 检测到中断信号: {}",
                            session.agent_name,
                            detection
                                .signals
                                .iter()
                                .map(|s| s.description.as_str())
                                .collect::<Vec<_>>()
                                .join("; ")
                        ),
                    ))
                    .await;

                    // 检查冷却时间
                    let can_resume = self.check_cooldown(session);
                    if can_resume && self.config.auto_resume_enabled {
                        resume_actions.push((session.clone(), detection.has_active_goal));
                    } else if !can_resume {
                        self.push_event(EngineEvent::new(
                            LogLevel::Info,
                            Some(session.id.clone()),
                            "续跑冷却中，跳过本次触发",
                        ))
                        .await;
                    }
                }
                Verdict::TaskCompleted => {
                    // 标记完成
                }
                Verdict::Suspicious => {
                    if self.config.heartbeat_log {
                        self.push_event(EngineEvent::new(
                            LogLevel::Info,
                            Some(session.id.clone()),
                            "疑似中断，继续观察...",
                        ))
                        .await;
                    }
                }
                Verdict::Running => {}
            }
        }

        // 5. 更新会话状态
        {
            let mut state = self.state.lock().await;
            let mut total_resumes = state.status.total_resumes;

            for session in &mut all_sessions {
                // 根据检测结果更新状态
                if let Some((_, detection)) = detections
                    .iter()
                    .find(|(s, _)| s.id == session.id)
                {
                    match detection.verdict {
                        Verdict::TaskCompleted => session.status = SessionStatus::Completed,
                        Verdict::ConfirmInterrupt => {
                            session.status = SessionStatus::Interrupted
                        }
                        Verdict::Suspicious => session.status = SessionStatus::Suspended,
                        Verdict::Running => {
                            if session.status != SessionStatus::Completed {
                                session.status = SessionStatus::Active;
                            }
                        }
                    }
                }

                // 更新续跑计数
                if resume_actions.iter().any(|(r, _)| r.id == session.id) {
                    session.resume_count += 1;
                    session.last_resume_at =
                        Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                    total_resumes += 1;
                }
            }

            let active = all_sessions
                .iter()
                .filter(|s| s.status == SessionStatus::Active)
                .count();
            let interrupted = all_sessions
                .iter()
                .filter(|s| s.status == SessionStatus::Interrupted)
                .count();

            state.status.sessions_total = all_sessions.len();
            state.status.sessions_active = active;
            state.status.sessions_interrupted = interrupted;
            state.status.total_resumes = total_resumes;
            state.status.total_detections += detections.len() as u32;
            state.status.last_scan_at =
                Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
            state.sessions = all_sessions;
        }

        // 6. 执行续跑动作（智能选择提示词）
        for (session, use_goal_prompt) in &resume_actions {
            let resumer = Resumer::new(self.config.clone());
            let prompt_type = if *use_goal_prompt { "Goal恢复" } else { "通用" };
            match resumer.resume(session, *use_goal_prompt).await {
                Ok(msg) => {
                    self.push_event(EngineEvent::new(
                        LogLevel::Success,
                        Some(session.id.clone()),
                        format!("已触发续跑[{}模式] (第{}次): {}", prompt_type, session.resume_count + 1, msg),
                    ))
                    .await;
                }
                Err(e) => {
                    self.push_event(EngineEvent::new(
                        LogLevel::Error,
                        Some(session.id.clone()),
                        format!("续跑失败: {e}"),
                    ))
                    .await;
                }
            }
        }

        if self.config.heartbeat_log {
            let state = self.state.lock().await;
            self.push_event(EngineEvent::new(
                LogLevel::Info,
                None,
                format!(
                    "心跳: 会话 {} 个, 活跃 {}, 中断 {}",
                    state.status.sessions_total,
                    state.status.sessions_active,
                    state.status.sessions_interrupted
                ),
            ))
            .await;
        }
    }

    /// 检查续跑冷却时间
    fn check_cooldown(&self, session: &AgentSession) -> bool {
        match &session.last_resume_at {
            Some(last) => {
                if let Ok(last_time) =
                    NaiveDateTime::parse_from_str(last, "%Y-%m-%d %H:%M:%S")
                {
                    let elapsed = Local::now().naive_local() - last_time;
                    elapsed.num_seconds() as u64 >= self.config.resume_cooldown_secs
                } else {
                    true
                }
            }
            None => true,
        }
    }

    /// 停止监控
    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        state.running = false;
        state.status.running = false;
    }
}

use chrono::NaiveDateTime;
