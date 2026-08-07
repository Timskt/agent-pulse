pub mod claude_code;
pub mod codex;
pub mod opencode;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[cfg(target_os = "windows")]
mod windows_process_identity {
    use std::ffi::c_void;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            creation_time: *mut FileTime,
            exit_time: *mut FileTime,
            kernel_time: *mut FileTime,
            user_time: *mut FileTime,
        ) -> i32;
        fn CloseHandle(object: *mut c_void) -> i32;
    }

    pub(super) fn creation_ticks(process_id: u32) -> u64 {
        // SAFETY: 只传入普通 PID；成功后所有输出指针都指向有效、可写的本地 FileTime，
        // 且无论 GetProcessTimes 成败都在返回前关闭拥有的 process handle。
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
            if process.is_null() {
                return 0;
            }
            let mut creation = FileTime { low: 0, high: 0 };
            let mut exit = FileTime { low: 0, high: 0 };
            let mut kernel = FileTime { low: 0, high: 0 };
            let mut user = FileTime { low: 0, high: 0 };
            let ok = GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
            CloseHandle(process);
            if ok == 0 {
                return 0;
            }
            (u64::from(creation.high) << 32) | u64::from(creation.low)
        }
    }
}

/// Windows 原始进程创建 FILETIME tick；其他平台返回 0。
///
/// Unix 秒不足以区分同一秒内的 PID 复用。自动投递把这个值一路带到 Windows helper，
/// 并在不可逆写入期间持有已核验进程句柄，形成严格的进程代际身份。
#[cfg(target_os = "windows")]
pub(crate) fn process_creation_ticks(process_id: u32) -> u64 {
    windows_process_identity::creation_ticks(process_id)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn process_creation_ticks(_process_id: u32) -> u64 {
    0
}

#[cfg(any(target_os = "windows", test))]
fn stable_creation_ticks(before: u64, identity_matches: bool, after: u64) -> Option<u64> {
    (identity_matches && before != 0 && before == after).then_some(after)
}

/// 进程快照 — 一次扫描共享，避免每个适配器重复枚举进程
#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub pid: u32,
    /// 进程启动时刻（Unix 秒）；与 PID 一起组成进程代际身份。
    pub started_at: u64,
    /// 小写进程名（Windows 含 .exe 后缀）
    pub name: String,
    /// 完整命令行（用于展示和跨扫描身份核验）
    pub cmd: String,
    /// 保留参数边界的 argv。身份提取必须使用它，不能从展示字符串反推参数。
    pub argv: Vec<String>,
    /// 工作目录
    pub cwd: String,
}

/// 获取当前所有进程的轻量快照（PID/启动时刻/name/cmd/cwd；不刷新 CPU/内存/磁盘）
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
        .map(|(pid, process)| {
            let argv = process_args(process);
            ProcessSnapshot {
                pid: pid.as_u32(),
                started_at: process.start_time(),
                name: process.name().to_string_lossy().to_lowercase(),
                cmd: argv.join(" "),
                argv,
                cwd: process_cwd(process),
            }
        })
        .collect()
}

fn process_args(process: &sysinfo::Process) -> Vec<String> {
    process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy().to_string())
        .collect()
}

