//! 监控引擎 —— 一轮扫描把「发现 → 判定 → 提醒 → 续跑 → 记账」串成一条流水线
//!
//! 三条纪律贯穿全文：
//!
//! 1. **重活全部下沉到阻塞线程**：进程枚举、读会话日志、SQLite 写入都在
//!    `spawn_blocking` 里做，异步侧只做状态合并、通知和续跑。磁盘再慢也不会
//!    把 tokio 运行时和界面一起拖住。
//! 2. **配置每轮重读**：引擎不再在构造时给 `AppConfig` 拍快照，
//!    用户改完设置下一轮就生效，不必重启监控。
//! 3. **锁不可重入**：`state` 是 `tokio::sync::Mutex`。任何要 `push_event` 的
//!    地方都得先把需要的值复制出来再放锁——否则就是自己锁死自己。
//!
//! 文案一律走 `i18n`：日志面板是用户界面的一部分，不该在代码里写死中文。

use crate::adapters::{self, AgentSession, SessionStatus};
use crate::config::{AppConfig, ConfigManager};
use crate::cost::{self, CostTracker, RateLimitForecast};
use crate::detector::{AttentionLevel, DetectionResult, Detector, Verdict};
use crate::i18n::{I18n, Lang};
use crate::notify::Notifier;
use crate::resumer::Resumer;
use crate::storage::Storage;
use crate::webhook::WebhookNotifier;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tokio::time::{interval, Duration, MissedTickBehavior};

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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineStatus {
    pub running: bool,
    pub sessions_total: usize,
    pub sessions_active: usize,
    pub sessions_interrupted: usize,
    /// 需要人介入的会话数（等待输入 / 限流 / 出错），也就是托盘角标上的数字
    pub pending_attention: usize,
    pub total_resumes: u32,
    pub total_detections: u32,
    pub last_scan_at: Option<String>,
    pub uptime_secs: u64,
    /// 今日累计花费（美元）
    pub cost_today: f64,
}

/// 监控引擎共享状态
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorState {
    pub running: bool,
    pub sessions: Vec<AgentSession>,
    pub events: Vec<EngineEvent>,
    pub status: EngineStatus,
}

/// 一轮扫描在阻塞线程里攒出来的全部结果
struct ScanOutcome {
    sessions: Vec<AgentSession>,
    detections: Vec<(AgentSession, DetectionResult)>,
    /// 本轮新落库的用量记录条数
    usage_added: usize,
    /// 今日累计花费（美元）
    cost_today: f64,
    /// 限流窗口预测；没配 token 预算时为 None
    forecast: Option<RateLimitForecast>,
}

/// PID → (TTY, 终端应用名)
type TerminalCache = HashMap<u32, (Option<String>, Option<String>)>;

/// 监控引擎 — 核心调度器
pub struct MonitorEngine {
    pub state: Arc<Mutex<MonitorState>>,
    /// 持有管理器而不是快照，配置才能热更新
    config_manager: Arc<ConfigManager>,
    started_at: std::sync::Mutex<Option<std::time::Instant>>,
    storage: Arc<Storage>,
    /// 感知层通道；要等 Tauri 把窗口和托盘建好才能装上
    notifier: OnceLock<Arc<Notifier>>,
    cost: Arc<CostTracker>,
    /// 查一次 TTY 要 spawn 十几个 `ps`，按 PID 缓存到进程消失为止
    terminal_cache: std::sync::Mutex<TerminalCache>,
}

