pub mod claude_code;
pub mod codex;
pub mod opencode;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// 进程快照 — 一次扫描共享，避免每个适配器重复枚举进程
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub pid: u32,
    /// 小写进程名（Windows 含 .exe 后缀）
    pub name: String,
    /// 完整命令行
    pub cmd: String,
    /// 工作目录
    pub cwd: String,
}

/// 获取当前所有进程的轻量快照（仅 name/cmd/cwd，不刷新 CPU/内存/磁盘）
pub fn take_process_snapshot() -> Vec<ProcessSnapshot> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );

    system
        .processes()
        .iter()
        .map(|(pid, process)| ProcessSnapshot {
            pid: pid.as_u32(),
            name: process.name().to_string_lossy().to_lowercase(),
            cmd: process
                .cmd()
                .iter()
                .map(|c| c.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" "),
            cwd: process
                .cwd()
                .map(|c| c.to_string_lossy().to_string())
                .unwrap_or_default(),
        })
        .collect()
}

/// 将路径转为 glob 安全的模式串
///
/// glob crate 将 `\` 视为转义字符，Windows 反斜杠路径会导致匹配永远失败，
/// 因此统一转为正斜杠（Windows API 同样接受正斜杠）。
pub fn to_glob_pattern(path: &std::path::Path, suffix: &str) -> String {
    format!("{}{}", path.to_string_lossy().replace('\\', "/"), suffix)
}

/// Agent 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    /// 会话唯一 ID
    pub id: String,
    /// Agent 类型标识
    pub adapter_id: String,
    /// Agent 显示名称
    pub agent_name: String,
    /// 关联的进程 PID
    pub pid: u32,
    /// 进程命令行
    pub command: String,
    /// 工作目录
    pub working_dir: String,
    /// 会话文件路径（如有）
    pub session_file: Option<String>,
    /// 会话发现时间
    pub discovered_at: String,
    /// 最后活动时间
    pub last_activity: String,
    /// 会话状态
    pub status: SessionStatus,
    /// 已续跑次数（累计，只增不减）
    ///
    /// **只统计真的把字敲进去的那些次。** 这个语义很要紧：一旦把失败也算进来，
    /// 「敲不进去」就会被当成「已经敲够了」。见 `monitor::commit_resume_outcome`。
    ///
    /// 这个数是给人看的（界面上「已续跑 N 次」、会话历史里的累计值），
    /// **不参与任何判定**。要限制「还该不该继续催」，用 [`Self::resume_streak`]。
    pub resume_count: u32,
    /// 最后一次续跑时间
    pub last_resume_at: Option<String>,
    /// 连续催了几次都没见它动（进程内状态，不落库）
    ///
    /// `max_resume_count` 那道上限真正想拦的是「对着一个不响应的会话没完没了地催」，
    /// 而不是「一个会话一辈子只准被催 5 次」。用累计次数去撞上限，
    /// 结果就是一个跑了一整天、真的停顿过六次的会话，从第六次起再没人管——
    /// 「它其实没干完活，每次都要我去发继续」就是这么来的。
    ///
    /// 所以判定改用这个数：**一旦看见会话自己在干活（`Verdict::Running`）就清零**，
    /// 额度还回去。只有连着催 N 次都毫无反应，才认定催也没用、闭嘴等人。
    #[serde(default)]
    pub resume_streak: u32,
    /// 连续投递失败次数（进程内状态，不落库）
    ///
    /// 跟上面两个都分开是因为三者驱动完全不同的行为：累计次数只用来显示；
    /// 「催了没反应」的连击数决定还该不该继续催；而失败次数决定
    /// 「还要不要接着用这条通道，以及什么时候该改成大声告诉用户」。
    /// 投递成功一次即清零。
    #[serde(default)]
    pub resume_failures: u32,

    // ── 以下字段由监控循环每轮回填，适配器发现会话时留空 ──
    /// 注意力级别（v1.1）：这个会话现在要不要叫人
    #[serde(default)]
    pub attention: crate::detector::AttentionLevel,
    /// 触发该注意力级别的依据
    #[serde(default)]
    pub attention_detail: Option<String>,
    /// 检测侧结构性证据；只供界面解释判定，不参与动作层重算
    #[serde(default)]
    pub detection_evidence: Option<crate::detector::DetectionEvidence>,
    /// 它为什么停下来（v1.6）
    ///
    /// 跟 `attention` 放在一起是因为两者是同一件事的两面：级别说「要不要叫人」，
    /// 原因说「叫来了能干什么」。界面上必须能看出后者——应用有三种情况会
    /// **故意不催**（进程没了、撞限流、它在问一个具体问题），如果界面上只写着
    /// 「已中断」，用户看到的就是守护神漏了一次，而不是它做了一个正确的决定。
    #[serde(default)]
    pub interrupt_reason: crate::detector::InterruptReason,
    /// 针对上面那个原因，这一轮打算怎么办（v1.6）
    ///
    /// 由判定层算好再发上来，**不让界面照着原因表再推一遍**。原因和手段之间
    /// 不是一一对应的显然关系（`RuntimeError` 要敲、`RateLimited` 不敲），
    /// 前端抄一份判断就等于同一条策略存了两份，下次加原因时漏改一处
    /// 编译器一句话都不会说，用户看到的却是「该说的时候没说」。
    #[serde(default)]
    pub resume_tactic: crate::detector::ResumeTactic,
    /// 所在终端的 TTY（如 `/dev/ttys003`）
    ///
    /// 痛点 #2「多会话混乱」的关键：同一个目录下开两个 Claude Code 时，
    /// 只有 TTY + 终端应用能让人一眼认出「是哪个标签页在等我」。
    #[serde(default)]
    pub tty: Option<String>,
    /// 所属终端应用（iTerm2 / Terminal / WezTerm …）
    #[serde(default)]
    pub terminal_app: Option<String>,
    /// 本会话的 token 用量与成本（v1.2，仅 Claude Code 有数据）
    #[serde(default)]
    pub usage: Option<crate::cost::UsageSnapshot>,
}

