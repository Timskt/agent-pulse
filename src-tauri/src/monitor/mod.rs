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
use crate::resumer::{activity_fingerprint, ActivityFingerprint, ResumeOutcome, Resumer};
use crate::storage::Storage;
use crate::webhook::WebhookNotifier;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, Notify};
use tokio::time::{interval, Duration, MissedTickBehavior};

/// 连续失败几次就该改成大声说
///
/// 1 次可能只是恰好前台窗口被别人抢了；连着 2 次基本就是通道本身坏了。
/// 门槛压得低是故意的：这个功能的失败模式是**静默**，而静默的失败
/// 跟「正常工作」在屏幕上长得一模一样，晚说一小时就是白等一小时。
const RESUME_FAILURE_ALERT_AT: u32 = 2;

/// 活动日志最多留多少条
///
/// 日志面板是给人看的，不是审计追踪——真要追历史有 SQLite 里的
/// `resume_records` / `detection_records`。
pub const EVENT_RING_CAP: usize = 500;

/// 这一轮该把事件环末尾的多少条推给前端
///
/// 单独抽成函数是因为 bug 就出在这段算术上：原来的泵拿 `events.len()` 当游标，
/// 而这个长度攒到 [`EVENT_RING_CAP`] 就不再变了，于是「长度没变 = 没有新事件」
/// 在那之后**永远成立**，活动日志静默停更。用只增的累计数当游标才认得出新事件。
///
/// 返回值封顶在 `ring_len`：环里已经被裁掉的那些，前端这辈子都拿不到了，
/// 硬要按差值切会直接越界 panic。这种情况下丢的是最老的几条，
/// 而日志面板本来就只显示最近的——比崩掉好。
pub fn fresh_tail(pushed: u64, sent: u64, ring_len: usize) -> usize {
    // `pushed < sent` 正常跑不出来（计数只增）。但真出现时用饱和减
    // 得到 0（这一轮不推），而不是让 `u64` 减法在 debug 下 panic、
    // 在 release 下绕成一个天文数字然后按它去切片。
    let behind = pushed.saturating_sub(sent);
    behind.min(ring_len as u64) as usize
}

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
    /// 已经进入协调器、尚未完成真实输入的续跑数（含等待全局投递锁）。
    pub resume_pending: usize,
    /// 已完成输入、正在只读核验会话记录的续跑数。
    pub resume_verifying: usize,
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
    /// 一共往 `events` 里推过多少条（**只增**，不随事件环裁剪回落）
    ///
    /// 推送泵靠它认「哪些是新的」。用 `events.len()` 不行：那个长度到 500 就
    /// 不动了，于是「长度没变 = 没有新事件」这个判断在攒够 500 条之后**永远成立**，
    /// 活动日志从此静默停更——后端还在记，界面上再也不出现新的一行。
    ///
    /// 不发给前端（`skip`）：前端拿 `events` 数组本身就够了，多一个它用不上的
    /// 计数只会让人以为该拿它做点什么。
    #[serde(skip)]
    pub events_pushed: u64,
}

impl MonitorState {
    /// 往活动日志里追加一条，并把环裁回上限
    ///
    /// 逻辑放在状态自己身上而不是引擎上，是为了能测：`MonitorEngine` 要
    /// `ConfigManager` 和 `Storage` 才建得起来，而 `ConfigManager::new()` 会往
    /// 用户真正那份 `config.json` 里写东西——为了测两行计数去碰用户的配置文件，
    /// 代价和收益完全不成比例。
    ///
    /// 两个计数的分工是这段代码的全部要点：`events_pushed` 跟着**推过多少**走，
    /// `events.len()` 跟着**留着多少**走。它们一旦被写成同一个数，推送泵就再也
    /// 认不出新事件（见 [`fresh_tail`]）。
    pub fn push_event(&mut self, event: EngineEvent) {
        self.events.push(event);
        if self.events.len() > EVENT_RING_CAP {
            let drain_count = self.events.len() - EVENT_RING_CAP;
            self.events.drain(0..drain_count);
        }
        self.events_pushed = self.events_pushed.saturating_add(1);
    }
}

/// 一轮扫描在阻塞线程里攒出来的全部结果
struct ScanOutcome {
    sessions: Vec<AgentSession>,
    detections: Vec<DetectionSnapshot>,
    /// 判定层明确说「再问一句可能改变结果」的候选；异步侧每轮最多问一个
    to_arbitrate: Vec<ArbitrationRequest>,
    /// 本轮新落库的用量记录条数
    usage_added: usize,
    /// 今日累计花费（美元）
    cost_today: f64,
    /// 限流窗口预测；没配 token 预算时为 None
    forecast: Option<RateLimitForecast>,
}

/// 一次判定连同它读取记录时看到的版本。
///
/// 判定和动作之间不能只靠 `session_id` 连接：记录只要又长了一行，刚才那份“卡住”
/// 结论就已经过期，必须交给下一轮重新判断，不能拿旧结论继续往终端里敲字。
struct DetectionSnapshot {
    session: AgentSession,
    detection: DetectionResult,
    activity: Option<ActivityFingerprint>,
}

struct ResumeAction {
    session: AgentSession,
    use_goal_prompt: bool,
    observed_activity: Option<ActivityFingerprint>,
    /// 绑定生成动作时那一次守护生命周期；停止后即使马上重启，旧动作也不能复活。
    lifecycle_epoch: u64,
    /// 检测快照形成时已经停顿了多久；不能把排队与投递后核验的耗时算进去。
    stuck_secs: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ResumeCommit {
    resume_count: u32,
    resume_failures: u32,
}

/// 会话级续跑租约表。
///
/// 自动扫描、手动按钮乃至未来的远程入口都必须先从这里拿租约。租约离开作用域时
/// 自动释放，因此新增早退分支、`?` 返回甚至任务 unwind 都不会把会话永久卡在
/// “续跑处理中”。真正的投递仍由全局 `delivery_lock` 串行化；这里管的是同会话幂等。
#[derive(Default)]
struct ResumeRegistry {
    sessions: std::sync::Mutex<HashSet<String>>,
    /// 新动作入队或任一会话租约释放时唤醒协调 worker。
    wake: Notify,
}

impl ResumeRegistry {
    fn try_acquire(self: &Arc<Self>, session_id: &str) -> Option<ResumeLease> {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !sessions.insert(session_id.to_string()) {
            return None;
        }
        Some(ResumeLease {
            registry: Arc::clone(self),
            session_id: session_id.to_string(),
        })
    }