impl MonitorEngine {
    pub fn new(config_manager: Arc<ConfigManager>, storage: Arc<Storage>) -> Self {
        // 游标从上次退出的位置接着走，重启不会把历史用量重算一遍
        let cursors = storage.load_usage_cursors();
        Self {
            state: Arc::new(Mutex::new(MonitorState::default())),
            config_manager,
            started_at: std::sync::Mutex::new(None),
            storage,
            notifier: OnceLock::new(),
            cost: Arc::new(CostTracker::new(cursors)),
            terminal_cache: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// 装上感知层通道（Tauri `setup` 里调用一次）
    pub fn attach_notifier(&self, notifier: Arc<Notifier>) {
        let _ = self.notifier.set(notifier);
    }

    /// 当前配置快照
    pub fn config(&self) -> AppConfig {
        self.config_manager.get()
    }

    /// 添加事件日志
    /// 往活动日志里追加一条
    ///
    /// 公开出来是给 `remote` 模块用的：看板启动/停止也该出现在同一条日志流里，
    /// 用户不用去别处找「看板到底起来了没有」。
    pub async fn push_event(&self, event: EngineEvent) {
        let mut state = self.state.lock().await;
        tracing::info!("[AgentPulse] {}", event.message);
        state.events.push(event);
        // 只留最近 500 条：日志面板是给人看的，不是审计追踪
        if state.events.len() > 500 {
            let drain_count = state.events.len() - 500;
            state.events.drain(0..drain_count);
        }
    }

    /// 启动监控循环
    pub async fn start(&self) {
        {
            let mut state = self.state.lock().await;
            // 托盘和界面各点一次「开始监控」不该起两条循环
            if state.running {
                return;
            }
            state.running = true;
            state.status.running = true;
        }
        *self.started_at.lock().unwrap() = Some(std::time::Instant::now());

        let lang = self.config().language;
        self.push_event(EngineEvent::new(
            LogLevel::Success,
            None,
            I18n::from_code(&lang).t("log.engine_started"),
        ))
        .await;

        let mut poll_secs = self.config().poll_interval_secs.max(1);
        let mut ticker = new_ticker(poll_secs);

        loop {
            ticker.tick().await;

            {
                let state = self.state.lock().await;
                if !state.running {
                    break;
                }
            }

            self.scan_once().await;

            // 轮询间隔改了就换个节拍器，不必重启监控
            let latest = self.config().poll_interval_secs.max(1);
            if latest != poll_secs {
                poll_secs = latest;
                ticker = new_ticker(poll_secs);
            }

            // 先把 Instant 复制出来，别让 std guard 跨 await
            let started_copy = *self.started_at.lock().unwrap();
            if let Some(started) = started_copy {
                let mut state = self.state.lock().await;
                state.status.uptime_secs = started.elapsed().as_secs();
            }
        }
    }

    /// 停止监控
    pub async fn stop(&self) {
        {
            let mut state = self.state.lock().await;
            if !state.running {
                return;
            }
            state.running = false;
            state.status.running = false;
        }

        let config = self.config();
        if let Some(notifier) = self.notifier.get() {
            // 停了就把角标清掉，别留个红点让人以为还有事没处理
            notifier.update_tray_badge(&config.notification, 0, &config.language);
        }
        self.push_event(EngineEvent::new(
            LogLevel::Info,
            None,
            I18n::from_code(&config.language).t("log.engine_stopped"),
        ))
        .await;
    }

    /// 执行一次完整扫描
    pub async fn scan_once(&self) {
        let config = self.config();
        let lang = config.language.clone();
        let i18n = I18n::from_code(&lang);

        self.storage.record_scan();

        let existing: HashMap<String, AgentSession> = {
            let state = self.state.lock().await;
            state
                .sessions
                .iter()
                .map(|s| (s.id.clone(), s.clone()))
                .collect()
        };

        let Some(outcome) = self.collect(config.clone(), existing).await else {
            return;
        };
        let ScanOutcome {
            mut sessions,
            detections,
            usage_added,
            cost_today,
            forecast,
        } = outcome;

        // ── 判定：确认中断要报警要续跑，刚完成的要知会，其余只观察 ──
        let webhook = if config.webhook.enabled {
            Some(WebhookNotifier::new(
                config.webhook.clone(),
                Lang::from_code(&config.language),
            ))
        } else {
            None
        };
        let mut resume_actions: Vec<(AgentSession, bool)> = Vec::new();

        for (session, detection) in &detections {
            match detection.verdict {
                Verdict::ConfirmInterrupt => {
                    let signals = detection
                        .signals
                        .iter()
                        .map(|s| s.description.as_str())
                        .collect::<Vec<_>>()
                        .join("; ");
                    self.storage.record_detection(
                        &session.id,
                        &session.agent_name,
                        "ConfirmInterrupt",
                        &signals,
                        detection.has_active_goal,
                    );
                    self.push_event(EngineEvent::new(
                        LogLevel::Warn,
                        Some(session.id.clone()),
                        i18n.tf(
                            "log.interrupt_detected",
                            &[("agent", &session.agent_name), ("signals", &signals)],
                        ),
                    ))
                    .await;
                    if let Some(hook) = &webhook {
                        hook.notify_interrupt(&session.agent_name, &session.id, &signals)
                            .await;
                    }

                    let can_resume = check_cooldown(session, config.resume_cooldown_secs);
                    if can_resume && config.auto_resume_enabled {
                        resume_actions.push((session.clone(), detection.has_active_goal));
                    } else if !can_resume {
                        self.push_event(EngineEvent::new(
                            LogLevel::Info,
                            Some(session.id.clone()),
                            i18n.t("log.cooldown_skip"),
                        ))
                        .await;
                    }
                }

                Verdict::TaskCompleted => {
                    // 只在「刚刚完成」那一轮知会：completed 会一直挂着，
                    // 每轮都发一次 webhook 等于自制垃圾消息
                    if session.status != SessionStatus::Completed {
                        if let Some(hook) = &webhook {
                            hook.notify_complete(&session.agent_name, &session.id).await;
                        }
                    }
                }
                Verdict::Suspicious => {
                    if config.heartbeat_log {
                        self.push_event(EngineEvent::new(
                            LogLevel::Info,
                            Some(session.id.clone()),
                            i18n.t("log.suspicious"),
                        ))
                        .await;
                    }
                }
                Verdict::Running => {}
            }
        }

        // ── 把判定结论合并回会话列表 ──
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut resumed_now = 0u32;
        for session in &mut sessions {
            if let Some((_, detection)) = detections.iter().find(|(s, _)| s.id == session.id) {
                session.attention = detection.attention;
                session.attention_detail = detection.attention_detail.clone();
                match detection.verdict {
                    Verdict::TaskCompleted => session.status = SessionStatus::Completed,
                    Verdict::ConfirmInterrupt => session.status = SessionStatus::Interrupted,
                    Verdict::Suspicious => session.status = SessionStatus::Suspended,
                    Verdict::Running => {
                        if session.status != SessionStatus::Completed {
                            session.status = SessionStatus::Active;
                        }
                    }
                }
            }
            if resume_actions.iter().any(|(r, _)| r.id == session.id) {
                session.resume_count += 1;
                session.last_resume_at = Some(now.clone());
                resumed_now += 1;
            }
        }

        // ── 感知层：该叫人的叫人，托盘角标同步 ──
        let pending = sessions.iter().filter(|s| s.attention.is_pending()).count();
        if let Some(notifier) = self.notifier.get() {
            for session in &sessions {
                if session.attention == AttentionLevel::None {
                    continue;
                }
                let label = session_label(session);
                let fired = notifier.notify_attention(
                    &config.notification,
                    &lang,
                    &session.id,
                    &label,
                    session.attention,
                    session.attention_detail.as_deref(),
                );
                if fired {
                    self.push_event(EngineEvent::new(
                        LogLevel::Warn,
                        Some(session.id.clone()),
                        i18n.tf(
                            "log.alerted",
                            &[
                                ("label", &label),
                                ("level", i18n.t(session.attention.i18n_key())),
                            ],
                        ),
                    ))
                    .await;
                }
            }
            notifier.update_tray_badge(&config.notification, pending as u32, &lang);
        }

        // ── 记账与限流预警 ──
        if config.cost.enabled {
            if usage_added > 0 {
                self.push_event(EngineEvent::new(
                    LogLevel::Info,
                    None,
                    i18n.tf(
                        "log.usage_added",
                        &[
                            ("count", &usage_added.to_string()),
                            ("cost", &format!("{cost_today:.2}")),
                        ],
                    ),
                ))
                .await;
            }
            self.check_cost_alerts(&config, &i18n, cost_today, &sessions, forecast)
                .await;
        }

        // ── 写回状态 ──
        let total = sessions.len();
        let active = sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Active)
            .count();
        let interrupted = sessions
            .iter()
            .filter(|s| s.status == SessionStatus::Interrupted)
            .count();
        // 只把「确认中断」计入检测数：否则每轮扫描都会给每个健康会话记一笔，
        // 界面上的数字就永远对不上 detection_records 里的行数
        let confirmed = detections
            .iter()
            .filter(|(_, d)| matches!(d.verdict, Verdict::ConfirmInterrupt))
            .count() as u32;
        {
            let mut state = self.state.lock().await;
            state.status.sessions_total = total;
            state.status.sessions_active = active;
            state.status.sessions_interrupted = interrupted;
            state.status.pending_attention = pending;
            state.status.total_resumes += resumed_now;
            state.status.total_detections += confirmed;
            state.status.last_scan_at = Some(now);
            state.status.cost_today = cost_today;
            state.sessions = sessions;
        }

        // ── 执行续跑：AppleScript 一次可能要几秒，放最后，别拖着状态更新 ──
        self.run_resumes(&config, &i18n, &webhook, &resume_actions)
            .await;

        if config.heartbeat_log {
            self.push_event(EngineEvent::new(
                LogLevel::Info,
                None,
                i18n.tf(
                    "log.heartbeat",
                    &[
                        ("total", &total.to_string()),
                        ("active", &active.to_string()),
                        ("interrupted", &interrupted.to_string()),
                    ],
                ),
            ))
            .await;
        }
    }

    /// 一轮扫描的重活：进程枚举、输出读取、成本解析、历史落库
    ///
    /// 全部塞进一个 `spawn_blocking`，异步侧只等一次结果——
    /// 分成多次 `spawn_blocking` 只会让每轮扫描多几次线程调度往返。
    async fn collect(
        &self,
        config: AppConfig,
        existing: HashMap<String, AgentSession>,
    ) -> Option<ScanOutcome> {
        let storage = self.storage.clone();
        let cost = self.cost.clone();
        // 缓存按值进出：小 map 拷贝一次比把锁塞进闭包干净
        let cache_in = self.terminal_cache.lock().unwrap().clone();

        let joined = tokio::task::spawn_blocking(move || {
            let mut cache = cache_in;
            let snapshot = adapters::take_process_snapshot();
            let live_pids: HashSet<u32> = snapshot.iter().map(|p| p.pid).collect();
            // 进程没了缓存也就没用了，顺手清掉，别让它随着开机时长一直长
            cache.retain(|pid, _| live_pids.contains(pid));

            // 1. 成本增量刷新：只读 Claude Code 已经写在磁盘上的日志
            let mut usage_added = 0usize;
            if config.cost.enabled && cost.should_refresh() {
                let (entries, cursors) = cost.refresh(&config.cost.price_overrides);
                if !entries.is_empty() {
                    usage_added = storage.record_usage_batch(&entries);
                }
                if !cursors.is_empty() {
                    storage.save_usage_cursors(&cursors);
                }
            }

            // 2. 通过适配器发现会话
            let adapter_list = adapters::all_adapters();
            let mut sessions: Vec<AgentSession> = Vec::new();
            for adapter in &adapter_list {
                if !config.enabled_adapters.iter().any(|id| id == adapter.id()) {
                    continue;
                }
                let discovered = adapter.discover_sessions(&snapshot);
                if !discovered.is_empty() {
                    tracing::debug!(
                        "[AgentPulse] {} 发现 {} 个会话",
                        adapter.name(),
                        discovered.len()
                    );
                }
                sessions.extend(discovered);
            }

            // 3. 合并上一轮状态，保住续跑计数与首次发现时间
            for session in &mut sessions {
                if let Some(old) = existing.get(&session.id) {
                    session.resume_count = old.resume_count;
                    session.last_resume_at = old.last_resume_at.clone();
                    session.discovered_at = old.discovered_at.clone();
                    session.status = old.status.clone();
                }
            }

            // 4. 回填终端定位信息与用量（「跳到终端」和成本卡片都靠这一步）
            for session in &mut sessions {
                let entry = cache.entry(session.pid).or_insert_with(|| {
                    (
                        crate::resumer::session_tty(session.pid),
                        crate::resumer::session_terminal_app(session.pid),
                    )
                });
                session.tty = entry.0.clone();
                session.terminal_app = entry.1.clone();

                if config.cost.enabled {
                    if let Some(file) = session.session_file.clone() {
                        session.usage = storage.usage_for_session_file(&file);
                    }
                }
            }

            // 5. 逐会话检测（进程存活直接从快照判定，不再额外问系统）
            let detector = Detector::new(config.clone());
            let mut detections: Vec<(AgentSession, DetectionResult)> = Vec::new();
            for adapter in &adapter_list {
                for session in &sessions {
                    if session.adapter_id != adapter.id() {
                        continue;
                    }
                    let alive = live_pids.contains(&session.pid);
                    let output = adapter.recent_output(session);
                    // 故障单独走一条通道：散文里提到「500」不算出错，
                    // 只有记录自己标成故障的行才算
                    let errors = adapter.error_output(session);
                    // 回合结构：区分「正在跑工具/压缩上下文」和「真的停下来等人」，
                    // 光看文件 mtime 这两者长得一样
                    let turn = adapter.turn_state(session);
                    let result = detector.detect(
                        session,
                        output.as_deref(),
                        errors.as_deref(),
                        alive,
                        turn,
                    );
                    detections.push((session.clone(), result));
                }
            }

            // 6. 会话历史时间线：以会话文件为主键，PID 换了也能接上同一条线
            for session in &sessions {
                let usage = session.usage.clone().unwrap_or_default();
                let key = session.session_file.clone().unwrap_or_else(|| {
                    format!(
                        "{}-{}-{}",
                        session.adapter_id, session.pid, session.discovered_at
                    )
                });
                storage.upsert_session_history(
                    &key,
                    &session.id,
                    &session.agent_name,
                    &session.working_dir,
                    session.session_file.as_deref().unwrap_or(""),
                    session.tty.as_deref().unwrap_or(""),
                    session.terminal_app.as_deref().unwrap_or(""),
                    session.status.key(),
                    session.resume_count,
                    usage.total_tokens,
                    usage.cost_usd,
                );
            }

            // 7. 今日花费与限流预测（都是 SQL 聚合，一并在这里算完）
            let (cost_today, forecast) = if config.cost.enabled {
                let today = Local::now().format("%Y-%m-%d").to_string();
                let spent = storage.cost_for_date(&today);
                let forecast = if config.cost.rate_limit_token_budget > 0 {
                    let window = config.cost.rate_limit_window_hours.max(1);
                    Some(cost::forecast_rate_limit(
                        window,
                        config.cost.rate_limit_token_budget,
                        storage.tokens_in_last_hours(window),
                        storage.tokens_in_last_hours(1),
                    ))
                } else {
                    None
                };
                (spent, forecast)
            } else {
                (0.0, None)
            };

            let outcome = ScanOutcome {
                sessions,
                detections,
                usage_added,
                cost_today,
                forecast,
            };
            (outcome, cache)
        })
        .await;

        match joined {
            Ok((outcome, cache)) => {
                *self.terminal_cache.lock().unwrap() = cache;
                Some(outcome)
            }
            Err(e) => {
                tracing::error!("[AgentPulse] 扫描任务失败: {e}");
                None
            }
        }
    }

    /// 逐个执行续跑动作
    async fn run_resumes(
        &self,
        config: &AppConfig,
        i18n: &I18n,
        webhook: &Option<WebhookNotifier>,
        actions: &[(AgentSession, bool)],
    ) {
        if actions.is_empty() {
            return;
        }
        let resumer = Resumer::new(config.clone());

        for (session, use_goal_prompt) in actions {
            let prompt_type = if *use_goal_prompt { "goal" } else { "generic" };
            let mode = i18n.t(if *use_goal_prompt {
                "log.mode_goal"
            } else {
                "log.mode_generic"
            });

            match resumer.resume(session, *use_goal_prompt).await {
                Ok(msg) => {
                    self.storage.record_resume(
                        &session.id,
                        &session.agent_name,
                        &session.working_dir,
                        prompt_type,
                        true,
                        &msg,
                    );
                    self.push_event(EngineEvent::new(
                        LogLevel::Success,
                        Some(session.id.clone()),
                        i18n.tf(
                            "log.resume_sent",
                            &[
                                ("mode", mode),
                                ("count", &(session.resume_count + 1).to_string()),
                                ("detail", &msg),
                            ],
                        ),
                    ))
                    .await;

                    if let Some(hook) = webhook {
                        hook.notify_resume(&session.agent_name, &session.id, &msg)
                            .await;
                    }
                    if let Some(notifier) = self.notifier.get() {
                        notifier.notify_resumed(
                            &config.notification,
                            &config.language,
                            &session.id,
                            &msg,
                        );
                    }
                }
                Err(e) => {
                    self.storage.record_resume(
                        &session.id,
                        &session.agent_name,
                        &session.working_dir,
                        prompt_type,
                        false,
                        &e,
                    );
                    self.push_event(EngineEvent::new(
                        LogLevel::Error,
                        Some(session.id.clone()),
                        i18n.tf("log.resume_failed", &[("detail", &e)]),
                    ))
                    .await;
                }
            }
        }
    }

    /// 成本与限流告警
    ///
    /// 阈值判定放在这里而不是 `Notifier` 里：通知层只负责「怎么送达」，
    /// 「什么时候该喊」是业务判断。节流由 `notify_alert` 兜底。
    async fn check_cost_alerts(
        &self,
        config: &AppConfig,
        i18n: &I18n,
        cost_today: f64,
        sessions: &[AgentSession],
        forecast: Option<RateLimitForecast>,
    ) {
        let Some(notifier) = self.notifier.get() else {
            return;
        };
        let cfg = &config.notification;
        let threshold = config.cost.alert_at_percent.clamp(1, 100) as f64;

        if config.cost.daily_budget_usd > 0.0 {
            let ratio = cost_today / config.cost.daily_budget_usd * 100.0;
            if ratio >= threshold {
                let body = i18n.tf(
                    "notify.budget.daily_body",
                    &[
                        ("spent", &format!("{cost_today:.2}")),
                        ("budget", &format!("{:.2}", config.cost.daily_budget_usd)),
                        ("percent", &format!("{}", ratio.round() as u64)),
                    ],
                );
                notifier.notify_alert(cfg, "budget:daily", i18n.t("notify.budget.title"), &body);
                self.push_event(EngineEvent::new(LogLevel::Warn, None, body)).await;
            }
        }

        if config.cost.session_budget_usd > 0.0 {
            for session in sessions {
                let Some(usage) = &session.usage else { continue };
                if usage.cost_usd < config.cost.session_budget_usd {
                    continue;
                }
                let label = session_label(session);
                let body = i18n.tf(
                    "notify.budget.session_body",
                    &[
                        ("label", &label),
                        ("spent", &format!("{:.2}", usage.cost_usd)),
                        ("budget", &format!("{:.2}", config.cost.session_budget_usd)),
                    ],
                );
                let key = format!("budget:session:{}", session.id);
                notifier.notify_alert(cfg, &key, i18n.t("notify.budget.title"), &body);
            }
        }

        if let Some(f) = forecast {
            if f.used_percent as f64 >= threshold {
                let minutes = f
                    .minutes_to_limit
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "—".to_string());
                let body = i18n.tf(
                    "notify.rate_forecast.body",
                    &[
                        ("window", &f.window_hours.to_string()),
                        ("percent", &f.used_percent.to_string()),
                        ("minutes", &minutes),
                    ],
                );
                notifier.notify_alert(
                    cfg,
                    "rate:forecast",
                    i18n.t("notify.rate_forecast.title"),
                    &body,
                );
                self.push_event(EngineEvent::new(LogLevel::Warn, None, body)).await;
            }
        }
    }
}