impl AgentSession {
    /// 会话记录上一次变动到现在隔了多久（秒）；说不准就返回 `None`
    ///
    /// 只有**有记录文件**的会话才回答得了。`last_activity` 对 Claude Code 来说
    /// 是那个 jsonl 的 mtime，问它「多久没动了」是准的；可 Codex / OpenCode
    /// 这类没有可读记录的适配器，每轮发现会话时都把它填成「现在」——
    /// 拿它算出来的永远接近 0，那不是「刚刚才停」，那是「不知道」。
    ///
    /// 所以不知道就说不知道，不要用 0 冒充：这个数会进库、会被平均，
    /// 掺一批假的 0 进去，「平均卡了多久」就会被稀释成一个谁都不该相信的数。
    pub fn stuck_secs(&self) -> Option<i64> {
        self.session_file.as_ref()?;
        let last =
            chrono::NaiveDateTime::parse_from_str(&self.last_activity, "%Y-%m-%d %H:%M:%S").ok()?;
        let secs = (chrono::Local::now().naive_local() - last).num_seconds();
        // 负数意味着 mtime 在未来（改过系统时间、挂载的网络盘时钟偏了）。
        // 这种数说不出任何事情，当成不知道。
        (secs >= 0).then_some(secs)
    }

    /// 这个会话在历史表里的主键
    ///
    /// 优先用会话文件路径：进程重启换了 PID，但只要还是同一份记录，
    /// 时间线就该接上同一条，而不是多出一行。
    ///
    /// 没有记录文件的适配器（Codex / OpenCode）退回 `adapter-pid-工作目录`。
    ///
    /// **这里刻意不含 `discovered_at`。** 含过，结果是这样的：`discovered_at`
    /// 只在进程内靠上一轮的会话表续着，AgentPulse 自己一重启那张表就空了，
    /// 同一个会话下一轮被填上「现在」，于是生出一个全新的键、库里多一行。
    /// 真实库里因此出现过同一个会话摊成 16 行的情况——历史页看着像噪音，
    /// 是因为它那时列的其实是「重启记录」，不是会话。
    ///
    /// 代价是 PID 被系统回收、且新进程恰好在同一个工作目录跑同一种 agent 时，
    /// 两个会话会并成一行。那是小概率且只丢一行历史；而含时间戳的版本是
    /// **每次重启都必然**多出一堆重复行。
    ///
    /// **必须跟收尾那一步用同一个定义**，否则「本轮还活着的键」跟表里的键对不上，
    /// [`crate::storage::Storage::close_missing_sessions`] 会把活着的会话
    /// 判成已结束——一个字面量抄两遍就够犯这个错，所以收成一个方法。
    pub fn history_key(&self) -> String {
        self.session_file
            .clone()
            .unwrap_or_else(|| format!("{}-{}-{}", self.adapter_id, self.pid, self.working_dir))
    }
}