fn process_cwd(process: &sysinfo::Process) -> String {
    process
        .cwd()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 对进程快照补充经过双重验证的 Windows creation FILETIME。
///
/// 全量进程枚举与适配器消费之间存在时间窗口；若 PID 在窗口内复用，直接再按 PID 读取
/// FILETIME 会把旧快照的命令行/cwd 与新进程的 generation 拼在一起。Windows 因此执行：
///
/// 1. 读取一次 creation ticks；
/// 2. 定向刷新同一 PID，并严格核对 start_time、进程名、命令行和可用的 cwd；
/// 3. 再读一次 creation ticks，只有前后相等且非零才接受。
///
/// 非 Windows 沿用原来的 `0` 语义，使调用方可以统一用 `Option`：只有 Windows 的
/// `None` 表示候选身份无法证明，适配器必须 fail closed。
#[cfg(target_os = "windows")]
pub(crate) fn validated_process_creation_ticks(process: &ProcessSnapshot) -> Option<u64> {
    let before = process_creation_ticks(process.pid);
    if before == 0 {
        return None;
    }

    let pid = sysinfo::Pid::from_u32(process.pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
    let current = system.process(pid)?;
    let current_name = current.name().to_string_lossy().to_lowercase();
    let current_command = process_args(current).join(" ");
    let current_cwd = process_cwd(current);
    let identity_matches = process.started_at != 0
        && current.start_time() == process.started_at
        && !process.name.is_empty()
        && current_name == process.name
        && !process.cmd.is_empty()
        && current_command == process.cmd
        && (process.cwd.is_empty() || current_cwd == process.cwd);

    let after = process_creation_ticks(process.pid);
    stable_creation_ticks(before, identity_matches, after)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn validated_process_creation_ticks(_process: &ProcessSnapshot) -> Option<u64> {
    Some(0)
}

/// 用「PID + 启动时刻」生成进程代际稳定的会话 id。
///
/// 只用 PID 会把已经退出的旧 Agent 和随后复用同一 PID 的新进程并成同一个会话，
/// 连带继承旧会话的冷却、失败退避和自动续跑额度。启动时刻跨 AgentPulse 重启稳定，
/// 又能在 PID 复用时自然换代，因此比首次发现时间更适合做身份的一部分。
pub fn process_session_id(prefix: &str, process: &ProcessSnapshot) -> String {
    process_session_id_with_creation_ticks(prefix, process, 0)
}

/// 用调用方已经读取到的 Windows 原始 creation FILETIME 生成严格进程代际 ID。
/// 非 Windows 或读取失败时退回跨平台 Unix 秒语义。
pub fn process_session_id_with_creation_ticks(
    prefix: &str,
    process: &ProcessSnapshot,
    process_created_at_ticks: u64,
) -> String {
    let generation = if process_created_at_ticks == 0 {
        process.started_at
    } else {
        process_created_at_ticks
    };
    format!("{prefix}-{}-{generation}", process.pid)
}

/// 续跑前确认「这个 PID 仍然是刚才发现的那一代 Agent 进程」。
///
/// 只看 PID 存不存在还不够：进程退出后 PID 可能很快被系统复用。这里直接定向刷新
/// 目标 PID，而不是为每条排队动作重新枚举整张进程表；启动时刻必须一致，命令行在
/// 两边都可读时也必须一致。旧版本反序列化出来没有启动时刻时，仍退回 PID + 命令行。
pub fn process_matches_session(session: &AgentSession) -> bool {
    let pid = sysinfo::Pid::from_u32(session.pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    let Some(current) = system.process(pid) else {
        return false;
    };

    if session.process_started_at != 0 && current.start_time() != session.process_started_at {
        return false;
    }
    if session.process_created_at_ticks != 0
        && process_creation_ticks(session.pid) != session.process_created_at_ticks
    {
        return false;
    }

    let current_command = current
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    session.command.is_empty() || current_command.is_empty() || current_command == session.command
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
    /// Rust 生成、前端只回传的不透明运行时代际键。手动续跑必须精确绑定该值，
    /// 防止旧界面行在 PID 复用后误投到新的进程实例。
    #[serde(default)]
    pub runtime_generation: String,
    /// 进程启动时刻（Unix 秒）。只在 Rust 内参与进程代际识别，不暴露给界面。
    #[serde(default, skip_serializing)]
    pub process_started_at: u64,
    /// Windows 原始进程创建 FILETIME tick；用于排除同秒 PID 复用。
    #[serde(default, skip_serializing)]
    pub process_created_at_ticks: u64,
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
    /// 限流保持窗口：按到这个时刻之前一律不敲字（v1.8）
    ///
    /// 进程内状态，不落库。跟 `resume_streak` 一样由 `scan_once` 逐轮合并：
    /// 会话消失重现就重新开始，那是对的——重新出现的会话该重新看证据。
    ///
    /// 存在的理由是**限流的证据会滚出视野**：适配器只读记录尾部 40 行，
    /// 而 agent 撞上限流后还会继续写重试日志，那行 `429` 很快被顶出去。
    /// 没有这个字段的话，手段就只能维持到那行字滚走为止，然后在窗口还没过去
    /// 的时候重新开始敲——那正是会让号被封的行为。
    #[serde(default)]
    pub rate_limit_hold: Option<crate::detector::RateLimitHold>,
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
    /// 没有记录文件的适配器（Codex / OpenCode）退回
    /// `adapter-pid-进程启动时刻-工作目录`。旧数据缺少启动时刻时兼容旧键形状。
    ///
    /// **这里刻意不含 `discovered_at`。** 含过，结果是这样的：`discovered_at`
    /// 只在进程内靠上一轮的会话表续着，AgentPulse 自己一重启那张表就空了，
    /// 同一个会话下一轮被填上「现在」，于是生出一个全新的键、库里多一行。
    /// 真实库里因此出现过同一个会话摊成 16 行的情况——历史页看着像噪音，
    /// 是因为它那时列的其实是「重启记录」，不是会话。
    ///
    /// 进程启动时刻来自操作系统，跨 AgentPulse 重启保持不变，但 PID 被复用时会变化；
    /// 因而既不会像 `discovered_at` 那样每次应用重启都裂出新行，也不会把两代进程并在一起。
    ///
    /// **必须跟收尾那一步用同一个定义**，否则「本轮还活着的键」跟表里的键对不上，
    /// [`crate::storage::Storage::close_missing_sessions`] 会把活着的会话
    /// 判成已结束——一个字面量抄两遍就够犯这个错，所以收成一个方法。
    pub fn history_key(&self) -> String {
        self.session_file.clone().unwrap_or_else(|| {
            let generation = if self.process_created_at_ticks == 0 {
                self.process_started_at
            } else {
                self.process_created_at_ticks
            };
            if generation == 0 {
                format!("{}-{}-{}", self.adapter_id, self.pid, self.working_dir)
            } else {
                format!(
                    "{}-{}-{}-{}",
                    self.adapter_id, self.pid, generation, self.working_dir
                )
            }
        })
    }
}

impl Default for AgentSession {
    fn default() -> Self {
        Self {
            id: String::new(),
            adapter_id: String::new(),
            agent_name: String::new(),
            pid: 0,
            runtime_generation: String::new(),
            process_started_at: 0,
            process_created_at_ticks: 0,
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
            rate_limit_hold: None,
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
        Box::new(codex::CodexAdapter::new()),
        Box::new(opencode::OpenCodeAdapter),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_process_matches_its_own_snapshot() {
        let snapshot = take_process_snapshot()
            .into_iter()
            .find(|process| process.pid == std::process::id())
            .expect("测试进程应该在系统快照里");
        let session = AgentSession {
            pid: snapshot.pid,
            process_started_at: snapshot.started_at,
            command: snapshot.cmd,
            ..Default::default()
        };
        assert!(process_matches_session(&session));
    }

    #[test]
    fn creation_tick_validation_requires_a_stable_nonzero_generation_and_matching_identity() {
        assert_eq!(stable_creation_ticks(100, true, 100), Some(100));
        assert_eq!(stable_creation_ticks(0, true, 0), None);
        assert_eq!(stable_creation_ticks(100, false, 100), None);
        assert_eq!(stable_creation_ticks(100, true, 101), None);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_creation_tick_validation_keeps_the_legacy_zero_generation() {
        let snapshot = ProcessSnapshot {
            pid: 42,
            started_at: 100,
            name: "codex".into(),
            cmd: "codex".into(),
            argv: vec!["codex".into()],
            cwd: "/tmp/project".into(),
        };
        assert_eq!(validated_process_creation_ticks(&snapshot), Some(0));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_creation_tick_validation_accepts_only_the_same_snapshot_identity() {
        let snapshot = take_process_snapshot()
            .into_iter()
            .find(|process| process.pid == std::process::id())
            .expect("测试进程应该在系统快照里");
        let ticks = validated_process_creation_ticks(&snapshot)
            .expect("当前测试进程的 generation 应可被双重验证");
        assert_ne!(ticks, 0);

        let mut stale = snapshot;
        stale.cmd.push_str(" --different-generation");
        assert_eq!(validated_process_creation_ticks(&stale), None);
    }

    #[test]
    fn missing_process_never_matches_a_session() {
        let session = AgentSession {
            pid: u32::MAX,
            ..Default::default()
        };
        assert!(!process_matches_session(&session));
    }
    #[test]
    fn a_reused_pid_with_a_different_start_time_is_rejected() {
        let snapshot = take_process_snapshot()
            .into_iter()
            .find(|process| process.pid == std::process::id())
            .expect("测试进程应该在系统快照里");
        let session = AgentSession {
            pid: snapshot.pid,
            process_started_at: snapshot.started_at.saturating_add(1),
            command: snapshot.cmd,
            ..Default::default()
        };
        assert!(
            !process_matches_session(&session),
            "PID 相同但启动代际不同，必须视为另一进程"
        );
    }

    #[test]
    fn process_generation_is_part_of_the_session_id() {
        let base = ProcessSnapshot {
            pid: 42,
            started_at: 100,
            name: "codex".into(),
            cmd: "codex".into(),
            argv: vec!["codex".into()],
            cwd: "/tmp/project".into(),
        };
        let mut replacement = base.clone();
        replacement.started_at = 200;

        assert_eq!(process_session_id("cx", &base), "cx-42-100");
        assert_ne!(
            process_session_id("cx", &base),
            process_session_id("cx", &replacement),
            "PID 被复用后必须生成新会话 id，不能继承旧冷却与额度"
        );
    }

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

    /// 没有记录文件时，PID 被复用也不能把两代进程并成一条历史。
    #[test]
    fn a_reused_pid_gets_a_new_history_key() {
        let mut old = session(&ago(0), None);
        old.adapter_id = "codex".into();
        old.pid = 111;
        old.process_started_at = 1_000;
        old.working_dir = "/tmp/proj".into();
        let mut replacement = old.clone();
        replacement.process_started_at = 2_000;

        assert_eq!(old.history_key(), "codex-111-1000-/tmp/proj");
        assert_ne!(old.history_key(), replacement.history_key());
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
        before.process_started_at = 1_000;
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
