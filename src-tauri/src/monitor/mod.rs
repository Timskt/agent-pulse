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
use crate::ai_judge::AiJudge;
use crate::config::{AppConfig, ConfigManager};
use crate::cost::{self, CostTracker, RateLimitForecast};
use crate::detector::{
    Arbitration, AttentionLevel, DetectionResult, Detector, ResumeTactic, Verdict,
};
use crate::i18n::{I18n, Lang};
use crate::notify::Notifier;
use crate::resumer::{ResumeOutcome, Resumer};
use crate::storage::Storage;
use crate::webhook::WebhookNotifier;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tokio::time::{interval, Duration, MissedTickBehavior};

/// 连续失败几次就该改成大声说
///
/// 1 次可能只是恰好前台窗口被别人抢了；连着 2 次基本就是通道本身坏了。
/// 门槛压得低是故意的：这个功能的失败模式是**静默**，而静默的失败
/// 跟「正常工作」在屏幕上长得一模一样，晚说一小时就是白等一小时。
const RESUME_FAILURE_ALERT_AT: u32 = 2;

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
    /// 判定层明确说「再问一句可能改变结果」的候选；异步侧每轮最多问一个
    to_arbitrate: Vec<ArbitrationRequest>,
    /// 本轮新落库的用量记录条数
    usage_added: usize,
    /// 今日累计花费（美元）
    cost_today: f64,
    /// 限流窗口预测；没配 token 预算时为 None
    forecast: Option<RateLimitForecast>,
}

/// 一次待问的第二意见。指纹把答案绑定到这一版记录，记录一变就重新判断。
struct ArbitrationRequest {
    session_id: String,
    agent_name: String,
    recent_output: String,
    fingerprint: u64,
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
    /// 「这句话上次说的是什么情况」：话题键 → 情况指纹
    ///
    /// 见 [`MonitorEngine::push_event_on_change`]。
    said: std::sync::Mutex<HashMap<String, String>>,
    /// 会话 →（记录指纹，第二意见）。指纹变化即失效，避免旧答案跨回合生效。
    arbitrations: std::sync::Mutex<HashMap<String, (u64, Arbitration)>>,
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
            said: std::sync::Mutex::new(HashMap::new()),
            arbitrations: std::sync::Mutex::new(HashMap::new()),
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

    /// 只在「情况变了」的那一轮说话
    ///
    /// 事件和状态是两种东西，日志却只有一条流。「检测到中断」是**事件**，
    /// 发生一次说一次；「已经催不动了」是**状态**，它会一直成立——每 10 秒
    /// 重播一遍的结果是日志面板被自己的历史刷满，真正的新消息被挤下屏幕。
    /// 一个反复自我复述的日志，等于没有日志。
    ///
    /// 所以状态类的话走这里：`topic` 说的是「这条在讲哪件事」，
    /// `fingerprint` 说的是「那件事现在什么样」。指纹没变就闭嘴；
    /// 变了（比如连击数从 5 涨到 8）才再说一次。情况解除时调用方要
    /// [`Self::forget_topic`] 把记忆清掉，否则同一个情况第二次发生就说不出口了。
    async fn push_event_on_change(&self, topic: String, fingerprint: &str, event: EngineEvent) {
        let worth_saying = {
            let mut said = self.said.lock().unwrap();
            should_say(&mut said, topic, fingerprint)
        };
        if worth_saying {
            self.push_event(event).await;
        }
    }

    /// 忘掉某个话题上次说过什么，让它下次能重新开口
    fn forget_topic(&self, topic: &str) {
        self.said.lock().unwrap().remove(topic);
    }

    /// 「催不动了」这条话题的键
    fn exhausted_topic(session_id: &str) -> String {
        format!("nudges_exhausted:{session_id}")
    }