impl Default for AgentSession {
    fn default() -> Self {
        Self {
            id: String::new(),
            adapter_id: String::new(),
            agent_name: String::new(),
            pid: 0,
            command: String::new(),
            working_dir: String::new(),
            session_file: None,
            discovered_at: String::new(),
            last_activity: String::new(),
            status: SessionStatus::Active,
            resume_count: 0,
            last_resume_at: None,
            resume_streak: 0,
            resume_failures: 0,
            attention: crate::detector::AttentionLevel::None,
            attention_detail: None,
            detection_evidence: None,
            interrupt_reason: crate::detector::InterruptReason::None,
            resume_tactic: crate::detector::ResumeTactic::Nudge,
            tty: None,
            terminal_app: None,
            usage: None,
        }
    }
}

/// 会话状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// 活跃运行中
    Active,
    /// 疑似中断（检测到中断信号但未确认）
    Suspended,
    /// 已确认中断，等待续跑
    Interrupted,
    /// 已完成
    Completed,
    /// 进程已退出
    Exited,
}

impl SessionStatus {
    /// 稳定字符串键
    ///
    /// 状态名是要给人看的，但**翻译由渲染方负责**：前端有自己的字典，
    /// 落库和日志只存这个不随语言变的键，免得数据库里躺着一堆中文。
    pub fn key(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Suspended => "suspended",
            SessionStatus::Interrupted => "interrupted",
            SessionStatus::Completed => "completed",
            SessionStatus::Exited => "exited",
        }
    }
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// 会话回合在记录里表现出的结构性状态
///
/// 存在的理由：**「多久没写文件」根本区分不了「在拼命干活」和「在等人回话」**。
/// 压缩上下文、跑一条几分钟的构建命令、或者纯思考很久，记录文件都不会落盘；
/// 只看 mtime 就会把这些全判成卡住，然后往一个正在干活的会话里敲字。
///
/// 回合有没有收尾是结构上看得见的：工具调用发出去还没等到结果、
/// 或者最后一条是真人刚敲的提示词，都说明 agent 这会儿正忙。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    /// 认不出来（记录里没有可判断的结构，或适配器不支持）
    #[default]
    Unknown,
    /// 工具调用已发出但结果还没回来 —— 正在跑命令，别打扰
    ToolRunning,
    /// 回合尚未收尾（刚收到提示词、刚拿到工具结果、正在思考）
    Busy,
    /// 回合已收尾，确实停在等人
    AwaitingUser,
}

impl TurnState {
    /// 这会儿 agent 是不是正忙
    ///
    /// `Unknown` 不算忙：认不出来时保留原有的超时兜底，
    /// 否则不写记录文件的 agent（Codex / OpenCode）就彻底测不出中断了。
    pub fn is_busy(&self) -> bool {
        matches!(self, TurnState::ToolRunning | TurnState::Busy)
    }
}