    #[cfg(test)]
    fn is_active(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(session_id)
    }

    fn notify_worker(&self) {
        self.wake.notify_one();
    }

    async fn wait_for_work(&self) {
        self.wake.notified().await;
    }
}

struct ResumeLease {
    registry: Arc<ResumeRegistry>,
    session_id: String,
}

impl Drop for ResumeLease {
    fn drop(&mut self) {
        self.registry
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.session_id);
        // 队列里可能正保留着这个 session 的最新后继；释放即唤醒，
        // 不依赖下一轮扫描，也不怕调用方早退或 unwind 忘记通知。
        self.registry.notify_worker();
    }
}

/// 一个并发阶段的 RAII 计数器。
///
/// 状态快照会把这两个计数展示给用户；任何早退或 unwind 都必须把数字还回去，
/// 否则界面会永久显示“仍在投递/核验”。
struct PhaseCounter<'a> {
    counter: &'a AtomicUsize,
}

impl<'a> PhaseCounter<'a> {
    fn enter(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl Drop for PhaseCounter<'_> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 把协调器内部的三个瞬时数字合并进公开状态。
///
/// 排队动作与已经派发、等待桌面锁的动作互斥存在，因此 pending 是两者之和；
/// 核验阶段已经完成真实输入，必须单列，不能让界面误以为还在等着敲字。
fn merge_resume_pipeline_status(
    status: &mut EngineStatus,
    queued: usize,
    delivery_pending: usize,
    verifying: usize,
) {
    status.resume_pending = queued.saturating_add(delivery_pending);
    status.resume_verifying = verifying;
}

/// 自动续跑协调队列：同一会话只保留最新动作，不让扫描频率把旧快照堆成长龙。
///
/// `order` 保证不同会话先进先出，`actions` 负责按 session id 合并。一个会话正在
/// 投递时，队列仍可保留一条更新鲜的后继快照：当前动作若因等待全局锁而过期，worker
/// 可以立刻接上最新动作；若当前动作已经送达，后继动作会被冷却/状态重验安全取消。
#[derive(Default)]
struct ResumeQueue {
    order: VecDeque<String>,
    actions: HashMap<String, ResumeAction>,
}

impl ResumeQueue {
    fn upsert(&mut self, action: ResumeAction) {
        let session_id = action.session.id.clone();
        if !self.actions.contains_key(&session_id) {
            self.order.push_back(session_id.clone());
        }
        self.actions.insert(session_id, action);
    }

    /// 取出第一条当前能拿到会话租约的动作。
    ///
    /// 已经有动作在核验的会话不会挡住队列后面的其他会话：它的最新后继快照
    /// 保留在队尾，等租约释放后再重验。最多检查当前队列一圈，避免所有会话
    /// 都忙时原地旋转占满 CPU。
    fn pop_ready<T>(
        &mut self,
        mut try_acquire: impl FnMut(&str) -> Option<T>,
    ) -> Option<(ResumeAction, T)> {
        let candidates = self.order.len();
        for _ in 0..candidates {
            let Some(session_id) = self.order.pop_front() else {
                break;
            };
            let Some(action) = self.actions.remove(&session_id) else {
                continue;
            };

            if let Some(token) = try_acquire(&session_id) {
                return Some((action, token));
            }

            self.order.push_back(session_id.clone());
            self.actions.insert(session_id, action);
        }
        None
    }

    fn clear(&mut self) {
        self.order.clear();
        self.actions.clear();
    }

    fn len(&self) -> usize {
        self.actions.len()
    }
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
    /// 扫描不可重入：后台节拍、托盘“立即扫描”和界面按钮可能同时触发。
    /// 不串行会让两轮都看到同一个中断，再各自投递一遍。
    scan_lock: Mutex<()>,
    /// 所有真实键盘/剪贴板投递共用一把锁。前台窗口和剪贴板都是全局单件，
    /// 手动续跑也必须跟自动续跑走同一条串行通道。
    delivery_lock: Mutex<()>,
    /// 同一个会话同一时刻只允许存在一个续跑意图；租约离开作用域时自动释放。
    resume_registry: Arc<ResumeRegistry>,
    /// 自动动作进入专用协调队列；扫描只产出动作，不再等待每个动作的落地核验。
    resume_queue: std::sync::Mutex<ResumeQueue>,
    resume_worker_started: AtomicBool,
    /// 已从合并队列派发、正在等全局锁或执行真实输入的动作数。
    resume_delivery_pending: AtomicUsize,
    /// 已释放桌面级投递锁、正在观察各自会话记录的动作数。
    resume_verifying: AtomicUsize,
    /// 每次开始/停止都递增。动作绑定生成时的代数，防止“停一下又启动”复活旧队列。
    lifecycle_epoch: AtomicU64,
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
            scan_lock: Mutex::new(()),
            delivery_lock: Mutex::new(()),
            resume_registry: Arc::new(ResumeRegistry::default()),
            resume_queue: std::sync::Mutex::new(ResumeQueue::default()),
            resume_worker_started: AtomicBool::new(false),
            resume_delivery_pending: AtomicUsize::new(0),
            resume_verifying: AtomicUsize::new(0),
            lifecycle_epoch: AtomicU64::new(0),
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

    /// 面向界面/API 的一致状态快照。
    ///
    /// 会话与检测结果住在 async 状态锁里，协调器的瞬时阶段则用原子计数和有界
    /// 队列维护。读取时在这里合并，避免每次阶段切换都为两个数字争用整份状态锁。
    pub async fn snapshot(&self) -> MonitorState {
        let mut snapshot = self.state.lock().await.clone();
        let queued = self
            .resume_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        merge_resume_pipeline_status(
            &mut snapshot.status,
            queued,
            self.resume_delivery_pending.load(Ordering::SeqCst),
            self.resume_verifying.load(Ordering::SeqCst),
        );
        snapshot
    }

    pub async fn status_snapshot(&self) -> EngineStatus {
        self.snapshot().await.status
    }

    /// 添加事件日志
    /// 往活动日志里追加一条
    ///
    /// 公开出来是给 `remote` 模块用的：看板启动/停止也该出现在同一条日志流里，
    /// 用户不用去别处找「看板到底起来了没有」。
    pub async fn push_event(&self, event: EngineEvent) {
        tracing::info!("[AgentPulse] {}", event.message);
        self.state.lock().await.push_event(event);
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
    pub async fn start(self: &Arc<Self>) {
        {
            let mut state = self.state.lock().await;
            // 托盘和界面各点一次「开始监控」不该起两条循环
            if state.running {
                return;
            }
            state.running = true;
            state.status.running = true;
            self.lifecycle_epoch.fetch_add(1, Ordering::SeqCst);
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
            self.lifecycle_epoch.fetch_add(1, Ordering::SeqCst);
        }

        // 尚未拿到投递通道的自动动作全部取消；生命周期代数还会挡住已经出队、
        // 正在等待手动动作释放全局锁的那一条。手动续跑不在这个队列里，不受影响。
        self.resume_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();

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
    pub async fn scan_once(self: &Arc<Self>) {
        // `scan_now`、托盘菜单和后台 ticker 都能走到这里。两轮并发不只是多读一次盘：
        // 它们会同时基于同一份旧状态安排续跑，最终给同一会话敲两条提示词。
        let _scan_guard = self.scan_lock.lock().await;

        let config = self.config();
        let lang = config.language.clone();
        let i18n = I18n::from_code(&lang);

        self.storage.record_scan();

        let (existing, auto_resume_armed, lifecycle_epoch): (
            HashMap<String, AgentSession>,
            bool,
            u64,
        ) = {
            let state = self.state.lock().await;
            (
                state
                    .sessions
                    .iter()
                    .map(|s| (s.id.clone(), s.clone()))
                    .collect(),
                state.running,
                self.lifecycle_epoch.load(Ordering::SeqCst),
            )
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
        let mut resume_actions: Vec<ResumeAction> = Vec::new();
        // 本轮**新**确认中断的会话数
        //
        // 一个会话可以连着几十轮都是「确认中断」（用户关了自动续跑，或者已经
        // 催不动了）。那是一个持续的状态，不是几十次检测——每轮都记一笔，
        // 界面上的检测数就会跟 `detection_records` 里的行数越差越远。
        let mut newly_confirmed: u32 = 0;

        for snapshot in &detections {
            let session = &snapshot.session;
            let detection = &snapshot.detection;
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
                        let reason_text = i18n.t(detection.interrupt_reason.i18n_key());
                        // 限流保持窗口要把截止时刻说出来。只说「在等」的话，
                        // 用户看到的是一段没有尽头的沉默——那跟守护神漏了一次
                        // 分不出来，而这里恰恰是它做了一个正确的决定。
                        let message = match (&detection.rate_limit_hold, tactic) {
                            (Some(hold), ResumeTactic::Wait) => i18n.tf(
                                "log.rate_limit_hold",
                                &[
                                    ("agent", &session.agent_name),
                                    ("reason", reason_text),
                                    ("marker", hold.marker.as_deref().unwrap_or("-")),
                                    ("until", &hold.until),
                                ],
                            ),
                            _ => {
                                let key = if tactic == ResumeTactic::Wait {
                                    "log.resume_wait"
                                } else {
                                    "log.resume_hand_off"
                                };
                                i18n.tf(
                                    key,
                                    &[("agent", &session.agent_name), ("reason", reason_text)],
                                )
                            }
                        };
                        self.push_event_on_change(
                            Self::tactic_topic(&session.id),
                            detection.interrupt_reason.key(),
                            EngineEvent::new(LogLevel::Warn, Some(session.id.clone()), message),
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
                    if cooled && has_budget && config.auto_resume_enabled && auto_resume_armed {
                        resume_actions.push(ResumeAction {
                            session: session.clone(),
                            use_goal_prompt: detection.has_active_goal,
                            observed_activity: snapshot.activity,
                            lifecycle_epoch,
                            stuck_secs: session.stuck_secs(),
                        });
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
        // 计数一律等 续跑 worker 拿到真实结果之后由 `commit_resume_outcome` 落笔。
        // 旧代码在这儿就把计数加了、失败也不回退，等于把「敲不进去」记成
        // 「已经敲够了」，五次之后自动续跑对这个会话永久沉默——详见
        // `ResumeOutcome::counts_as_nudge` 上的说明。
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let running_session_ids: HashSet<String> = detections
            .iter()
            .filter(|snapshot| snapshot.detection.verdict == Verdict::Running)
            .map(|snapshot| snapshot.session.id.clone())
            .collect();
        for session in &mut sessions {
            if let Some(snapshot) = detections
                .iter()
                .find(|snapshot| snapshot.session.id == session.id)
            {
                let detection = &snapshot.detection;
                session.attention = detection.attention;
                session.attention_detail = detection.attention_detail.clone();
                session.detection_evidence = Some(detection.evidence.clone());
                session.interrupt_reason = detection.interrupt_reason;
                session.rate_limit_hold = detection.rate_limit_hold.clone();
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

        // ── 会话历史落库：**必须在上面那段合并之后** ──
        //
        // 位置有讲究。以前这一步在 `collect()` 里，那会儿本轮判定还没合并回
        // `session.status`，写进历史表的永远是上一轮的结论。而且它只写「本轮
        // 发现的」会话，于是用户关掉的会话那一行再也没人碰——连同
        // `last_status = 'active'` 冻在库里，历史页因此一直显示「运行中」。
        self.sync_session_history(&config, &sessions);

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
            // 投递核验已从扫描临界区拆出，因此上一轮动作可能在本轮读盘期间完成。
            // 不能用扫描开始时复制的计数覆盖刚落笔的投递结果；在同一把状态锁内把
            // 续跑运行态重新合并回来。只有本轮明确看见 `Running` 才清空自动连击。
            merge_resume_runtime(&mut sessions, &state.sessions, &running_session_ids);
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

        // 从这里开始不再持有扫描锁。投递后的落地核验最长要等数秒，多个会话串行时
        // 更久；把它留在扫描临界区会让“立即扫描”和下一轮检测一起排队。动作自身有
        // 会话租约、全局投递锁和出手前失效检查，因此可以安全地与下一轮检测重叠。
        drop(_scan_guard);
        self.enqueue_resume_actions(resume_actions);
    }

    /// 把本轮的会话状态写进历史表，并给已经消失的会话收尾
    ///
    /// 两件事必须挨在一起做，因为它们共用「本轮还活着的键」这一份事实：
    /// 先把看得见的写进去（顺带把复活的会话 `ended_at` 清空），
    /// 再把没写到的那些盖上收尾时间。
    ///
    /// **适配器全关的时候直接返回**，见 [`should_reconcile_history`]。
    fn sync_session_history(&self, config: &AppConfig, sessions: &[AgentSession]) {
        if !should_reconcile_history(config) {
            return;
        }
        let mut live_keys = Vec::with_capacity(sessions.len());
        for session in sessions {
            let key = session.history_key();
            let usage = session.usage.clone().unwrap_or_default();
            self.storage.upsert_session_history(
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
            live_keys.push(key);
        }
        let closed = self.storage.close_missing_sessions(&live_keys);
        if closed > 0 {
            tracing::debug!("[AgentPulse] {closed} 个会话已从视野消失，历史里收尾");
        }
    }

    /// 一轮扫描的重活：进程枚举、输出读取、成本解析
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
                    // 限流保持窗口要跨轮活着，这正是它存在的全部理由：
                    // 那行 `429` 会滚出记录尾部 40 行，窗口不能跟着它一起消失
                    session.rate_limit_hold = old.rate_limit_hold.clone();
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
            let mut detections: Vec<DetectionSnapshot> = Vec::new();
            let mut to_arbitrate = Vec::new();
            for adapter in &adapter_list {
                for session in &sessions {
                    if session.adapter_id != adapter.id() {
                        continue;
                    }
                    let alive = live_pids.contains(&session.pid);
                    let activity_before = activity_fingerprint(session);
                    let output = adapter.recent_output(session);
                    // 故障单独走一条通道：散文里提到「500」不算出错，
                    // 只有记录自己标成故障的行才算
                    let errors = adapter.error_output(session);
                    // 回合结构：区分「正在跑工具/压缩上下文」和「真的停下来等人」，
                    // 光看文件 mtime 这两者长得一样
                    let turn = adapter.turn_state(session);
                    let activity_after = activity_fingerprint(session);

                    // 三次读取（正文、错误、回合结构）期间记录变了，说明会话此刻正在
                    // 推进。这一轮任何“卡住”结论都建立在混合版本上，既不合并进状态，
                    // 更不能拿去续跑；等下一轮对稳定快照重新判定。
                    if activity_before != activity_after {
                        tracing::debug!(
                            "[AgentPulse] 会话 {} 在判定期间仍有活动，本轮跳过旧快照",
                            session.id
                        );
                        continue;
                    }

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
                    detections.push(DetectionSnapshot {
                        session: session.clone(),
                        detection: result,
                        activity: activity_after,
                    });
                }
            }

            // 6. 今日花费与限流预测（都是 SQL 聚合，一并在这里算完）
            //
            // 会话历史的落库**不在这里**：这会儿 `session.status` 还是上一轮的
            // 结论，本轮判定要等 `scan_once` 合并回去。在这儿写等于让历史表
            // 永远慢一轮，一个刚中断的会话在历史里还标着「运行中」。
            // 见 `scan_once` 里的 `sync_session_history`。
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

    /// 将本轮动作交给专用续跑协调器。
    ///
    /// 扫描到这里就结束，不等待 AppleScript 与最长数秒的落地核验。同一会话的后续
    /// 扫描只会替换队列里的旧快照，不会无限堆积；worker 跳过 leased session，
    /// 为其他会话派发任务，只有真实桌面投递在全局锁上公平串行。
    fn enqueue_resume_actions(self: &Arc<Self>, actions: Vec<ResumeAction>) {
        if actions.is_empty() {
            return;
        }
        self.ensure_resume_worker();

        let mut queued = false;
        let mut queue = self
            .resume_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for action in actions {
            // 不因同会话已有 in-flight 就丢掉新证据。它可能仍在核验和记账；这期间
            // 旧动作会过期，而最新动作应作为唯一后继留在队列。若 in-flight 已经成功，
            // 后继动作取得 lease 后会被冷却与状态重验取消，不会双投。
            queue.upsert(action);
            queued = true;
        }
        drop(queue);
        if queued {
            self.resume_registry.notify_worker();
        }
    }

    /// worker 全进程只启动一次；所有自动动作都从这里经过。
    fn ensure_resume_worker(self: &Arc<Self>) {
        if self
            .resume_worker_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            engine.resume_worker().await;
        });
    }

    async fn resume_worker(self: Arc<Self>) {
        loop {
            // 每个会话的租约一直持有到核验和记账结束，但已经完成真实输入的会话
            // 不再占住全局投递锁。worker 可以继续派发其他会话；它们只在不可逆
            // 的窗口/剪贴板/键盘阶段排队，随后各自并行观察自己的记录文件。
            while let Some((action, lease)) = self.dequeue_ready_resume_action() {
                let engine = Arc::clone(&self);
                tokio::spawn(async move {
                    engine.run_auto_resume(action, lease).await;
                    // lease 的 Drop 会释放会话并唤醒 worker；早退和 unwind 也走同一路径。
                });
            }
            self.resume_registry.wait_for_work().await;
        }
    }

    fn dequeue_ready_resume_action(&self) -> Option<(ResumeAction, ResumeLease)> {
        self.resume_queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_ready(|session_id| self.resume_registry.try_acquire(session_id))
    }

    /// 执行一条自动动作。
    ///
    /// 只有不可逆的真实输入必须串行：剪贴板和前台窗口是全局单件。输入完成、脚本
    /// 恢复剪贴板后立即释放 `delivery_lock`；最长 6 秒的记录核验只读目标会话文件，
    /// 可以与其他会话并行。会话租约仍覆盖到核验、记账和通知全部结束，保证同会话
    /// 不会在上一条结果尚未落笔时又投递一次。
    async fn run_auto_resume(self: &Arc<Self>, action: ResumeAction, _lease: ResumeLease) {
        let session = &action.session;
        let delivery_phase = PhaseCounter::enter(&self.resume_delivery_pending);

        let attempt = {
            let _delivery_guard = self.delivery_lock.lock().await;
            let latest_config = self.config();
            if !self.auto_action_is_current(&action, &latest_config).await {
                None
            } else {
                let resumer = Resumer::new(latest_config);
                Some((
                    resumer.deliver(session, action.use_goal_prompt).await,
                    resumer,
                ))
            }
        };
        drop(delivery_phase);

        let latest_config = self.config();
        let i18n = I18n::from_code(&latest_config.language);
        let Some((delivery, resumer)) = attempt else {
            self.push_event_on_change(
                format!("resume_stale:{}", session.id),
                &format!("{}:{:?}", action.lifecycle_epoch, action.observed_activity),
                EngineEvent::new(
                    LogLevel::Info,
                    Some(session.id.clone()),
                    i18n.t("log.resume_stale_skip"),
                ),
            )
            .await;
            return;
        };

        let (outcome, detail) = match delivery {
            Ok(delivery) => {
                let verify_phase = PhaseCounter::enter(&self.resume_verifying);
                let result = resumer.verify_delivery(session, delivery).await;
                drop(verify_phase);
                result
            }
            Err(error) => (ResumeOutcome::Failed, error),
        };

        let prompt_type = if action.use_goal_prompt {
            "goal"
        } else {
            "generic"
        };
        let mode = i18n.t(if action.use_goal_prompt {
            "log.mode_goal"
        } else {
            "log.mode_generic"
        });

        let landed = outcome.counts_as_nudge();
        let commit = self.commit_resume_outcome(&session.id, outcome).await;

        self.storage.record_resume(crate::storage::ResumeEvent {
            session_id: &session.id,
            agent_name: &session.agent_name,
            working_dir: &session.working_dir,
            prompt_type,
            success: landed,
            outcome: outcome.storage_key(),
            stuck_secs: action.stuck_secs,
            message: &detail,
        });

        if landed {
            self.push_event(EngineEvent::new(
                LogLevel::Success,
                Some(session.id.clone()),
                i18n.tf(
                    "log.resume_sent",
                    &[
                        ("mode", mode),
                        ("count", &commit.resume_count.to_string()),
                        (
                            "detail",
                            &format!("{} · {}", i18n.t(outcome.i18n_key()), detail),
                        ),
                    ],
                ),
            ))
            .await;

            if latest_config.webhook.enabled {
                WebhookNotifier::new(
                    latest_config.webhook.clone(),
                    Lang::from_code(&latest_config.language),
                )
                .notify_resume(&session.agent_name, &session.id, &detail)
                .await;
            }
            if let Some(notifier) = self.notifier.get() {
                notifier.notify_resumed(
                    &latest_config.notification,
                    &latest_config.language,
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
            self.escalate_resume_failure(
                &latest_config,
                &i18n,
                session,
                commit.resume_failures,
                &detail,
            )
            .await;
        }
    }

    /// 动作排队后重新确认它仍然成立。
    async fn auto_action_is_current(&self, action: &ResumeAction, config: &AppConfig) -> bool {
        if !same_lifecycle(
            action.lifecycle_epoch,
            self.lifecycle_epoch.load(Ordering::SeqCst),
        ) {
            return false;
        }

        // 有记录文件时，必须还是检测那一版；任何增长都说明旧判定已经过期。
        if action.session.session_file.is_some()
            && activity_fingerprint(&action.session) != action.observed_activity
        {
            return false;
        }

        let state = self.state.lock().await;
        auto_resume_state_allows(&state, &action.session.id, config)
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
    async fn commit_resume_outcome(
        &self,
        session_id: &str,
        outcome: ResumeOutcome,
    ) -> ResumeCommit {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut state = self.state.lock().await;
        if outcome.counts_as_nudge() {
            state.status.total_resumes = state.status.total_resumes.saturating_add(1);
        }
        let Some(session) = state.sessions.iter_mut().find(|s| s.id == session_id) else {
            return ResumeCommit::default();
        };
        apply_resume_outcome(session, outcome, now);
        ResumeCommit {
            resume_count: session.resume_count,
            resume_failures: session.resume_failures,
        }
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

    /// 手动续跑也走引擎的全局投递协调器。
    ///
    /// 旧入口在 Tauri 命令里直接 new 一个 `Resumer`，因此能和后台自动续跑同时抢
    /// 剪贴板、抢前台窗口；双击按钮也会排出两次动作。现在同会话先占位，所有会话
    /// 再共用串行投递锁，成功后统一落库、更新冷却与累计计数。
    pub async fn manual_resume(
        &self,
        session_id: &str,
        use_goal_prompt: bool,
    ) -> Result<String, String> {
        let config = self.config();
        let i18n = I18n::from_code(&config.language);
        let Some(lease) = self.resume_registry.try_acquire(session_id) else {
            return Err(i18n.t("err.resume_in_progress").to_string());
        };

        let result = self
            .manual_resume_inner(session_id, use_goal_prompt, config, &i18n)
            .await;
        // Drop 同时释放会话并唤醒 worker，自动队列里的最新后继可立即重验。
        drop(lease);
        result
    }

    async fn manual_resume_inner(
        &self,
        session_id: &str,
        use_goal_prompt: bool,
        config: AppConfig,
        i18n: &I18n,
    ) -> Result<String, String> {
        let delivery_phase = PhaseCounter::enter(&self.resume_delivery_pending);
        let (session, resumer, delivery) = {
            let _delivery_guard = self.delivery_lock.lock().await;

            // 排队期间会话可能退出，所以拿到全局投递锁之后才取最终快照。
            let session = {
                let state = self.state.lock().await;
                state
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .cloned()
                    .ok_or_else(|| i18n.t("err.session_not_found").to_string())?
            };

            let resumer = Resumer::new(config);
            let delivery = resumer.deliver(&session, use_goal_prompt).await;
            (session, resumer, delivery)
        };
        drop(delivery_phase);

        // 核验只读这一条会话自己的记录文件，不能继续占着桌面级投递锁。
        let (outcome, detail) = match delivery {
            Ok(delivery) => {
                let verify_phase = PhaseCounter::enter(&self.resume_verifying);
                let result = resumer.verify_delivery(&session, delivery).await;
                drop(verify_phase);
                result
            }
            Err(error) => (ResumeOutcome::Failed, error),
        };
        let stuck_secs = session.stuck_secs();
        let ok = outcome.counts_as_nudge();
        let text = format!("{} · {}", i18n.t(outcome.i18n_key()), detail);

        let prompt_type = if use_goal_prompt { "goal" } else { "generic" };
        self.storage.record_resume(crate::storage::ResumeEvent {
            session_id: &session.id,
            agent_name: &session.agent_name,
            working_dir: &session.working_dir,
            prompt_type,
            success: ok,
            outcome: outcome.storage_key(),
            stuck_secs,
            message: &detail,
        });
        self.commit_manual_resume_outcome(&session.id, outcome)
            .await;

        self.push_event(EngineEvent::new(
            if ok {
                LogLevel::Success
            } else {
                LogLevel::Error
            },
            Some(session.id),
            i18n.tf("log.resume_manual", &[("detail", &text)]),
        ))
        .await;

        if ok {
            Ok(text)
        } else {
            Err(text)
        }
    }

    async fn commit_manual_resume_outcome(&self, session_id: &str, outcome: ResumeOutcome) {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let mut state = self.state.lock().await;
        if outcome.counts_as_nudge() {
            state.status.total_resumes = state.status.total_resumes.saturating_add(1);
        }
        let Some(session) = state.sessions.iter_mut().find(|s| s.id == session_id) else {
            return;
        };
        apply_manual_resume_outcome(session, outcome, now);
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

/// 本轮能不能给会话历史收尾
///
/// 收尾的推理是「表里有、本轮没发现，所以它没了」。这句话只有在**真的去看过**
/// 的前提下才成立：适配器全关的时候一个都发现不了，照着收尾会把用户正在跑的
/// 会话全标成已结束。
///
/// 抽成自由函数是为了能测——`MonitorEngine` 要 `ConfigManager::new()`，
/// 那玩意儿读真实配置目录，单元测试里碰不得。
/// 生命周期代数必须完全一致；“停止 → 立即启动”也不能让旧队列重新合法。
fn same_lifecycle(action_epoch: u64, current_epoch: u64) -> bool {
    action_epoch == current_epoch
}

/// 自动动作拿到全局投递锁后仍需满足的运行态策略。
///
/// 单独抽出来让停止语义可直接测试：`scan_now` 可以在停止状态刷新检测，但绝不能
/// 顺手触发自动输入；停止后再启动时，生命周期代数还会在外层挡住旧动作复活。
fn auto_resume_state_allows(state: &MonitorState, session_id: &str, config: &AppConfig) -> bool {
    if !state.running || !config.auto_resume_enabled {
        return false;
    }
    let Some(current) = state.sessions.iter().find(|s| s.id == session_id) else {
        return false;
    };
    current.status == SessionStatus::Interrupted
        && current.resume_tactic == ResumeTactic::Nudge
        && has_nudges_left(current, config.max_resume_count)
        && check_cooldown(
            current,
            effective_cooldown(config.resume_cooldown_secs, current.resume_failures),
        )
}

/// 将扫描期间可能由投递任务更新的运行态重新合并回来。
///
/// 检测和投递现在允许重叠：否则每个会话最长 6 秒的落地核验会把整个扫描入口锁住。
/// 代价是扫描开始时复制的旧计数不能再整份覆盖当前状态。累计次数、失败退避和冷却
/// 时间始终以当前状态为准；只有本轮明确观察到会话在推进，才清空自动续跑连击。
fn merge_resume_runtime(
    scanned: &mut [AgentSession],
    current: &[AgentSession],
    running_session_ids: &HashSet<String>,
) {
    let current_by_id: HashMap<&str, &AgentSession> = current
        .iter()
        .map(|session| (session.id.as_str(), session))
        .collect();
    for session in scanned {
        let Some(latest) = current_by_id.get(session.id.as_str()) else {
            continue;
        };
        session.resume_count = latest.resume_count;
        session.resume_failures = latest.resume_failures;
        session.last_resume_at = latest.last_resume_at.clone();
        session.resume_streak = if running_session_ids.contains(&session.id) {
            0
        } else {
            latest.resume_streak
        };
    }
}

fn should_reconcile_history(config: &AppConfig) -> bool {
    !config.enabled_adapters.is_empty()
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

/// 手动续跑成功要进入累计统计，但不消耗自动续跑的连击额度。
///
/// 用户主动点一次，说明他明确希望这次动作发生；`max_resume_count` 管的是守护器
/// 自动空转，不该拿用户动作去挤占。但界面上的“已续跑 N 次”和总统计必须包含它，
/// 否则记录中心有一行、会话卡片和总数却都没变，三处事实互相矛盾。
fn apply_manual_resume_outcome(session: &mut AgentSession, outcome: ResumeOutcome, now: String) {
    session.last_resume_at = Some(now);
    if outcome.is_failure() {
        session.resume_failures = session.resume_failures.saturating_add(1);
    } else {
        session.resume_count = session.resume_count.saturating_add(1);
        session.resume_failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_lease_is_exclusive_and_released_by_drop() {
        let registry = Arc::new(ResumeRegistry::default());
        {
            let _lease = registry.try_acquire("session-1").expect("首次应拿到租约");
            assert!(registry.is_active("session-1"));
            assert!(
                registry.try_acquire("session-1").is_none(),
                "同一会话不能同时存在两个续跑意图"
            );
            assert!(
                registry.try_acquire("session-2").is_some(),
                "不同会话可以各自占位，真实投递再由全局锁串行"
            );
        }
        assert!(!registry.is_active("session-1"), "离开作用域必须自动释放");
        assert!(registry.try_acquire("session-1").is_some());
    }

    #[test]
    fn phase_counter_returns_to_zero_on_every_scope_exit() {
        let counter = AtomicUsize::new(0);
        {
            let _outer = PhaseCounter::enter(&counter);
            assert_eq!(counter.load(Ordering::SeqCst), 1);
            {
                let _inner = PhaseCounter::enter(&counter);
                assert_eq!(counter.load(Ordering::SeqCst), 2);
            }
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn pipeline_snapshot_separates_pending_delivery_from_verification() {
        let mut status = EngineStatus::default();
        merge_resume_pipeline_status(&mut status, 2, 3, 4);
        assert_eq!(status.resume_pending, 5);
        assert_eq!(status.resume_verifying, 4);

        merge_resume_pipeline_status(&mut status, usize::MAX, 1, 0);
        assert_eq!(status.resume_pending, usize::MAX, "快照计数不能溢出回零");
        assert_eq!(status.resume_verifying, 0);
    }

    #[test]
    fn resume_queue_coalesces_each_session_to_the_latest_snapshot() {
        fn action(id: &str, epoch: u64) -> ResumeAction {
            let mut session = session_with("/tmp", None);
            session.id = id.to_string();
            ResumeAction {
                session,
                use_goal_prompt: false,
                observed_activity: None,
                lifecycle_epoch: epoch,
                stuck_secs: None,
            }
        }

        let mut queue = ResumeQueue::default();
        queue.upsert(action("session-1", 1));
        queue.upsert(action("session-2", 1));

        // 即使 session-1 已有动作在核验，新扫描也应保留一条最新后继，而不是丢掉
        // 更鲜的证据；同时它不能挡住排在后面的 session-2。
        let registry = Arc::new(ResumeRegistry::default());
        let lease = registry.try_acquire("session-1").expect("模拟在途核验");
        queue.upsert(action("session-1", 2));

        assert_eq!(queue.len(), 2, "同会话只保留一条，队列不会随扫描次数膨胀");
        let (first, _first_lease) = queue
            .pop_ready(|id| registry.try_acquire(id))
            .expect("应跳过忙会话");
        assert_eq!(first.session.id, "session-2");
        assert_eq!(queue.len(), 1, "忙会话的最新动作必须留在队列");

        drop(lease);
        let (next, _next_lease) = queue
            .pop_ready(|id| registry.try_acquire(id))
            .expect("租约释放后应立即接上最新动作");
        assert_eq!(next.session.id, "session-1");
        assert_eq!(next.lifecycle_epoch, 2, "应消费最新快照而不是旧动作");
        assert!(queue.pop_ready(|id| registry.try_acquire(id)).is_none());
    }

    #[test]
    fn resume_queue_keeps_all_actions_when_every_session_is_busy() {
        fn action(id: &str) -> ResumeAction {
            let mut session = session_with("/tmp", None);
            session.id = id.to_string();
            ResumeAction {
                session,
                use_goal_prompt: false,
                observed_activity: None,
                lifecycle_epoch: 1,
                stuck_secs: None,
            }
        }

        let mut queue = ResumeQueue::default();
        queue.upsert(action("session-1"));
        queue.upsert(action("session-2"));
        let registry = Arc::new(ResumeRegistry::default());
        let _lease_1 = registry.try_acquire("session-1").expect("占住会话 1");
        let _lease_2 = registry.try_acquire("session-2").expect("占住会话 2");

        assert!(queue.pop_ready(|id| registry.try_acquire(id)).is_none());
        assert_eq!(queue.len(), 2, "检查一圈后不能丢动作或复制动作");

        // 再检查一圈也应稳定返回；这钉住“所有会话忙时原地旋转/膨胀”的退化。
        assert!(queue.pop_ready(|id| registry.try_acquire(id)).is_none());
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn stopped_monitor_rejects_queued_auto_resume() {
        let config = AppConfig {
            auto_resume_enabled: true,
            max_resume_count: 3,
            resume_cooldown_secs: 0,
            ..Default::default()
        };

        let mut session = session_with("/tmp", None);
        session.id = "session-1".to_string();
        session.status = SessionStatus::Interrupted;
        session.resume_tactic = ResumeTactic::Nudge;

        let mut state = MonitorState {
            running: true,
            sessions: vec![session],
            ..Default::default()
        };
        assert!(auto_resume_state_allows(&state, "session-1", &config));

        state.running = false;
        assert!(
            !auto_resume_state_allows(&state, "session-1", &config),
            "停止守护后，已经排队但尚未投递的自动动作必须失效"
        );
    }

    #[test]
    fn stop_then_restart_does_not_revive_an_old_action() {
        let action_epoch = 7;
        assert!(same_lifecycle(action_epoch, 7));
        assert!(
            !same_lifecycle(action_epoch, 9),
            "停止和重新启动各推进一次代数，旧动作不能因 running 再次为 true 而复活"
        );
    }

    #[test]
    fn overlapping_scan_preserves_a_completed_resume_commit() {
        let mut stale_scan = session_with("/tmp", None);
        stale_scan.id = "session-1".to_string();
        stale_scan.resume_count = 1;
        stale_scan.resume_streak = 1;

        let mut current = stale_scan.clone();
        current.resume_count = 7;
        current.resume_streak = 3;
        current.resume_failures = 2;
        current.last_resume_at = Some("2026-08-07 12:00:00".to_string());

        let mut scanned = vec![stale_scan];
        merge_resume_runtime(&mut scanned, &[current], &HashSet::new());
        assert_eq!(scanned[0].resume_count, 7);
        assert_eq!(scanned[0].resume_streak, 3);
        assert_eq!(scanned[0].resume_failures, 2);
        assert_eq!(
            scanned[0].last_resume_at.as_deref(),
            Some("2026-08-07 12:00:00")
        );
    }

    #[test]
    fn observed_progress_resets_only_the_auto_streak() {
        let mut scanned_session = session_with("/tmp", None);
        scanned_session.id = "session-1".to_string();
        let mut current = scanned_session.clone();
        current.resume_count = 7;
        current.resume_streak = 3;
        current.resume_failures = 2;

        let mut scanned = vec![scanned_session];
        merge_resume_runtime(
            &mut scanned,
            &[current],
            &HashSet::from(["session-1".to_string()]),
        );
        assert_eq!(scanned[0].resume_count, 7);
        assert_eq!(scanned[0].resume_streak, 0);
        assert_eq!(scanned[0].resume_failures, 2);
    }

    /// 攒够 500 条之后还认得出新事件
    ///
    /// 这条是那个 bug 的正脸。原来的推送泵拿 `events.len()` 当游标，而事件环封顶在
    /// [`EVENT_RING_CAP`]——长度到 500 就不再变了，于是「长度没变 = 没有新事件」
    /// 在那之后**永远成立**：后端继续记日志，界面上再也不出现新的一行，
    /// 不报错、不留痕，看起来就像「最近没发生什么事」。
    #[test]
    fn a_saturated_ring_still_reports_new_events() {
        let cap = EVENT_RING_CAP;
        // 环已经满了、游标停在 500：跑了一阵子之后的稳定状态
        // 又来一条 → 推过的总数 501，环长度仍然是 500
        assert_eq!(
            fresh_tail(cap as u64 + 1, cap as u64, cap),
            1,
            "环满之后新事件必须还能推出去"
        );
    }

    /// 长度不变但计数在涨——上面那条的一般形式
    #[test]
    fn a_pinned_length_does_not_hide_a_burst() {
        let cap = EVENT_RING_CAP;
        // 一个轮询周期里来了 7 条，环被裁掉 7 条，长度还是 500
        assert_eq!(fresh_tail(cap as u64 + 7, cap as u64, cap), 7);
    }

    /// 没有新事件时不推空包
    #[test]
    fn nothing_new_means_nothing_emitted() {
        assert_eq!(fresh_tail(0, 0, 0), 0);
        assert_eq!(fresh_tail(42, 42, 42), 0);
    }

    /// 前端落后太多时按环里剩下的推，不越界
    ///
    /// 泵每 800 毫秒醒一次，正常追得上。但主线程卡住、机器休眠再唤醒之后，
    /// 累计数可以比环里留着的多出好几千。拿差值当切片起点会直接 panic；
    /// 封顶到环长度只丢掉最老的那些——日志面板本来就只看最近的。
    #[test]
    fn falling_far_behind_clamps_instead_of_panicking() {
        assert_eq!(fresh_tail(10_000, 0, EVENT_RING_CAP), EVENT_RING_CAP);
        // 环还没满时同样不能超过环长
        assert_eq!(fresh_tail(10_000, 0, 3), 3);
    }

    /// 计数万一回退，这一轮就别推
    ///
    /// 正常跑不出来（`events_pushed` 只增）。但 `u64` 减法遇到负数在 debug 下
    /// 会 panic、在 release 下会绕成天文数字然后被拿去切片，两种都比
    /// 「这一轮不推」糟糕得多。
    #[test]
    fn a_backwards_counter_emits_nothing() {
        assert_eq!(fresh_tail(3, 9, EVENT_RING_CAP), 0);
    }

    /// 冷启动第一轮：环里有几条就推几条
    #[test]
    fn a_cold_start_emits_what_is_already_there() {
        assert_eq!(fresh_tail(4, 0, 4), 4);
    }

    /// 事件环裁剪之后，计数不能跟着回落
    ///
    /// 这条守的是 [`MonitorState::push_event`] 里两个计数的分工：一个跟「推过多少」
    /// 走，一个跟「留着多少」走。它们一旦被写成同一个数，上面那个静默停更立刻复活。
    #[test]
    fn the_counter_keeps_climbing_after_the_ring_trims() {
        let mut state = MonitorState::default();
        let overflow = 25;
        for i in 0..(EVENT_RING_CAP + overflow) {
            state.push_event(EngineEvent::new(LogLevel::Info, None, format!("第 {i} 条")));
        }
        assert_eq!(state.events.len(), EVENT_RING_CAP, "环该封顶");
        assert_eq!(
            state.events_pushed,
            (EVENT_RING_CAP + overflow) as u64,
            "计数该记全部推过的条数，不受裁剪影响"
        );
        // 最新的还在，最老的被裁掉了
        let last = format!("第 {} 条", EVENT_RING_CAP + overflow - 1);
        assert_eq!(state.events.last().unwrap().message, last);
        assert!(!state.events.iter().any(|e| e.message == "第 0 条"));
    }

    /// 满环 + 新事件，串起来跑一遍
    ///
    /// 前两条分别测了「计数对不对」和「算术对不对」，这条把它们接上：
    /// 环满之后再推一条，泵该恰好取到那一条，而且取到的就是最新那条。
    #[test]
    fn a_full_ring_then_one_more_emits_exactly_that_one() {
        let mut state = MonitorState::default();
        for i in 0..EVENT_RING_CAP {
            state.push_event(EngineEvent::new(LogLevel::Info, None, format!("旧 {i}")));
        }
        // 泵已经追平
        let sent = state.events_pushed;
        state.push_event(EngineEvent::new(LogLevel::Warn, None, "新来的"));

        let fresh = fresh_tail(state.events_pushed, sent, state.events.len());
        assert_eq!(fresh, 1);
        let emitted = &state.events[state.events.len() - fresh..];
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].message, "新来的");
    }

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
    fn manual_delivery_counts_in_history_but_not_the_auto_streak() {
        let mut s = session_with("/tmp", None);
        s.resume_streak = 2;
        apply_manual_resume_outcome(&mut s, ResumeOutcome::Landed, stamp());
        assert_eq!(s.resume_count, 1, "手动成功也是真实的一次续跑");
        assert_eq!(s.resume_streak, 2, "用户点的动作不能消耗自动续跑的连击额度");
        assert_eq!(s.resume_failures, 0);
        assert!(s.last_resume_at.is_some(), "仍要启动冷却，防止自动立刻补敲");
    }

    #[test]
    fn failed_manual_delivery_only_advances_channel_failures() {
        let mut s = session_with("/tmp", None);
        s.resume_count = 4;
        s.resume_streak = 2;
        apply_manual_resume_outcome(&mut s, ResumeOutcome::Silent, stamp());
        assert_eq!(s.resume_count, 4);
        assert_eq!(s.resume_streak, 2);
        assert_eq!(s.resume_failures, 1);
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

    /// 适配器全关时不能收尾：那时候「没发现会话」说的是「没去看」
    #[test]
    fn history_is_not_reconciled_when_nothing_is_being_watched() {
        let mut config = AppConfig::default();
        assert!(
            should_reconcile_history(&config),
            "默认配置是开着适配器的，本轮该收尾"
        );

        config.enabled_adapters.clear();
        assert!(
            !should_reconcile_history(&config),
            "一个适配器都没开的时候收尾，会把活着的会话标成已结束"
        );
    }
}