    /// 「这次故意不敲字」这条话题的键
    ///
    /// 跟 [`Self::exhausted_topic`] 分开：一个会话可以先撞限流（等），
    /// 限流过去之后变成额度用光（催不动了）。两件事共用一个话题的话，
    /// 后一件会被前一件的指纹压住，说不出口。
    fn tactic_topic(session_id: &str) -> String {
        format!("resume_tactic:{session_id}")
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

        // 先体检投递通道，再开始扫描：权限失效这件事该在用户还没等它干活时就说
        let config = self.config();
        self.check_resume_channel(&config, &I18n::from_code(&lang))
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
            to_arbitrate,
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
        // 本轮**新**确认中断的会话数
        //
        // 一个会话可以连着几十轮都是「确认中断」（用户关了自动续跑，或者已经
        // 催不动了）。那是一个持续的状态，不是几十次检测——每轮都记一笔，
        // 界面上的检测数就会跟 `detection_records` 里的行数越差越远。
        let mut newly_confirmed: u32 = 0;

        for (session, detection) in &detections {
            match detection.verdict {
                Verdict::ConfirmInterrupt => {
                    let signals = detection
                        .signals
                        .iter()
                        .map(|s| s.description.as_str())
                        .collect::<Vec<_>>()
                        .join("; ");

                    // `session.status` 这会儿还是上一轮的结论（本轮的合并回写在后面），
                    // 所以这一句问的是「它是刚刚才中断的吗」。中断是**事件**，
                    // 发生一次报一次：落库、日志、webhook 三处都跟着这一个条件走。
                    // 否则一个开着关闭自动续跑的会话，会每 10 秒给你发一条 webhook。
                    if session.status != SessionStatus::Interrupted {
                        newly_confirmed += 1;
                        self.storage.record_detection(
                            &session.id,
                            &session.agent_name,
                            "ConfirmInterrupt",
                            &signals,
                            detection.has_active_goal,
                            detection.interrupt_reason.key(),
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
                    }

                    // ── 先问「该用什么手段」，再问「现在能不能动手」──
                    //
                    // 这两个问题不是一个问题，上一版把它们合成了一个：只要判定是
                    // 确认中断，手段就固定是「往终端里敲一句继续」，三道闸门只管
                    // 拦时机。于是三类停顿被同一个动作误伤——
                    // 进程已经死了（字落进它身后的 shell，还被记成一次成功续跑）、
                    // 撞上限流（敲了也不会提前恢复，白烧一次额度）、
                    // 它在问一个 `(y/n)`（敲回车等于替用户批准了一件他没看过的事）。
                    //
                    // 手段来自判定层给出的原因（[`InterruptReason::tactic`]），
                    // 不在这儿重新猜：这一层只有会话状态，没有本轮的证据。
                    let tactic = detection.interrupt_reason.tactic();
                    if tactic != ResumeTactic::Nudge {
                        // 不敲字要说出来。上一版只有「动手」和「冷却中」两种说法，
                        // 于是「这次故意不动手」只能靠沉默表达，用户在日志里看不到
                        // 任何解释，只会觉得守护神漏了一次。
                        //
                        // 走 `on_change`：原因是**状态**，会一直成立到情况变化，
                        // 每 10 秒重播一遍等于自己刷屏。指纹用原因键——原因变了
                        // （限流过去了却变成了报错）要能重新开口。
                        let key = if tactic == ResumeTactic::Wait {
                            "log.resume_wait"
                        } else {
                            "log.resume_hand_off"
                        };
                        self.push_event_on_change(
                            Self::tactic_topic(&session.id),
                            detection.interrupt_reason.key(),
                            EngineEvent::new(
                                LogLevel::Warn,
                                Some(session.id.clone()),
                                i18n.tf(
                                    key,
                                    &[
                                        ("agent", &session.agent_name),
                                        ("reason", i18n.t(detection.interrupt_reason.i18n_key())),
                                    ],
                                ),
                            ),
                        )
                        .await;
                        continue;
                    }

                    // ── 该不该动手，在这里决定 ──
                    //
                    // 三道闸门各管一件事，故意分开写，也故意分开报：
                    // 冷却管「太频繁」、总开关管「用户不让」、额度管「催也没用」。
                    // 上一版把额度写在判定层，于是额度用光时会话状态被顺手降级，
                    // 提醒也跟着消失——应用不打算动手的那一刻，正是最该叫人的一刻。
                    let cooldown =
                        effective_cooldown(config.resume_cooldown_secs, session.resume_failures);
                    let cooled = check_cooldown(session, cooldown);
                    let has_budget = has_nudges_left(session, config.max_resume_count);
                    if cooled && has_budget && config.auto_resume_enabled {
                        resume_actions.push((session.clone(), detection.has_active_goal));
                    } else if !has_budget {
                        // 说清楚是「催不动了」而不是「还在冷却」：两者的下一步动作
                        // 完全不同——一个等几十秒就好，一个得人去看一眼。
                        // 判定仍然是 ConfirmInterrupt，所以注意力分级照样会叫人。
                        self.push_event_on_change(
                            Self::exhausted_topic(&session.id),
                            &session.resume_streak.to_string(),
                            EngineEvent::new(
                                LogLevel::Warn,
                                Some(session.id.clone()),
                                i18n.tf(
                                    "log.nudges_exhausted",
                                    &[
                                        ("agent", &session.agent_name),
                                        ("count", &session.resume_streak.to_string()),
                                    ],
                                ),
                            ),
                        )
                        .await;
                    } else if !cooled {
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
        //
        // **这里不碰 `resume_count` / `last_resume_at`**：字还没敲出去呢。
        // 计数一律等 `run_resumes` 拿到真实结果之后由 `commit_resume_outcome` 落笔。
        // 旧代码在这儿就把计数加了、失败也不回退，等于把「敲不进去」记成
        // 「已经敲够了」，五次之后自动续跑对这个会话永久沉默——详见
        // `ResumeOutcome::counts_as_nudge` 上的说明。
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        for session in &mut sessions {
            if let Some((_, detection)) = detections.iter().find(|(s, _)| s.id == session.id) {
                session.attention = detection.attention;
                session.attention_detail = detection.attention_detail.clone();
                session.detection_evidence = Some(detection.evidence.clone());
                session.interrupt_reason = detection.interrupt_reason;
                session.resume_tactic = detection.interrupt_reason.tactic();
                match detection.verdict {
                    Verdict::TaskCompleted => session.status = SessionStatus::Completed,
                    Verdict::ConfirmInterrupt => session.status = SessionStatus::Interrupted,
                    Verdict::Suspicious => session.status = SessionStatus::Suspended,
                    Verdict::Running => {
                        if session.status != SessionStatus::Completed {
                            session.status = SessionStatus::Active;
                        }
                        // **看见它自己在干活，就把续跑额度还回去。**
                        // 这是闭环的另一半：`resume_streak` 只该数「催了却没反应」的连击，
                        // 一旦会话恢复推进，之前那几次催就都算奏效了，不该继续压着额度。
                        // 累计次数（`resume_count`）不动——那是给人看的历史。
                        session.resume_streak = 0;
                        // 额度回来了，「催不动了」这句话下次卡住时要能重新说出口
                        self.forget_topic(&Self::exhausted_topic(&session.id));
                        // 「这次不敲字」同理。它的指纹是原因键，而同一个原因
                        // 完全可能隔一小时再来一次（限流窗口就是这样）：
                        // 中间恢复过就得让它重新开口，否则第二次撞限流时
                        // 日志里一片安静，看着像守护神睡着了。
                        self.forget_topic(&Self::tactic_topic(&session.id));
                    }
                }
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
        // 检测数只数「新确认」的那几次，理由见 `newly_confirmed` 的定义处
        let confirmed = newly_confirmed;
        {
            let mut state = self.state.lock().await;
            state.status.sessions_total = total;
            state.status.sessions_active = active;
            state.status.sessions_interrupted = interrupted;
            state.status.pending_attention = pending;
            state.status.total_detections += confirmed;
            state.status.last_scan_at = Some(now);
            state.status.cost_today = cost_today;
            state.sessions = sessions;
        }

        // 仲裁只加速「弱证据」的结论，不阻塞本轮状态合并。答案缓存到下一轮使用；
        // 每轮最多问一个，避免多个会话同时可疑时突然并发烧额度。
        self.ask_one_arbitration(&config, &i18n, to_arbitrate).await;

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
        let arbitrations_in = self.arbitrations.lock().unwrap().clone();

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
                    session.resume_streak = old.resume_streak;
                    session.resume_failures = old.resume_failures;
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
            let mut to_arbitrate = Vec::new();
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
                    let fingerprint = output.as_deref().map(transcript_fingerprint);
                    let second_opinion = fingerprint.and_then(|fingerprint| {
                        arbitrations_in
                            .get(&session.id)
                            .and_then(|(seen, answer)| (*seen == fingerprint).then_some(*answer))
                    });
                    let result = detector.detect(
                        session,
                        output.as_deref(),
                        errors.as_deref(),
                        alive,
                        turn,
                        second_opinion,
                    );
                    if result.wants_second_opinion {
                        if let (Some(recent_output), Some(fingerprint)) = (output, fingerprint) {
                            to_arbitrate.push(ArbitrationRequest {
                                session_id: session.id.clone(),
                                agent_name: session.agent_name.clone(),
                                recent_output,
                                fingerprint,
                            });
                        }
                    }
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
                to_arbitrate,
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

    /// 每轮最多问一个第二意见；失败只写日志，不改变已有判定。
    async fn ask_one_arbitration(
        &self,
        config: &AppConfig,
        i18n: &I18n,
        requests: Vec<ArbitrationRequest>,
    ) {
        if !config.ai_judge.enabled {
            return;
        }
        let Some(request) = requests.into_iter().next() else {
            return;
        };

        let judge = AiJudge::new(config.ai_judge.clone());
        match judge
            .arbitrate(&request.agent_name, &request.recent_output)
            .await
        {
            Ok(answer) => {
                self.arbitrations
                    .lock()
                    .unwrap()
                    .insert(request.session_id.clone(), (request.fingerprint, answer));
                let verdict = i18n.t(match answer {
                    Arbitration::Finished => "arbitration.finished",
                    Arbitration::Unfinished => "arbitration.unfinished",
                });
                self.push_event(EngineEvent::new(
                    LogLevel::Info,
                    Some(request.session_id),
                    i18n.tf(
                        "log.arbitration_answered",
                        &[("agent", &request.agent_name), ("verdict", verdict)],
                    ),
                ))
                .await;
            }
            Err(error) => {
                self.push_event(EngineEvent::new(
                    LogLevel::Warn,
                    Some(request.session_id),
                    i18n.tf(
                        "log.arbitration_failed",
                        &[("agent", &request.agent_name), ("detail", &error)],
                    ),
                ))
                .await;
            }
        }
    }

    /// 逐个执行续跑动作
    ///
    /// **必须串行**：剪贴板是全局单件，前台窗口也只有一个。两个续跑并发跑
    /// AppleScript，就会互相抢剪贴板和焦点，两边都可能敲到对方的窗口里去。
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

            // 投递 + 核验：会话记录有没有长出新内容，才是「敲进去了」的证据
            let (outcome, detail) = resumer.resume_verified(session, *use_goal_prompt).await;
            let landed = outcome.counts_as_nudge();
            let failures = self.commit_resume_outcome(&session.id, outcome).await;

            self.storage.record_resume(
                &session.id,
                &session.agent_name,
                &session.working_dir,
                prompt_type,
                landed,
                &detail,
            );

            if landed {
                self.push_event(EngineEvent::new(
                    LogLevel::Success,
                    Some(session.id.clone()),
                    i18n.tf(
                        "log.resume_sent",
                        &[
                            ("mode", mode),
                            ("count", &(session.resume_count + 1).to_string()),
                            (
                                "detail",
                                &format!("{} · {}", i18n.t(outcome.i18n_key()), detail),
                            ),
                        ],
                    ),
                ))
                .await;

                if let Some(hook) = webhook {
                    hook.notify_resume(&session.agent_name, &session.id, &detail)
                        .await;
                }
                if let Some(notifier) = self.notifier.get() {
                    notifier.notify_resumed(
                        &config.notification,
                        &config.language,
                        &session.id,
                        &detail,
                    );
                }
            } else {
                self.push_event(EngineEvent::new(
                    LogLevel::Error,
                    Some(session.id.clone()),
                    i18n.tf(
                        "log.resume_failed",
                        &[(
                            "detail",
                            &format!("{} · {}", i18n.t(outcome.i18n_key()), detail),
                        )],
                    ),
                ))
                .await;
                self.escalate_resume_failure(config, i18n, session, failures, &detail)
                    .await;
            }
        }
    }

    /// 把一次投递的真实结果落到会话上，返回落笔后的连续失败次数
    ///
    /// 单独拎出来、并且只在拿到 [`ResumeOutcome`] 之后调用，是这一版的核心修正。
    /// 三个计数器驱动三种完全不同的行为，所以必须分开写：
    ///
    /// - `resume_count`：累计帮了多少次，**只给人看**，不参与判定。
    /// - `resume_streak`：连着催了几次还不见动静，对着 `max_resume_count` 那道上限。
    ///   会话一旦恢复推进就由 `scan_once` 清零。
    /// - `resume_failures`：连着几次根本没送达，驱动退避和升级告警。送达即清零。
    ///
    /// 三者共同的前提是**投递失败时一个都不加**——否则「敲不进去」会被当成
    /// 「已经催够了」，自动续跑在一个字都没敲进去的情况下自己把自己关掉。
    ///
    /// `last_resume_at` 两种情况都写：它是冷却计时的起点，失败也得等一会儿再试，
    /// 不然通道坏掉时会变成每轮扫描都去撞一次墙。
    async fn commit_resume_outcome(&self, session_id: &str, outcome: ResumeOutcome) -> u32 {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut state = self.state.lock().await;
        if outcome.counts_as_nudge() {
            state.status.total_resumes += 1;
        }
        let Some(session) = state.sessions.iter_mut().find(|s| s.id == session_id) else {
            return 0;
        };
        apply_resume_outcome(session, outcome, now);
        session.resume_failures
    }

    /// 连续失败到一定次数就**别再静默了**
    ///
    /// 这个功能整个价值就是「用户无感」，所以它自己坏掉的时候必须反过来：
    /// 静默地失败等于装作在工作。一次失败可能只是恰好被别的窗口抢了焦点；
    /// 连着 [`RESUME_FAILURE_ALERT_AT`] 次基本就是通道本身坏了（权限掉了、
    /// 终端关了、pane 没了），那就弹一次通知，把能照着做的那句话给出去。
    /// `notify_alert` 自带 ≥600 秒节流，不会变成刷屏。
    async fn escalate_resume_failure(
        &self,
        config: &AppConfig,
        i18n: &I18n,
        session: &AgentSession,
        failures: u32,
        detail: &str,
    ) {
        if failures < RESUME_FAILURE_ALERT_AT {
            return;
        }
        let Some(notifier) = self.notifier.get() else {
            return;
        };
        let body = i18n.tf(
            "notify.resume_broken.body",
            &[
                ("label", &session_label(session)),
                ("count", &failures.to_string()),
                ("detail", detail),
            ],
        );
        notifier.notify_alert(
            &config.notification,
            &format!("resume:broken:{}", session.id),
            i18n.t("notify.resume_broken.title"),
            &body,
        );
    }

    /// 开工前先给投递通道做个体检
    ///
    /// 补的是「诊断能力有、但从来不主动用」这个洞：应用早就能查辅助功能授权，
    /// 却只在用户手动点「续跑演练」时查。于是权限失效这件事，总是等到某个会话
    /// 已经卡住、额度也白烧了几次之后才被发现。
    ///
    /// 只写日志、不弹通知：tmux / screen / iTerm2 三条通道根本不需要这个授权，
    /// 对那些用户来说弹窗是误报。真的敲不进去时，`escalate_resume_failure`
    /// 会拿着实证去吵。
    async fn check_resume_channel(&self, config: &AppConfig, i18n: &I18n) {
        if !config.auto_resume_enabled {
            return;
        }
        if let Some(hint) = crate::resumer::channel_health(&config.language).await {
            self.push_event(EngineEvent::new(
                LogLevel::Warn,
                None,
                i18n.tf("log.channel_unhealthy", &[("detail", &hint)]),
            ))
            .await;
        }
    }

    /// 手动续跑之后同步会话上的续跑状态
    ///
    /// **故意不动 `resume_count`**：那个计数对着 `max_resume_count` 那道上限，
    /// 上限管的是「别没完没了地自动催」——用户自己点的那一次不该消耗它。
    ///
    /// 但 `last_resume_at` 必须写：不写的话，下一轮扫描的自动续跑会看到一个
    /// 空的冷却计时器，立刻再敲一遍，同一个会话吃两条提示词。
    /// `resume_failures` 也跟着走：手动敲成了就证明通道是好的，自动那边的退避
    /// 该马上松开。
    pub async fn note_manual_resume(&self, session_id: &str, outcome: ResumeOutcome) {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut state = self.state.lock().await;
        let Some(session) = state.sessions.iter_mut().find(|s| s.id == session_id) else {
            return;
        };
        session.last_resume_at = Some(now);
        if outcome.is_failure() {
            session.resume_failures = session.resume_failures.saturating_add(1);
        } else {
            session.resume_failures = 0;
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
                self.push_event(EngineEvent::new(LogLevel::Warn, None, body))
                    .await;
            }
        }

        if config.cost.session_budget_usd > 0.0 {
            for session in sessions {
                let Some(usage) = &session.usage else {
                    continue;
                };
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
                self.push_event(EngineEvent::new(LogLevel::Warn, None, body))
                    .await;
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

/// 记录指纹：只需要稳定地区分「同一段」和「已经变化」，不用于安全边界。
fn transcript_fingerprint(output: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    output.hash(&mut hasher);
    hasher.finish()
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

/// 失败越多，越沉得住气
///
/// 固定 30 秒冷却在通道好使的时候没问题；通道坏掉时它就成了每 30 秒撞一次墙——
/// 日志被刷满、AppleScript 反复抢前台窗口把用户正在打的字打断，而结果一次都不会变。
///
/// 所以按连续失败次数线性拉长，最多 6 倍。**只线性、有上限**是两个都要的：
/// 指数退避几次之后就会退到几小时，用户明明已经把权限补上了，却还要干等；
/// 6 倍（默认 3 分钟）刚好是「不烦人」和「修好了马上就能恢复」之间那个位置。
fn effective_cooldown(base_secs: u64, failures: u32) -> u64 {
    base_secs.saturating_mul(1 + failures.min(5) as u64)
}

/// 还该不该继续往这个会话里敲字
///
/// 这道闸门只拦**动作**，不改判定。区别是实打实的：
/// 上一版把它写在 `Detector::make_verdict` 里当提前返回，额度一光判定就降级成
/// 「疑似」，而 `grade_attention` 对「疑似」的处理是不打扰——于是应用一边放弃
/// 自己动手，一边把该给人的提醒也一起收了，最后谁都没管这个会话。
/// 「它其实没干完活，每次都要我去发继续」有一半是这么来的。
///
/// 现在判定照实说「确认中断」，注意力分级照常升到 `NeedsInput` 叫人，
/// 只有敲字这一件事停下来。放弃动手的那一刻，正是最该开口的一刻。
///
/// 数的是**连击**（`resume_streak`）而不是累计次数：上限想拦的是「对着一个
/// 不响应的会话空转」，不是「一个会话一辈子只准被催 5 次」。会话一动就清零。
fn has_nudges_left(session: &AgentSession, max: u32) -> bool {
    session.resume_streak < max
}

/// 这句话现在还值不值得说
///
/// 抽成自由函数的理由跟 [`apply_resume_outcome`] 一样：裹在锁里的策略测不到。
/// 规则一句话——同一个话题、同一个情况，只说第一次。
fn should_say(said: &mut HashMap<String, String>, topic: String, fingerprint: &str) -> bool {
    if said.get(&topic).is_some_and(|last| last == fingerprint) {
        return false;
    }
    said.insert(topic, fingerprint.to_string());
    true
}

/// 一次投递结果该怎么改写会话上的三个计数器
///
/// 从 `commit_resume_outcome` 里抽出来是为了**能单测**：这段逻辑正是上一版出错的
/// 地方（计数在投递之前就加、失败不回退），而它原来被一把 async 锁裹着，
/// 裹在锁里的策略是测不到的。规则本身只有一句：
/// **没送达就一个计数都不许动**，送达了才既算一次帮忙、也算一次连击。
fn apply_resume_outcome(session: &mut AgentSession, outcome: ResumeOutcome, now: String) {
    session.last_resume_at = Some(now);
    if outcome.is_failure() {
        session.resume_failures = session.resume_failures.saturating_add(1);
    } else {
        session.resume_count = session.resume_count.saturating_add(1);
        session.resume_streak = session.resume_streak.saturating_add(1);
        session.resume_failures = 0;
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

    // ── 投递结果 → 计数器：v1.5 修掉的那个「自己把自己关掉」的缺陷 ──

    fn stamp() -> String {
        Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    #[test]
    fn failed_delivery_never_burns_the_budget() {
        // 这就是用户报的「自动续跑好像根本不工作」：macOS 辅助功能授权每次
        // 重新构建应用都会失效，于是每次投递都失败。旧代码在投递前就加计数，
        // 五次之后 `max_resume_count` 到顶，这个会话从此永久沉默——
        // 而一个字都没真的敲进去过。
        let mut s = session_with("/tmp", None);
        for _ in 0..10 {
            apply_resume_outcome(&mut s, ResumeOutcome::Failed, stamp());
        }
        assert_eq!(s.resume_count, 0, "没送达就不算帮过忙");
        assert_eq!(s.resume_streak, 0, "更不该消耗上限额度");
        assert_eq!(s.resume_failures, 10, "但要记得这条通道一直在失败");
        assert!(s.last_resume_at.is_some(), "冷却计时还是要走，别每轮都撞墙");
    }

    #[test]
    fn silent_delivery_counts_as_failure() {
        // 脚本说成功、会话一动没动：从用户角度这跟报错是同一件事——没人替他按继续
        let mut s = session_with("/tmp", None);
        apply_resume_outcome(&mut s, ResumeOutcome::Silent, stamp());
        assert_eq!(s.resume_count, 0);
        assert_eq!(s.resume_failures, 1);
    }

    #[test]
    fn landed_delivery_clears_the_failure_streak() {
        let mut s = session_with("/tmp", None);
        apply_resume_outcome(&mut s, ResumeOutcome::Failed, stamp());
        apply_resume_outcome(&mut s, ResumeOutcome::Failed, stamp());
        apply_resume_outcome(&mut s, ResumeOutcome::Landed, stamp());
        assert_eq!(s.resume_failures, 0, "通道又通了，退避该立刻松开");
        assert_eq!(s.resume_count, 1);
        assert_eq!(s.resume_streak, 1);
    }

    #[test]
    fn unverifiable_delivery_still_counts() {
        // Codex / OpenCode 不落可读的记录文件，核验不了。这种情况按「大概进去了」
        // 记账，跟改动前的行为一致——不能因为核验不了就判它失败，那是无根据地
        // 给一条本来好使的通道判死刑。
        let mut s = session_with("/tmp", None);
        apply_resume_outcome(&mut s, ResumeOutcome::Unverifiable, stamp());
        assert_eq!(s.resume_count, 1);
        assert_eq!(s.resume_streak, 1);
        assert_eq!(s.resume_failures, 0);
    }

    #[test]
    fn backoff_grows_then_stops_growing() {
        assert_eq!(effective_cooldown(30, 0), 30);
        assert_eq!(effective_cooldown(30, 1), 60);
        assert_eq!(effective_cooldown(30, 5), 180);
        assert_eq!(
            effective_cooldown(30, 99),
            180,
            "退避要有上限：用户把权限补回来之后不该还得干等几小时"
        );
    }

    #[test]
    fn backoff_never_overflows() {
        assert_eq!(effective_cooldown(u64::MAX, 3), u64::MAX);
    }

    #[test]
    fn exhausted_streak_stops_typing_but_not_watching() {
        // 额度用光只该关掉「敲字」这一个动作。判定和提醒都不受影响——
        // 这条测试就是为了钉住那个曾经的悄悄放弃：额度一光，判定被降级成
        // 「疑似」，注意力分级于是不再叫人，会话被彻底遗忘。
        let max = 3;
        let mut s = session_with("/tmp/demo", None);
        assert!(has_nudges_left(&s, max), "刚开始就有额度");

        for landed in 1..=max {
            apply_resume_outcome(&mut s, ResumeOutcome::Unverifiable, stamp());
            assert_eq!(s.resume_streak, landed);
        }
        assert!(!has_nudges_left(&s, max), "连着催满就该停手");

        // 会话自己动起来 → `scan_once` 把连击清零，额度整个还回来
        s.resume_streak = 0;
        assert!(
            has_nudges_left(&s, max),
            "它一动就该重新守着它，不是一辈子只准被催 max 次"
        );
    }

    #[test]
    fn failed_deliveries_never_exhaust_the_budget() {
        // 敲不进去不算「催过」。这两件事混在一起时，权限一掉就会连着记 5 次
        // 「催过了」，然后自动续跑对这个会话永久沉默，而用户什么提示也收不到。
        let max = 3;
        let mut s = session_with("/tmp/demo", None);
        for _ in 0..20 {
            apply_resume_outcome(&mut s, ResumeOutcome::Failed, stamp());
        }
        assert!(
            has_nudges_left(&s, max),
            "一次都没送达，额度一格都不该被吃掉"
        );
    }

    #[test]
    fn standing_conditions_are_only_announced_once() {
        // 「已经催不动了」这个情况会一直成立。每 10 秒重播一次的结果是日志面板
        // 被自己的历史刷满，新消息还没被看见就被挤下去了。
        let mut said = HashMap::new();
        let topic = MonitorEngine::exhausted_topic("cc-1");

        assert!(should_say(&mut said, topic.clone(), "3"), "第一次要说");
        for _ in 0..5 {
            assert!(
                !should_say(&mut said, topic.clone(), "3"),
                "情况没变就别重复"
            );
        }
        assert!(
            should_say(&mut said, topic.clone(), "4"),
            "连击数变了是新情况，该再说一次"
        );

        // 会话恢复后要忘掉这条话题，否则同一个情况第二次发生就说不出口了
        said.remove(&topic);
        assert!(should_say(&mut said, topic, "4"));
    }
}