/// Agent 适配器 trait — 定义如何发现和监控特定 AI Agent
pub trait AgentAdapter: Send + Sync {
    /// 适配器唯一标识
    fn id(&self) -> &str;
    /// 显示名称
    fn name(&self) -> &str;
    /// 从进程快照中发现该类型 agent 会话
    fn discover_sessions(&self, processes: &[ProcessSnapshot]) -> Vec<AgentSession>;
    /// 获取会话文件路径（用于文件监听）
    fn session_files(&self) -> Vec<PathBuf>;
    /// 读取会话最近的输出内容（用于关键词匹配）
    fn recent_output(&self, session: &AgentSession) -> Option<String>;
    /// 读取**记录里明确标成故障的行**（API 错误 / 系统错误）
    ///
    /// 为什么要跟 [`Self::recent_output`] 分开：agent **谈论**一个错误和它
    /// **遇到**一个错误，在散文里长得一模一样。实测就栽在这上面——一句
    /// 「不再撞上错误关键词 500」被判成会话出了 500 错误，词边界也拦不住，
    /// 因为那确实是个独立的 `500`。
    ///
    /// 所以「出错 / 限流」这两级注意力只认这个通道：只有 agent 自己的运行时
    /// 写下「这是一个错误」的行才算数，散文一律不算。默认 `None` —— 读不出
    /// 结构的适配器就不报错，宁可漏报也不要在用户屏幕上弹一个假警报。
    fn error_output(&self, _session: &AgentSession) -> Option<String> {
        None
    }
    /// 判断回合有没有收尾
    ///
    /// 默认 `Unknown`：读不出记录结构的适配器沿用旧的超时逻辑。
    fn turn_state(&self, _session: &AgentSession) -> TurnState {
        TurnState::Unknown
    }
}