/// 扫描节拍器
///
/// `Delay` 而不是默认的 `Burst`：某轮扫描偶尔超过轮询间隔时，
/// 不要把欠下的 tick 攒起来连着补，否则慢磁盘上会出现连环扫描。
fn new_ticker(secs: u64) -> tokio::time::Interval {
    let mut ticker = interval(Duration::from_secs(secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker
}

/// 通知里用的会话标识
///
/// 痛点是「5 个标签页我不知道是哪个在等我」，所以标签要带上目录和 TTY：
/// `Claude Code · agent-pulse (ttys003)`。
fn session_label(session: &AgentSession) -> String {
    let dir = session
        .working_dir
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    let mut label = if dir.is_empty() {
        session.agent_name.clone()
    } else {
        format!("{} · {dir}", session.agent_name)
    };
    if let Some(tty) = &session.tty {
        let short = tty.rsplit('/').next().unwrap_or(tty);
        label.push_str(&format!(" ({short})"));
    }
    label
}

/// 续跑冷却判定：距上次续跑不足冷却时间就先忍着
///
/// 时间戳解析失败时返回 `true`（宁可多试一次，也别因为一条脏数据永久沉默）。
fn check_cooldown(session: &AgentSession, cooldown_secs: u64) -> bool {
    match &session.last_resume_at {
        Some(last) => match NaiveDateTime::parse_from_str(last, "%Y-%m-%d %H:%M:%S") {
            Ok(last_time) => {
                let elapsed = Local::now().naive_local() - last_time;
                elapsed.num_seconds().max(0) as u64 >= cooldown_secs
            }
            Err(_) => true,
        },
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with(dir: &str, tty: Option<&str>) -> AgentSession {
        AgentSession {
            agent_name: "Claude Code".to_string(),
            working_dir: dir.to_string(),
            tty: tty.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn label_shows_directory_and_tty() {
        let s = session_with("/Users/sky/code/git/agent-pulse", Some("/dev/ttys003"));
        assert_eq!(session_label(&s), "Claude Code · agent-pulse (ttys003)");
    }

    #[test]
    fn label_falls_back_to_agent_name() {
        let s = session_with("", None);
        assert_eq!(session_label(&s), "Claude Code");
    }

    #[test]
    fn cooldown_allows_first_resume() {
        assert!(check_cooldown(&session_with("/tmp", None), 300));
    }

    #[test]
    fn cooldown_blocks_recent_resume() {
        let mut s = session_with("/tmp", None);
        s.last_resume_at = Some(Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
        assert!(!check_cooldown(&s, 300));
    }

    #[test]
    fn cooldown_ignores_unparsable_timestamp() {
        let mut s = session_with("/tmp", None);
        s.last_resume_at = Some("not a timestamp".to_string());
        assert!(check_cooldown(&s, 300));
    }
}