/// 获取所有已注册的适配器
pub fn all_adapters() -> Vec<Box<dyn AgentAdapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter::new()),
        Box::new(codex::CodexAdapter),
        Box::new(opencode::OpenCodeAdapter),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个会话：`file` 决定它有没有可读记录
    fn session(last_activity: &str, file: Option<&str>) -> AgentSession {
        AgentSession {
            last_activity: last_activity.to_string(),
            session_file: file.map(str::to_string),
            ..Default::default()
        }
    }

    fn ago(secs: i64) -> String {
        (chrono::Local::now() - chrono::Duration::seconds(secs))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }

    #[test]
    fn stuck_secs_reads_the_transcript_mtime() {
        let s = session(&ago(600), Some("/tmp/x.jsonl"));
        let got = s.stuck_secs().expect("有记录文件、时间也能解析");
        // 留 5 秒余量：测试跑起来本身要花时间
        assert!((595..=605).contains(&got), "该在 600 秒上下，实际 {got}");
    }

    /// 没有记录文件就说不知道，**不能返回 0**
    ///
    /// Codex / OpenCode 每轮发现会话时都把 `last_activity` 填成「现在」，
    /// 拿它算出来的永远接近 0。那不是「刚刚才停」，那是「没这个数」——
    /// 混进统计的平均值里会把「平均卡了 20 分钟」稀释成几分钟。
    #[test]
    fn no_transcript_means_no_answer() {
        assert_eq!(session(&ago(600), None).stuck_secs(), None);
    }

    #[test]
    fn an_unparseable_timestamp_means_no_answer() {
        assert_eq!(session("", Some("/tmp/x.jsonl")).stuck_secs(), None);
        assert_eq!(
            session("2026-08-04T12:00:00Z", Some("/tmp/x.jsonl")).stuck_secs(),
            None,
            "ISO 8601 不是这里的格式，别当成能解析"
        );
    }

    /// mtime 在未来（改过系统时间、网络盘时钟偏了）→ 不知道
    ///
    /// 不返回负数是因为这个值会进库、会被求平均。一个 -3600 能把一整期的
    /// 平均卡住时长拉成负数，而「平均卡了负一小时」没有任何读法。
    #[test]
    fn a_future_timestamp_means_no_answer() {
        assert_eq!(
            session(&ago(-3600), Some("/tmp/x.jsonl")).stuck_secs(),
            None
        );
    }

    /// 刚动过的会话给得出一个数（≈0），而不是「不知道」
    ///
    /// 这条要守的是 `Some` 和 `None` 的区别：0 秒是一个真实的答案，
    /// 跟「没这个数」不是一回事（见上面 `no_transcript_means_no_answer`）。
    ///
    /// 所以断言留了 1 秒余量，**不能写成 `== Some(0)`**：`ago(0)` 把「现在」
    /// 格式化成秒级字符串，`stuck_secs()` 又重新取一次 `Local::now()`，
    /// 两次调用之间只要跨过一个整秒，差值就是 1。写死 0 的版本在本机能过
    /// 几十次，然后在 CI 上随机红一次——Windows runner 上就是这么红的，
    /// `left: Some(1), right: Some(0)`。
    #[test]
    fn a_just_touched_session_is_a_number_not_unknown() {
        let got = session(&ago(0), Some("/tmp/x.jsonl")).stuck_secs();
        assert!(
            matches!(got, Some(0..=1)),
            "刚动过的会话该给出 ≈0 秒这个真实答案，实际 {got:?}"
        );
    }

    #[test]
    fn the_history_key_is_the_transcript_path_when_there_is_one() {
        let s = session(&ago(0), Some("/tmp/x.jsonl"));
        assert_eq!(s.history_key(), "/tmp/x.jsonl");
    }

    /// 同一份记录跨进程重启要接上同一条时间线
    #[test]
    fn the_same_transcript_keeps_the_same_key_across_pids() {
        let mut a = session(&ago(0), Some("/tmp/x.jsonl"));
        a.pid = 111;
        let mut b = session(&ago(0), Some("/tmp/x.jsonl"));
        b.pid = 222;
        assert_eq!(a.history_key(), b.history_key());
    }

    /// 没有记录文件时退回 adapter-pid-工作目录，且两个会话不能撞成一个
    #[test]
    fn sessions_without_a_transcript_still_get_distinct_keys() {
        let mut a = session(&ago(0), None);
        a.adapter_id = "codex".into();
        a.pid = 111;
        a.working_dir = "/tmp/proj".into();
        let mut b = a.clone();
        b.pid = 222;

        assert_eq!(a.history_key(), "codex-111-/tmp/proj");
        assert_ne!(a.history_key(), b.history_key());
    }

    /// 同一个工作目录下跑着两种不同的 agent，不能并成一行
    #[test]
    fn different_adapters_in_one_directory_stay_apart() {
        let mut a = session(&ago(0), None);
        a.adapter_id = "codex".into();
        a.pid = 111;
        a.working_dir = "/tmp/proj".into();
        let mut b = a.clone();
        b.adapter_id = "opencode".into();

        assert_ne!(a.history_key(), b.history_key());
    }

    /// 键里不能有「首次发现时间」
    ///
    /// 这是真实库里 16 行重复的根因：`discovered_at` 只活在进程内，
    /// AgentPulse 一重启它就变成「现在」，同一个会话于是换了个键重新落库。
    #[test]
    fn the_key_survives_an_app_restart() {
        let mut before = session(&ago(0), None);
        before.adapter_id = "codex".into();
        before.pid = 111;
        before.working_dir = "/tmp/proj".into();
        before.discovered_at = "2026-08-04 10:00:00".into();

        // 重启后同一个会话被重新发现，`discovered_at` 填的是「现在」
        let mut after = before.clone();
        after.discovered_at = "2026-08-04 18:30:00".into();

        assert_eq!(
            before.history_key(),
            after.history_key(),
            "重启不该让同一个会话在历史里多出一行"
        );
    }
}
