use crate::adapters::AgentSession;
use crate::config::AppConfig;
use crate::i18n::I18n;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Command;

/// 续跑执行器 — 向中断的 Agent 发送续跑指令
///
/// 多窗口精确定位策略：
/// 1. 通过 PID 获取进程所在 TTY（如 /dev/ttys003）
/// 2. 通过 PID 父子关系识别终端应用（iTerm2/Terminal/VS Code/Cursor）
/// 3. 使用 AppleScript 遍历所有窗口/标签，精确匹配 TTY 后发送
///
/// **定位不到就不敲**：这一层的每个动作都会把字符敲进一个真人正在用的终端，
/// 敲错窗口比不续跑糟糕得多——那等于往别人的编辑器或另一个会话里插一句话再回车。
/// 所以所有「盲敲前台窗口」的分支都锁在 `config.auto_follow_latest` 后面（默认关），
/// 没开就返回 [`I18n`] 里的 `resume.blind_refused`，把选择权交回用户。
///
/// 平台支持：
/// - macOS: AppleScript + TTY 匹配（v0.1.0）
/// - Windows: SendInput API / PowerShell SendKeys (v0.2.0)
/// - Linux: xdotool / ydotool (v0.2.0)
pub struct Resumer {
    config: AppConfig,
    i18n: I18n,
}

/// 一次续跑投递**在现实里**的结果
///
/// 存在的理由是一个此前从没被问出口的问题：**脚本跑通了，字真的进那个会话了吗？**
///
/// 旧设计只有 `Result<String, String>`：`Ok` 的含义是「AppleScript / PowerShell /
/// xdotool 没报错」。可脚本成功跟字符落地是两件事——定位到一半焦点被别的窗口抢走、
/// 粘贴进了隔壁标签页、输入法把内容吃掉、pane 刚好被关掉，全都是「脚本成功、
/// 会话一动没动」。于是整条续跑链是**开环**的：发出动作，从不观察世界有没有变，
/// 也就永远学不会自己坏了。
///
/// 闭环靠的信号本来就躺在磁盘上：**agent 只要真的动起来，就会往自己的会话记录里
/// 写东西**。所以投递完盯一小会儿那个文件，长了就是落地了，没长就是没落地——
/// 至于为什么没落地（权限、焦点、输入法、通道），这一层不必知道，也正因如此
/// 以后新增通道不需要再配一套失败识别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOutcome {
    /// 通道自己就报错了：权限不在、定位不到、脚本超时
    Failed,
    /// 字敲出去了，而且**看见**会话动了起来
    Landed,
    /// 字敲出去了，但盯完那一小会儿会话还是没动——按键很可能进了别的窗口
    Silent,
    /// 字敲出去了；这个会话没有可读的记录文件，核验不了，只能按「大概进去了」记账
    Unverifiable,
}

impl ResumeOutcome {
    /// 这一次算不算「催过了一遍」
    ///
    /// 只有这种才该消耗 `max_resume_count` 的额度。**这是 v1.5 修掉的核心缺陷**：
    /// 旧代码在投递之前就把计数加了，失败也不回退，于是「敲不进去」被算成
    /// 「已经敲够了」——五次一到，这个会话的自动续跑就永久沉默，而一个字都没
    /// 真的敲进去过。macOS 的辅助功能授权每次重新构建应用都会失效，失败因此是
    /// 系统性的，不是偶发，用户看到的就是「自动续跑好像根本不工作」。
    pub fn counts_as_nudge(&self) -> bool {
        matches!(self, ResumeOutcome::Landed | ResumeOutcome::Unverifiable)
    }

    /// 这一次要不要计进「这条通道是不是坏了」
    ///
    /// `Silent` 也算：从用户角度看，「脚本说成功但会话没动」和「脚本报错」
    /// 是同一件事——没人替我按继续。
    pub fn is_failure(&self) -> bool {
        matches!(self, ResumeOutcome::Failed | ResumeOutcome::Silent)
    }

    /// 存进库、发给前端的稳定键
    ///
    /// 跟 [`Self::i18n_key`] 分开是刻意的：文案键随时可以改措辞、改前缀，
    /// 库里那一列改不了——已经写进去的行不会跟着变。所以落库走这个，
    /// 显示走那个，两者不共用一个字符串。
    pub fn storage_key(&self) -> &'static str {
        match self {
            ResumeOutcome::Failed => "failed",
            ResumeOutcome::Landed => "landed",
            ResumeOutcome::Silent => "silent",
            ResumeOutcome::Unverifiable => "unverifiable",
        }
    }

    /// 日志里那句「结果如何」的文案键
    pub fn i18n_key(&self) -> &'static str {
        match self {
            ResumeOutcome::Failed => "resume.outcome_failed",
            ResumeOutcome::Landed => "resume.outcome_landed",
            ResumeOutcome::Silent => "resume.outcome_silent",
            ResumeOutcome::Unverifiable => "resume.outcome_unverified",
        }
    }
}

/// 核验窗口：投递之后盯记录文件多久
///
/// Claude Code 收到提示词后是立刻把这条 user 消息追加进 jsonl 的，正常在一秒内。
/// 给到 6 秒是留给慢磁盘和网络盘；再长就会拖住扫描节拍，而且拖久了也不会变结论。
const VERIFY_WINDOW_SECS: u64 = 6;

/// 每次复查的间隔
const VERIFY_POLL_MS: u64 = 300;

/// 会话记录文件的活动指纹：(字节数, 修改时间)
///
/// 故意不问适配器「你怎么算动了」：**会话记录长出新内容**这条判据对所有把会话
/// 落盘的 agent 都成立，不需要每接一个新 agent 就重新实现一遍。没有记录文件的
/// agent 自然拿不到指纹，那种情况是「核验不可用」（[`ResumeOutcome::Unverifiable`]），
/// 不是失败——宁可少管，也不要给一个本来好使的通道判死刑。
fn activity_fingerprint(session: &AgentSession) -> Option<(u64, std::time::SystemTime)> {
    let path = session.session_file.as_ref()?;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// 以硬超时执行外部命令，超时后强制终止子进程
///
/// AppleScript / PowerShell / xdotool 在目标终端繁忙时可能长时间挂起
/// （macOS AppleEvent 默认要 120s 才报 -1712 超时），而 `std::process::Command::output()`
/// 是同步阻塞调用，会连带冻结整个监控循环。改为异步 spawn + kill_on_drop：
/// 超时后 future 被丢弃，子进程随之被回收。
async fn run_with_timeout(
    program: &str,
    args: &[&str],
    timeout_secs: u64,
    i18n: &I18n,
) -> Result<std::process::Output, String> {
    let child = tokio::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            i18n.tf(
                "cmd.spawn_failed",
                &[("program", program), ("detail", &e.to_string())],
            )
        })?;

    match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        child.wait_with_output(),
    )
    .await
    {
        Ok(result) => result.map_err(|e| {
            i18n.tf(
                "cmd.failed",
                &[("program", program), ("detail", &e.to_string())],
            )
        }),
        Err(_) => Err(i18n.tf(
            "cmd.timeout",
            &[("program", program), ("secs", &timeout_secs.to_string())],
        )),
    }
}

/// 执行 AppleScript（带硬超时），成功返回 stdout
#[cfg(target_os = "macos")]
async fn run_osascript(script: &str, i18n: &I18n) -> Result<String, String> {
    let output = run_with_timeout("osascript", &["-e", script], 20, i18n).await?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// 把提示词放进剪贴板，并记下原来的内容以便还原
///
/// 为什么不再用 `keystroke "整段提示词"`：`System Events` 的 keystroke 是**合成按键**，
/// 每个字符都要过一遍当前输入法。中文输入法开着的时候，一段中文提示词会被逐字
/// 重新解释成拼音——用户实测收到的是
/// 「啊啊啊啊啊啊啊啊啊goal啊啊啊啊啊啊，aaaaaaaaaa.aaaaaa，aaaaaaaaaaaa。」：
/// 每个汉字塌成一个「啊」或一个「a」，字数一一对应，只有 ASCII 的 `goal` 活了下来。
/// 这不是转义问题，换引号、换编码都没用，问题在于按键必须过输入法。
///
/// 剪贴板不过输入法：`set the clipboard to` 直接放 Unicode 文本，`⌘V` 只是一个
/// 「粘贴」动作，跟当前输入什么语言无关。代价是要借用一次用户的剪贴板，
/// 所以这里存旧值、[`RESTORE_CLIPBOARD`] 里再还回去。
#[cfg(target_os = "macos")]
fn stage_clipboard(escaped_prompt: &str) -> String {
    format!(
        r#"set savedClipboard to missing value
try
    set savedClipboard to the clipboard as text
end try
set the clipboard to "{escaped_prompt}""#
    )
}

/// 粘贴动作本身：`⌘V` + 回车
///
/// AppleScript 不在乎缩进，所以这段可以原样插进任何 `tell process` 块里。
#[cfg(target_os = "macos")]
const PASTE_KEYS: &str = r#"keystroke "v" using command down
            delay 0.3
            key code 36"#;

/// 把剪贴板还给用户
///
/// 还原前留 0.5s：`⌘V` 是异步的，太早改回去会粘到旧内容。
#[cfg(target_os = "macos")]
const RESTORE_CLIPBOARD: &str = r#"delay 0.5
try
    if savedClipboard is not missing value then set the clipboard to savedClipboard
end try"#;

/// 跨平台：读取会话所在终端的 TTY（Windows 无 TTY 概念，返回 None）
pub fn session_tty(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        Resumer::get_tty_for_pid(pid)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// 跨平台：识别会话所属的终端应用（识别不出返回 None）
pub fn session_terminal_app(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let name = Resumer::find_terminal_for_pid(pid);
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// 终端复用器里的一个投递目标（tmux pane / screen window）
///
/// **为什么这条路排在所有 GUI 终端之前**：
/// - `tmux send-keys -t %3` 是按 **pane id 寻址**的。不需要窗口在前台、不需要猜
///   是哪个标签、不需要辅助功能权限，用户甚至可以正在另一个 Space 里干别的。
/// - 它写的是 pane 的伪终端，**完全不经过输入法**。中文提示词被拼音改写成
///   「啊啊啊啊……」的那个问题（见 [`stage_clipboard`]）在这条路上不存在，
///   连剪贴板都不用借。
/// - 参数是 argv 数组，不拼 shell 字符串，提示词里有什么字符都无所谓。
///
/// 代价是它只覆盖「跑在复用器里的会话」，认不出来就往下走原来的 GUI 路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuxTarget {
    /// `tmux` 或 `screen`
    pub tool: &'static str,
    /// tmux 的 pane id（`%3`）；screen 的 session 名
    pub target: String,
    /// 这个 pane 的 tty，用来跟会话的 tty 对账（screen 拿不到，为空）
    pub tty: String,
}

impl MuxTarget {
    /// 这个目标是不是**逐 pane 精确**的
    ///
    /// tmux 能按 tty 把会话对到具体 pane，所以是精确的。
    /// screen 不暴露 tty→window 的映射，`-X stuff` 只能投给该 session
    /// **当前选中的那个 window**——万一用户切走了，字就敲进别的窗口。
    /// 所以 screen 只能算窗口级，得跟其它盲敲路径一样先要授权。
    pub fn is_exact(&self) -> bool {
        self.tool == "tmux"
    }
}

/// 生成投递命令序列：按顺序执行的 `(程序, argv)` 列表
///
/// 拆成纯函数是为了让三个平台的 CI 都能测到参数拼法——这里错一个 flag 的代价
/// 不是报错，而是把 `Enter` 四个字母敲进用户的会话。
///
/// 两个关键 flag：
/// - `-l`：关掉 key name 查表，把整串当字面 UTF-8 送。少了它，提示词里出现
///   `Enter`、`Escape`、`C-c` 这类词会被当成按键名解释。
/// - `--`：终止 flag 解析。少了它，以 `-` 开头的提示词会被 tmux 当成选项。
///
/// **故意不加 cfg**：纯字符串拼装，每个平台的单元测试都跑得到。
pub fn mux_commands(target: &MuxTarget, prompt: &str) -> Vec<(String, Vec<String>)> {
    match target.tool {
        "screen" => {
            // screen 的 `stuff` 直接把字符串塞进输入队列，末尾补一个回车（\r）。
            // 不用 \n：终端里回车键送的是 CR，LF 在某些 TUI 里不触发提交。
            vec![(
                "screen".to_string(),
                vec![
                    "-S".to_string(),
                    target.target.clone(),
                    "-X".to_string(),
                    "stuff".to_string(),
                    format!("{prompt}\r"),
                ],
            )]
        }
        // tmux：先送字面文本，再单独送一次 Enter。
        // 合成一步（在 prompt 末尾加 \r）也行，但分两步的失败模式更清楚：
        // 第一步失败就还没提交，用户的输入框里不会留半句话。
        _ => vec![
            (
                "tmux".to_string(),
                vec![
                    "send-keys".to_string(),
                    "-t".to_string(),
                    target.target.clone(),
                    "-l".to_string(),
                    "--".to_string(),
                    prompt.to_string(),
                ],
            ),
            (
                "tmux".to_string(),
                vec![
                    "send-keys".to_string(),
                    "-t".to_string(),
                    target.target.clone(),
                    "Enter".to_string(),
                ],
            ),
        ],
    }
}

/// 从 `tmux list-panes` 的输出里找出 tty 匹配的 pane
///
/// 每行形如 `%3\t/dev/ttys004`。会话的 tty 就是它所在 pane 的伪终端，
/// 所以这一步是**对账**而不是猜测：对上了就是它，对不上就没有。
///
/// **故意不加 cfg**：纯解析，每个平台都能测。
pub fn match_tmux_pane(list_output: &str, tty: &str) -> Option<String> {
    if tty.is_empty() {
        return None;
    }
    for line in list_output.lines() {
        let mut parts = line.split('\t');
        let pane = parts.next()?.trim();
        let pane_tty = parts.next().unwrap_or_default().trim();
        if pane.is_empty() || pane_tty.is_empty() {
            continue;
        }
        // tmux 报的是 /dev/ttys004，`ps -o tty=` 也被我们补成了同样的前缀；
        // 两边都可能少个 /dev/，所以按后缀互认一次
        if pane_tty == tty || pane_tty.ends_with(tty) || tty.ends_with(pane_tty) {
            return Some(pane.to_string());
        }
    }
    None
}

/// 查找会话所在的 tmux pane / screen session
///
/// 顺序是先 tmux（能精确到 pane）再 screen（只能到 session）。
/// 任何一步失败都返回 `None` 让调用方走原来的路——这里不该抛错，
/// 「机器上没装 tmux」是最常见的情况，不是故障。
#[cfg(unix)]
pub fn mux_target_for_pid(pid: u32) -> Option<MuxTarget> {
    let tty = session_tty(pid)?;

    // tmux：一次列出所有 session 的所有 pane，按 tty 对账
    if let Ok(out) = Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{pane_id}\t#{pane_tty}"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(pane) = match_tmux_pane(&text, &tty) {
                return Some(MuxTarget {
                    tool: "tmux",
                    target: pane,
                    tty,
                });
            }
        }
    }

    // screen：沿父进程链找 SCREEN，拿到 session 名
    if let Some(name) = screen_session_for_pid(pid) {
        return Some(MuxTarget {
            tool: "screen",
            target: name,
            tty: String::new(),
        });
    }

    None
}

/// 非 unix 平台没有 tmux/screen
#[cfg(not(unix))]
pub fn mux_target_for_pid(pid: u32) -> Option<MuxTarget> {
    let _ = pid;
    None
}

/// 沿父进程链找 `SCREEN`，返回它的 session 名（`12345.pts-0.host`）
///
/// screen 的子进程的父链上一定有那个 `SCREEN` 服务进程。拿到它的 pid 之后
/// 用 `screen -ls` 反查全名——`-X` 需要的是全名或唯一前缀，光有 pid 不够。
#[cfg(unix)]
fn screen_session_for_pid(pid: u32) -> Option<String> {
    let mut current = pid;
    for _ in 0..8 {
        let out = Command::new("ps")
            .args(["-o", "ppid=,comm=", "-p", &current.to_string()])
            .output()
            .ok()?;
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let (ppid_str, comm) = line.split_once(' ')?;
        let ppid: u32 = ppid_str.trim().parse().ok()?;
        let comm = comm.trim();
        // screen 的服务进程名是大写 SCREEN，客户端才是小写 screen
        if comm.rsplit('/').next().unwrap_or(comm) == "SCREEN" {
            let ls = Command::new("screen").arg("-ls").output().ok()?;
            let text = String::from_utf8_lossy(&ls.stdout);
            return match_screen_session(&text, current);
        }
        if ppid <= 1 {
            break;
        }
        current = ppid;
    }
    None
}

/// 从 `screen -ls` 的输出里按 pid 找出 session 全名
///
/// 输出形如 `\t12345.pts-0.host\t(Detached)`，第一段就是全名，点号前是 pid。
///
/// **故意不加 cfg**：纯解析，每个平台都能测。
pub fn match_screen_session(ls_output: &str, pid: u32) -> Option<String> {
    let prefix = format!("{pid}.");
    for line in ls_output.lines() {
        let name = line.split_whitespace().next().unwrap_or_default();
        if name.starts_with(&prefix) {
            return Some(name.to_string());
        }
    }
    None
}

/// 一次续跑演练的结果：**走完全部定位流程，但一个字都不敲**
///
/// 这个类型存在的理由：续跑是「按下去才知道对不对」的动作，而敲错窗口比不续跑
/// 糟糕得多。演练把「要冒风险试一次」变成「零风险随时可查」——用户在真的依赖
/// 自动续跑之前，先看一眼它认到了哪儿。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResumeProbe {
    pub session_id: String,
    /// `exact` | `window` | `none`
    pub certainty: String,
    /// 已本地化的确定性名称
    pub certainty_label: String,
    /// 已本地化的通道名称（tmux 面板 / iTerm2 标签 / 编辑器内置终端 …）
    pub channel: String,
    /// 定位到的具体目标：pane id、tty、窗口标题、窗口 id
    pub target: Option<String>,
    /// 已本地化的一段解释：这串字符会落到哪儿、为什么
    pub detail: String,
    /// 按现在的配置，真的点续跑会不会发出字符
    pub would_deliver: bool,
    pub terminal_app: Option<String>,
    pub tty: Option<String>,
    pub project_name: String,
    /// 「盲敲最前窗口」开没开
    pub allow_blind: bool,
    /// macOS 上缺「辅助功能」权限——界面据此给一个「去开权限」按钮
    ///
    /// 用一个布尔而不是让前端去认工具名：认名字就得按语言匹配字符串，
    /// 换成英文界面立刻失灵。
    pub needs_permission_fix: bool,
    /// 环境自检：这条路要用到的外部工具在不在
    pub tools: Vec<ToolStatus>,
}

/// 单个外部依赖的可用性
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    /// 已本地化的一句话：这个工具是干什么的
    pub purpose: String,
}

/// macOS：辅助功能权限拿到了没有
///
/// **这是「跳过去了但一个字都没敲」的头号原因。** `System Events` 的 keystroke
/// 需要「隐私与安全性 › 辅助功能」里勾上本应用；没勾上时，脚本前半段
/// （`activate` / 选标签）照样成功，用户看到窗口跳过来了，
/// 后半段 keystroke 直接抛 -1719，于是「跳转完就没动作」。
///
/// 更阴的一点：**重新构建过的应用会让这条授权失效**。TCC 记的是代码签名，
/// 换了二进制就不算同一个应用了，系统设置里那个勾还在、实际已经不生效。
///
/// 用 `UI elements enabled` 查询：它只读、不弹窗、不会把用户拽进设置面板。
#[cfg(target_os = "macos")]
async fn accessibility_granted(i18n: &I18n) -> bool {
    matches!(
        run_osascript(
            "tell application \"System Events\" to return UI elements enabled",
            i18n,
        )
        .await
        .as_deref(),
        Ok("true")
    )
}

/// 判断一段 osascript 报错是不是「没有辅助功能权限」
///
/// 单独拎出来是因为这个错误的**默认表现是静默**：用户只看到窗口跳过来，
/// 什么都没发生，然后来问「是不是坏了」。必须换成一句能照着做的话。
///
/// **故意不加 cfg**：纯字符串判断，每个平台都能测。
pub fn is_accessibility_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("-1719")
        || lower.contains("-25211")
        || lower.contains("not allowed to send keystrokes")
        || lower.contains("not allowed assistive access")
        || lower.contains("assistive access")
        || lower.contains("osascript is not allowed")
}

/// 一个通道要不要 macOS 的辅助功能权限
///
/// iTerm2 的 `write text` 和 tmux 的 `send-keys` 都是直接写伪终端，不碰
/// `System Events`，所以这两条路即使没授权也能敲进去。其余分支全靠合成按键。
///
/// **故意不加 cfg**：纯映射，每个平台都能测。
pub fn channel_needs_accessibility(channel_key: &str) -> bool {
    !matches!(
        channel_key,
        "probe.channel_tmux" | "probe.channel_screen" | "probe.channel_iterm2"
    )
}

/// 从 `codesign -d --requirements -` 的输出判断签名身份稳不稳
///
/// **这是「我明明勾选了却还是敲不进去」的真正原因。** macOS 把辅助功能授权挂在
/// 「指定要求」（designated requirement）上，既不是路径也不是 bundle id：
///
/// - 正式证书签过的：`identifier "…" and anchor apple generic and certificate leaf …`
///   —— 认的是**名字加证书**，重新构建、升级、换路径都还是同一个身份，勾一次就一直有效。
/// - 临时签名（adhoc）：`cdhash H"d449…"` —— 认的是**这一个二进制的哈希**。
///   改一行代码重新构建，哈希就变了，对系统来说这是另一个应用。
///
/// 于是就有了那个最气人的现象：系统设置里那个勾还在（它记的是旧哈希），
/// 但正在跑的这个二进制对不上，`UI elements enabled` 返回 false，
/// 合成按键静默失效。用户以为自己授权了，其实授权给的是上一个构建。
///
/// **故意不加 cfg**：纯字符串判断，每个平台都能测。
pub fn signature_is_stable(requirements: &str) -> bool {
    // 这两个标记只出现在真证书签出来的要求里；adhoc 的要求里只有一个 cdhash
    requirements.contains("anchor apple") || requirements.contains("certificate")
}

/// 当前进程所在的 `.app` 包路径；开发模式下（cargo 目录里）返回 `None`
///
/// 返回 `None` 是有意的：`tauri dev` 跑的是 target 目录里的裸二进制，
/// 本来就没有稳定签名，这时候报警只是噪音。
#[cfg(target_os = "macos")]
fn app_bundle_path() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    // …/AgentPulse.app/Contents/MacOS/agent-pulse → …/AgentPulse.app
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_string_lossy().into_owned())
}

/// 本应用的签名身份能不能活过一次更新
///
/// 查不出来就当它是稳定的：这一项只用来给错误信息加一句解释，
/// 拿不到证据的时候宁可少说一句，也不要凭猜测吓人。
#[cfg(target_os = "macos")]
async fn signature_stable(i18n: &I18n) -> bool {
    let Some(path) = app_bundle_path() else {
        return true;
    };
    let Ok(out) =
        run_with_timeout("codesign", &["-d", "--requirements", "-", &path], 10, i18n).await
    else {
        return true;
    };
    // codesign 把「指定要求」写到 stdout，其余签名信息写到 stderr，两边都要看
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    signature_is_stable(&text)
}

/// 缺权限时该说哪一句：是「还没勾」还是「勾了但签名换了所以失效」
///
/// 分两句话是因为要用户做的动作不一样：前者去勾上，后者**取消再勾一次**，
/// 而且得知道这事儿每次更新都会重演、根治要靠稳定签名。
/// 一句笼统的「去开权限」会让已经勾过的人以为程序在骗他。
///
/// 传两个 key 而不是写死一对：续跑失败和体检报告的语气不一样，
/// 但「怎么判断」这件事只该有一份实现。
#[cfg(target_os = "macos")]
async fn accessibility_hint(
    i18n: &I18n,
    stable_key: &'static str,
    adhoc_key: &'static str,
) -> &'static str {
    if signature_stable(i18n).await {
        stable_key
    } else {
        adhoc_key
    }
}

/// 走完全部定位流程但**一个字都不敲**，报告「续跑会把字敲到哪儿」
///
/// 这是 12.1 那张「从没实机验证过」表格的解药：把「要冒险按一次才知道」
/// 变成「零风险随时可查」。
pub async fn probe_resume(session: &AgentSession, config: &AppConfig) -> ResumeProbe {
    let i18n = I18n::from_code(&config.language);
    let allow_blind = config.auto_follow_latest;
    let tty = session_tty(session.pid);
    let terminal_app = session_terminal_app(session.pid);
    let project_name = project_name_of(&session.working_dir).to_string();

    let tools = collect_tools(&i18n).await;

    // 演练借真续跑的那一份 Resumer 来生成定位脚本：**同一份配置、同一批脚本**。
    // 演练要是自己搓一套脚本，两边迟早不一致。
    let resumer = Resumer::new(config.clone());

    // 复用器优先：认出来就到此为止，这是确定性最高的一条
    let (certainty, channel_key, target) = match mux_target_for_pid(session.pid) {
        Some(m) if m.is_exact() => ("exact", "probe.channel_tmux", Some(m.target)),
        Some(m) => ("window", "probe.channel_screen", Some(m.target)),
        None => {
            locate_gui(
                &resumer,
                session,
                terminal_app.as_deref().unwrap_or_default(),
                tty.as_deref(),
                &project_name,
            )
            .await
        }
    };

    let channel = i18n.t_owned(channel_key);

    let mut would_deliver = match certainty {
        "exact" | "window" => true,
        _ => allow_blind,
    };

    // macOS：定位到了也可能敲不进去——没有辅助功能权限的合成按键是空动作。
    //
    // 只在「本来会敲」的时候才查：定位都失败的行上再挂一句权限警告纯属噪音，
    // 而且能省一次 osascript 往返。
    let blocked_by_permission = {
        #[cfg(target_os = "macos")]
        {
            would_deliver
                && channel_needs_accessibility(channel_key)
                && !accessibility_granted(&i18n).await
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    };

    let mut detail = match certainty {
        "exact" => i18n.tf(
            "probe.detail_exact",
            &[
                ("channel", &channel),
                ("target", target.as_deref().unwrap_or("-")),
            ],
        ),
        "window" => i18n.tf(
            "probe.detail_window",
            &[
                ("channel", &channel),
                ("target", target.as_deref().unwrap_or("-")),
            ],
        ),
        _ if allow_blind => i18n.t_owned("probe.detail_none_blind"),
        _ => i18n.t_owned("probe.detail_none"),
    };

    if blocked_by_permission {
        would_deliver = false;
        // 「没勾」和「勾了但签名换了所以失效」要说的话不一样，别让勾过的人以为程序在骗他
        #[cfg(target_os = "macos")]
        let hint = accessibility_hint(
            &i18n,
            "probe.no_accessibility",
            "probe.no_accessibility_adhoc",
        )
        .await;
        #[cfg(not(target_os = "macos"))]
        let hint = "probe.no_accessibility";
        detail = format!("{detail}\n\n{}", i18n.t(hint));
    }

    ResumeProbe {
        session_id: session.id.clone(),
        certainty: certainty.to_string(),
        certainty_label: i18n.t_owned(match certainty {
            "exact" => "probe.certainty_exact",
            "window" => "probe.certainty_window",
            _ => "probe.certainty_none",
        }),
        channel,
        target,
        detail,
        would_deliver,
        terminal_app,
        tty,
        project_name,
        allow_blind,
        needs_permission_fix: blocked_by_permission,
        tools,
    }
}

/// 环境自检：续跑这条路要用到的外部依赖在不在
///
/// 每个平台只报自己真的会用到的东西——在 macOS 上列一行「xdotool 缺失」
/// 只会让人白担心。
async fn collect_tools(i18n: &I18n) -> Vec<ToolStatus> {
    let mut tools = Vec::new();

    #[cfg(unix)]
    {
        for (name, purpose) in [("tmux", "probe.tool_tmux"), ("screen", "probe.tool_screen")] {
            tools.push(ToolStatus {
                name: name.to_string(),
                available: unix_tool_present(name),
                purpose: i18n.t_owned(purpose),
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        tools.push(ToolStatus {
            name: i18n.t_owned("probe.tool_accessibility_name"),
            available: accessibility_granted(i18n).await,
            purpose: i18n.t_owned("probe.tool_accessibility"),
        });
    }

    #[cfg(target_os = "linux")]
    {
        for (name, purpose) in [
            ("xdotool", "probe.tool_xdotool"),
            ("ydotool", "probe.tool_ydotool"),
        ] {
            tools.push(ToolStatus {
                name: name.to_string(),
                available: unix_tool_present(name),
                purpose: i18n.t_owned(purpose),
            });
        }
        let clipboard = LINUX_CLIPBOARD_TOOLS
            .iter()
            .find(|(bin, _)| unix_tool_present(bin));
        tools.push(ToolStatus {
            name: clipboard
                .map(|(bin, _)| bin.to_string())
                .unwrap_or_else(|| "wl-copy / xclip / xsel".to_string()),
            available: clipboard.is_some(),
            purpose: i18n.t_owned("probe.tool_clipboard"),
        });
    }

    #[cfg(target_os = "windows")]
    {
        let _ = i18n;
        tools.push(ToolStatus {
            name: "powershell".to_string(),
            available: true,
            purpose: i18n.t_owned("probe.tool_powershell"),
        });
    }

    tools
}

/// `which <bin>` —— 只问在不在，不执行它
#[cfg(unix)]
fn unix_tool_present(bin: &str) -> bool {
    Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// macOS：跑「只定位」脚本，一个字都不敲
///
/// 定位链路和真续跑共用同一批生成器（[`Resumer::macos_locate_script`]），
/// 所以演练说「会敲进 iTerm2 的这个 tty」时，真续跑走的就是同一条路。
/// 两套独立的定位实现迟早会互相打脸——那时用户看到的就是
/// 「演练说会敲，按下去却没反应」，比没有演练更糟。
#[cfg(target_os = "macos")]
async fn locate_gui(
    resumer: &Resumer,
    _session: &AgentSession,
    terminal_app: &str,
    tty: Option<&str>,
    project_name: &str,
) -> (&'static str, &'static str, Option<String>) {
    let channel_key = macos_channel_key(terminal_app);
    let Some(script) = resumer.macos_locate_script(terminal_app, tty, project_name) else {
        // 认不出是哪个终端：连 osascript 都不启动，把应用名原样报上去
        let target = (!terminal_app.is_empty()).then(|| terminal_app.to_string());
        return ("none", channel_key, target);
    };
    match run_osascript(&script, &resumer.i18n).await.as_deref() {
        // TTY 对上了：GUI 这条路上能拿到的最强证据
        Ok("matched") => ("exact", channel_key, tty.map(str::to_string)),
        // 只到窗口：IDE 内置终端认得出是哪个窗口，认不出是窗口里的哪个面板
        Ok("vscode-window") => ("window", channel_key, Some(project_name.to_string())),
        // refused / no-app / 脚本失败：都是「定位不到」，别硬凑一个级别出来
        _ => ("none", channel_key, None),
    }
}

/// 终端应用 → 演练面板上显示的通道名
#[cfg(target_os = "macos")]
fn macos_channel_key(app: &str) -> &'static str {
    match app {
        "iTerm2" => "probe.channel_iterm2",
        "Terminal" => "probe.channel_terminal",
        a if TITLE_MATCHED_APPS.contains(&a) => "probe.channel_ide",
        "" => "probe.channel_unknown",
        _ => "probe.channel_frontmost",
    }
}

/// Linux：只查窗口 id，不激活也不输入
#[cfg(target_os = "linux")]
async fn locate_gui(
    _resumer: &Resumer,
    session: &AgentSession,
    _terminal_app: &str,
    _tty: Option<&str>,
    _project_name: &str,
) -> (&'static str, &'static str, Option<String>) {
    match Resumer::find_x11_window_for_pid(session.pid) {
        Some(wid) => ("window", "probe.channel_x11", Some(wid)),
        None => ("none", "probe.channel_x11", None),
    }
}

/// Windows：沿父进程链找宿主窗口、核一下标题，不切前台也不输入
///
/// 分级在 PowerShell 里就做完了（[`Resumer::windows_locate_script`]，与真续跑
/// 共用同一套定位逻辑），Rust 这边只翻译结果码——判定只有一份，
/// 演练和真续跑不会给出两种答案。
#[cfg(target_os = "windows")]
async fn locate_gui(
    resumer: &Resumer,
    session: &AgentSession,
    _terminal_app: &str,
    _tty: Option<&str>,
    project_name: &str,
) -> (&'static str, &'static str, Option<String>) {
    let script = Resumer::windows_locate_script(session.pid, project_name, resumer.allow_blind());
    let Ok(out) = run_with_timeout(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
        25,
        &resumer.i18n,
    )
    .await
    else {
        return ("none", "probe.channel_console", None);
    };
    let (host, code) = parse_windows_locate_output(&String::from_utf8_lossy(&out.stdout));
    let target = (!host.is_empty()).then_some(host);
    (
        windows_locate_certainty(&code),
        "probe.channel_console",
        target,
    )
}

/// 解析 Windows 定位脚本的输出：`HOST=<宿主进程名>` 一行 + 结果码一行
///
/// **故意不加 cfg**：纯解析，三个平台的 CI 都能编它、测它。
pub fn parse_windows_locate_output(stdout: &str) -> (String, String) {
    let mut host = String::new();
    let mut code = String::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("HOST=") {
            host = v.to_string();
        } else if !line.is_empty() {
            code = line.to_string();
        }
    }
    (host, code)
}

/// Windows 结果码 → 演练的确定性级别
///
/// `BLIND` 归「定位不到」而不是「窗口级」：它的字面意思是「标题没对上，
/// 但你开了盲敲所以照敲」，那正是 `probe.detail_none_blind` 要讲的话。
/// 谎报一个窗口级确定性反而会让人以为定位成功了。
pub fn windows_locate_certainty(code: &str) -> &'static str {
    match code {
        "EXACT" => "exact",
        "WINDOW" => "window",
        // BLIND / REFUSED / NO_WINDOW / 空
        _ => "none",
    }
}

/// 其它平台：没有续跑通道
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
async fn locate_gui(
    _resumer: &Resumer,
    _session: &AgentSession,
    _terminal_app: &str,
    _tty: Option<&str>,
    _project_name: &str,
) -> (&'static str, &'static str, Option<String>) {
    ("none", "probe.channel_unknown", None)
}

/// 直接把用户送到「辅助功能」设置页
///
/// 光说「去系统设置 › 隐私与安全性 › 辅助功能」不够——那是三层菜单，
/// 而且真正的动作是**取消再勾一次**，不是第一次勾上。少一步都可能让人卡住。
#[cfg(target_os = "macos")]
pub async fn open_accessibility_settings(lang: &str) -> Result<String, String> {
    let i18n = I18n::from_code(lang);
    let out = run_with_timeout(
        "open",
        &["x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"],
        10,
        &i18n,
    )
    .await?;
    if out.status.success() {
        Ok(i18n.t_owned("probe.settings_opened"))
    } else {
        Err(i18n.tf(
            "cmd.failed",
            &[
                ("program", "open"),
                ("detail", String::from_utf8_lossy(&out.stderr).trim()),
            ],
        ))
    }
}

/// 别的平台没有这个权限概念
#[cfg(not(target_os = "macos"))]
pub async fn open_accessibility_settings(lang: &str) -> Result<String, String> {
    Err(I18n::from_code(lang)
        .t("probe.settings_unsupported")
        .to_string())
}

/// 投递通道体检：现在到底敲不敲得进去（不敲字、不弹权限窗）
///
/// 这个功能的卖点是「静默、用户无感」，代价是**它坏掉的时候也一样静默**。
/// macOS 的辅助功能授权每次重新构建应用就会失效（TCC 记的是代码签名），
/// 于是典型剧本是：用户装了新版本，设置里那个勾还在，一切看起来正常，
/// 直到某天想起来「它好久没帮我按继续了」。
///
/// 所以引擎一启动就先体检：敲得进去就闭嘴，敲不进去就**现在**说，
/// 而不是等某个会话真卡住、还白烧掉几次续跑额度之后才说。
///
/// 返回 `None` = 通道健康；`Some(能照着做的一句话)` = 当下敲不进去。
#[cfg(target_os = "macos")]
pub async fn channel_health(lang: &str) -> Option<String> {
    let i18n = I18n::from_code(lang);
    if accessibility_granted(&i18n).await {
        return None;
    }
    Some(
        i18n.t_owned(
            accessibility_hint(
                &i18n,
                "resume.needs_accessibility",
                "resume.needs_accessibility_adhoc",
            )
            .await,
        ),
    )
}

/// Windows / Linux 没有这层授权，事前没什么可体检的
#[cfg(not(target_os = "macos"))]
pub async fn channel_health(_lang: &str) -> Option<String> {
    None
}

/// macOS 上进程名 → 应用显示名的映射
#[cfg(target_os = "macos")]
fn mac_app_display_name(terminal_app: &str) -> &str {
    match terminal_app {
        "Code" => "Visual Studio Code",
        other => other,
    }
}

/// Electron / Chromium 辅助进程的名字标记
///
/// 这些进程从来不是「会话所在的终端」，可它们的名字里偏偏带着关键词
/// （`Code Helper (Renderer)`、`com.apple.CodeSigningHelper`、`Qoder Helper`），
/// 是误判的主要来源。命中就跳过，继续往父进程走——主进程就在上面。
#[cfg(unix)]
const HELPER_MARKERS: &[&str] = &[
    "helper",
    "crashpad",
    "renderer",
    "gpu",
    "utility",
    "plugin",
    "codesigning",
];

/// 应用名 → 终端标识；按 `contains` 匹配，表的顺序就是优先级
#[cfg(unix)]
const TERMINAL_PATTERNS: &[(&str, &str)] = &[
    ("iterm", "iTerm2"),
    ("visual studio code", "Code"),
    ("vscodium", "Code"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("trae", "Trae"),
    ("qoder", "Qoder"),
    ("warp", "Warp"),
    ("wezterm", "WezTerm"),
    ("kitty", "Kitty"),
    ("alacritty", "Alacritty"),
    ("ghostty", "Ghostty"),
    ("konsole", "Konsole"),
    ("tilix", "Tilix"),
    ("xterm", "XTerm"),
    // JetBrains 全家桶：内置终端也是常见的 agent 落脚点。
    // 这些名字够独特，`contains` 不会误伤（"idea" 只出现在 IntelliJ IDEA.app 里）
    ("intellij idea", "IntelliJ IDEA"),
    ("pycharm", "PyCharm"),
    ("webstorm", "WebStorm"),
    ("goland", "GoLand"),
    ("clion", "CLion"),
    ("rustrover", "RustRover"),
    ("phpstorm", "PhpStorm"),
    ("rubymine", "RubyMine"),
    ("datagrip", "DataGrip"),
    ("rider", "Rider"),
    ("android studio", "Android Studio"),
];

/// 靠「窗口标题里有项目名」定位的应用
///
/// 这些应用一个窗口挂多个标签/面板，AppleScript 里也没有按 TTY 选标签的接口
/// （iTerm2 / Terminal.app 有，所以它们不在这张表里）。能拿到的最强证据就是
/// 窗口标题——IDE 都会把项目名写进标题栏。对不上就交给 `auto_follow_latest` 定夺。
#[cfg(target_os = "macos")]
const TITLE_MATCHED_APPS: &[&str] = &[
    "Code",
    "Cursor",
    "Windsurf",
    "Trae",
    "Qoder",
    "IntelliJ IDEA",
    "PyCharm",
    "WebStorm",
    "GoLand",
    "CLion",
    "RustRover",
    "PhpStorm",
    "RubyMine",
    "DataGrip",
    "Rider",
    "Android Studio",
];

/// 从工作目录里取项目名，用来跟窗口标题对照
///
/// 两种分隔符都要吃：Windows 上进程的 cwd 是 `C:\code\agent-pulse`，
/// 而同一台机器的 Git Bash / WSL 又会给出 `/mnt/c/code/agent-pulse`。
/// 只按 `/` 切的话，Windows 上取到的「项目名」是整条盘符路径，
/// 跟窗口标题永远对不上——于是每次续跑都被判成「定位不到」而放弃。
pub fn project_name_of(working_dir: &str) -> &str {
    working_dir
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
}

/// Windows 上「一个窗口挂多个标签」的宿主（小写进程名，不含 .exe）
///
/// 这类宿主认到窗口只等于认到应用，还得靠窗口标题才能判断当前标签是不是目标会话。
/// conhost / cmd 不在这张表里：它们一个窗口就是一个控制台。
///
/// **故意不加 cfg**：纯数据，每个平台的 CI 都能测。
pub const WINDOWS_MULTI_TAB_HOSTS: &[&str] = &[
    // 终端
    "windowsterminal",
    "hyper",
    "tabby",
    "conemu",
    "conemu64",
    "fluentterminal",
    // 编辑器 / IDE 的内置终端
    "code",
    "code - insiders",
    "cursor",
    "windsurf",
    "trae",
    "zed",
    "idea64",
    "pycharm64",
    "webstorm64",
    "goland64",
    "clion64",
    "rider64",
    "phpstorm64",
    "rubymine64",
    "rustrover64",
    "datagrip64",
    "studio64",
    "fleet",
];

/// 从可执行文件路径里取出 `.app` 包名
///
/// `/applications/visual studio code.app/contents/macos/electron` → `visual studio code`
#[cfg(unix)]
fn app_bundle_name(path: &str) -> Option<&str> {
    let idx = path.rfind(".app")?;
    // `.app` 后面必须是路径分隔符或者到头了，否则只是名字里恰好有这四个字符
    let after = &path[idx + 4..];
    if !(after.is_empty() || after.starts_with('/')) {
        return None;
    }
    let name = path[..idx].rsplit('/').next().unwrap_or_default();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 判断一个进程名是不是某个终端应用（传入的字符串必须已小写）
///
/// 为什么不能直接 `contains`：macOS 上 `ps -o comm=` 给的是**完整可执行文件路径**，
/// 于是 `/usr/bin/xcodebuild` 里带着 "code"、`com.apple.codesigninghelper` 也一样。
/// 认错的代价不是显示错一个名字——续跑的按键会照着这个判断敲进另一个应用里去。
///
/// 判定顺序：
/// 1. 先剔掉 Electron 的辅助进程（真正的主进程是它的祖先，继续往上走就能碰到）；
/// 2. 有 `.app` 包名就只看包名，这样 `Visual Studio Code.app/…/Electron` 才认得出来；
/// 3. 没有包名（Linux）才退回可执行文件名，且 `code` 这种通用词要求完全相等。
#[cfg(unix)]
fn classify_terminal(comm_lower: &str) -> Option<&'static str> {
    let base = comm_lower.rsplit('/').next().unwrap_or(comm_lower);
    if HELPER_MARKERS.iter().any(|m| base.contains(m)) {
        return None;
    }

    let bundle = app_bundle_name(comm_lower);
    let hay = bundle.unwrap_or(base);

    // Terminal.app 只认它自己；`gnome-terminal-server` 之类交给下面的 Linux 规则
    if hay == "terminal" {
        return Some("Terminal");
    }
    if let Some((_, app)) = TERMINAL_PATTERNS.iter().find(|(pat, _)| hay.contains(pat)) {
        return Some(app);
    }
    if bundle.is_none() {
        // Linux 上就是纯命令名：code / code-insiders / gnome-terminal-server
        if base == "code" || base.starts_with("code-") {
            return Some("Code");
        }
        if base.ends_with("-terminal") || base.contains("terminal-server") {
            return Some("Terminal");
        }
    }
    None
}

/// 只聚焦会话所在的终端窗口/标签，不发送任何按键
///
/// 用于「点击通知跳转到会话」——这是感知层的收口动作：
/// 用户被通知拉回来后，必须一步就能站到出问题的那个终端前面。
///
/// 只是聚焦，不敲任何字符，所以不受 `auto_follow_latest` 的约束：
/// 把窗口切到前面来最坏也只是切错一个窗口，用户自己看一眼就知道。
pub async fn focus_session(session: &AgentSession, lang: &str) -> Result<String, String> {
    let i18n = I18n::from_code(lang);

    #[cfg(target_os = "macos")]
    {
        let tty = Resumer::get_tty_for_pid(session.pid);
        let terminal_app = Resumer::find_terminal_for_pid(session.pid);

        let script = match (terminal_app.as_str(), &tty) {
            ("iTerm2", Some(tty_path)) => format!(
                r#"with timeout of 8 seconds
    tell application "iTerm2"
        activate
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                repeat with aSession in sessions of aTab
                    if tty of aSession contains "{tty_path}" then
                        select aSession
                        select aWindow
                        return "matched"
                    end if
                end repeat
            end repeat
        end repeat
        return "app-only"
    end tell
end timeout"#
            ),
            ("Terminal", Some(tty_path)) => format!(
                r#"with timeout of 8 seconds
    tell application "Terminal"
        activate
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                if tty of aTab contains "{tty_path}" then
                    set selected tab of aWindow to aTab
                    set index of aWindow to 1
                    return "matched"
                end if
            end repeat
        end repeat
        return "app-only"
    end tell
end timeout"#
            ),
            ("", _) => return Err(i18n.t("focus.no_terminal").to_string()),
            (other, _) => format!(
                r#"with timeout of 8 seconds
    tell application "{}"
        activate
    end tell
end timeout
return "app-only""#,
                mac_app_display_name(other)
            ),
        };

        run_osascript(&script, &i18n)
            .await
            .map(|raw| {
                let outcome = match raw.as_str() {
                    "matched" => i18n.t("resume.matched").to_string(),
                    "app-only" => i18n.t("focus.app_only").to_string(),
                    other => i18n.tf("resume.outcome_other", &[("raw", other)]),
                };
                i18n.tf(
                    "focus.done",
                    &[("terminal", terminal_app.as_str()), ("outcome", &outcome)],
                )
            })
            .map_err(|e| i18n.tf("focus.failed", &[("detail", &e)]))
    }

    #[cfg(target_os = "linux")]
    {
        match Resumer::find_x11_window_for_pid(session.pid) {
            Some(wid) => {
                run_with_timeout("xdotool", &["windowactivate", "--sync", &wid], 10, &i18n).await?;
                Ok(i18n.tf("focus.done_simple", &[("outcome", &wid)]))
            }
            None => Err(i18n.t("focus.no_window").to_string()),
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ps_script = format!(
            r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinFocus {{
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
}}
"@
$proc = Get-Process -Id {} -ErrorAction SilentlyContinue
if (-not $proc -or $proc.MainWindowHandle -eq [IntPtr]::Zero) {{
    Write-Output "NO_WINDOW"; exit 1
}}
[WinFocus]::ShowWindow($proc.MainWindowHandle, 9)
[WinFocus]::SetForegroundWindow($proc.MainWindowHandle)
Write-Output "FOCUSED"
"#,
            session.pid
        );
        let output = run_with_timeout(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &ps_script],
            15,
            &i18n,
        )
        .await?;
        if output.status.success() {
            Ok(i18n.tf(
                "focus.done_simple",
                &[("outcome", i18n.t("resume.matched"))],
            ))
        } else {
            Err(i18n.tf("resume.no_window", &[("pid", &session.pid.to_string())]))
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = session;
        Err(i18n.t("focus.unsupported").to_string())
    }
}

impl Resumer {
    pub fn new(config: AppConfig) -> Self {
        let i18n = I18n::from_code(&config.language);
        Self { config, i18n }
    }

    /// 用户是否明确允许「定位不到窗口时也照敲」
    ///
    /// 默认关。这个开关是整个续跑层唯一的盲敲授权：关着的时候，
    /// 任何一条「不确定字符会落到哪儿」的路径都必须放弃，而不是赌一把。
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    fn allow_blind(&self) -> bool {
        self.config.auto_follow_latest
    }

    /// 把 AppleScript / PowerShell 返回的结果码翻成人话
    ///
    /// 只有 macOS 和 Windows 会返回结果码：Linux 那边 xdotool / ydotool 的两条路
    /// 各自直接给出完整文案，没有中间码。cfg 必须跟调用点严格对齐——多挂一个平台，
    /// 那个平台的 `-D warnings` 就会因为 dead_code 而红。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn outcome_text(&self, raw: &str) -> String {
        match raw {
            "matched" | "vscode-window" => self.i18n.t("resume.matched").to_string(),
            "fallback" | "generic" | "warp" | "no-project-match" => {
                self.i18n.t("resume.followed").to_string()
            }
            other => self.i18n.tf("resume.outcome_other", &[("raw", other)]),
        }
    }

    /// 执行续跑
    /// `use_goal_prompt`: 是否使用 Goal 专用提示词（检测到活跃 goal 时为 true）
    ///
    /// 通道优先级：**先 tmux/screen，再 GUI 终端**。
    /// 复用器那条路按 pane id 寻址、不经输入法、不需要前台窗口，
    /// 确定性比任何 AppleScript / SendKeys / xdotool 路径都高一档，
    /// 所以只要认得出来就用它，认不出来才退回到窗口定位。
    pub async fn resume(
        &self,
        session: &AgentSession,
        use_goal_prompt: bool,
    ) -> Result<String, String> {
        let prompt = if use_goal_prompt {
            &self.config.goal_resume_prompt
        } else {
            &self.config.resume_prompt
        };

        if let Some(result) = self.try_resume_mux(session, prompt).await {
            return result;
        }

        #[cfg(target_os = "macos")]
        {
            self.resume_macos(session, prompt).await
        }

        #[cfg(target_os = "windows")]
        {
            self.resume_windows(session, prompt).await
        }

        #[cfg(target_os = "linux")]
        {
            self.resume_linux(session, prompt).await
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = (session, prompt);
            Err(self.i18n.t("resume.unsupported").to_string())
        }
    }

    /// 投递 **并核验**：这是外面该用的那个入口
    ///
    /// [`Self::resume`] 只回答「脚本跑通了没有」，这个方法回答「会话真的动了没有」。
    /// 差别不是措辞：整条自动续跑链此前是开环的——把按键发出去，从不看世界有没有变，
    /// 因此把「敲错窗口」「权限掉了」「输入法吃字」一律记成成功，
    /// 计数照加、上限照撞，最后自己把自己关掉。
    ///
    /// 做法很土但正好够用：投递前记下会话记录文件的指纹，投递后盯
    /// [`VERIFY_WINDOW_SECS`] 秒。文件长了 = 落地；没长 = 没落地；没有文件 = 核验不了。
    ///
    /// 返回 `(结论, 给人看的一句话)`。第二个值仍然是脚本自己的说法（命中了哪个窗口 /
    /// 报了什么错），核验结论不会把它盖掉——排查的时候两个都要。
    pub async fn resume_verified(
        &self,
        session: &AgentSession,
        use_goal_prompt: bool,
    ) -> (ResumeOutcome, String) {
        let before = activity_fingerprint(session);

        let detail = match self.resume(session, use_goal_prompt).await {
            Ok(msg) => msg,
            Err(e) => return (ResumeOutcome::Failed, e),
        };

        // 没有记录文件就别装作核验过了：这一步的价值全在诚实上
        let Some(before) = before else {
            return (ResumeOutcome::Unverifiable, detail);
        };

        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(VERIFY_WINDOW_SECS);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(VERIFY_POLL_MS)).await;
            if activity_fingerprint(session).is_some_and(|now| now != before) {
                return (ResumeOutcome::Landed, detail);
            }
        }
        (ResumeOutcome::Silent, detail)
    }

    /// 尝试通过 tmux/screen 投递；`None` = 这个会话不在复用器里，请走别的路
    ///
    /// 返回 `Some(Err(..))` 只在「认出了复用器但投递失败」时发生——那种情况不该
    /// 再退回去盲敲 GUI 窗口：既然已经确定会话在这个 pane 里，敲到别处只会更糟。
    async fn try_resume_mux(
        &self,
        session: &AgentSession,
        prompt: &str,
    ) -> Option<Result<String, String>> {
        let target = mux_target_for_pid(session.pid)?;

        // screen 只能投给「该 session 当前选中的 window」，属于窗口级不确定，
        // 跟其它盲敲路径同样的门槛
        if !target.is_exact() && !self.allow_blind_any() {
            return Some(Err(self.i18n.t("resume.blind_refused").to_string()));
        }

        tracing::info!(
            "[Resumer] 会话 {} → {} 目标 {}",
            session.id,
            target.tool,
            target.target
        );

        for (program, args) in mux_commands(&target, prompt) {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            match run_with_timeout(&program, &argv, 10, &self.i18n).await {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    return Some(Err(self.i18n.tf(
                        "resume.mux_failed",
                        &[("tool", target.tool), ("detail", &stderr)],
                    )));
                }
                Err(e) => {
                    return Some(Err(self.i18n.tf(
                        "resume.mux_failed",
                        &[("tool", target.tool), ("detail", &e)],
                    )))
                }
            }
        }

        Some(Ok(self.i18n.tf(
            "resume.sent_mux",
            &[("tool", target.tool), ("target", &target.target)],
        )))
    }

    /// 跟 [`Self::allow_blind`] 同一个开关，但不带平台 cfg
    ///
    /// 复用器那条路在所有 unix 上都可能走到，`allow_blind` 的 cfg 只覆盖三大平台，
    /// 这里单开一个入口，免得为了共用而把 cfg 放宽到跟调用点不匹配
    /// （那正是之前两次 CI 变红的原因）。
    fn allow_blind_any(&self) -> bool {
        self.config.auto_follow_latest
    }

    /// macOS: 通过 TTY 精确定位终端窗口并发送续跑指令
    #[cfg(target_os = "macos")]
    async fn resume_macos(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        // 1. 获取目标进程的 TTY
        let tty = Self::get_tty_for_pid(session.pid);
        // 2. 识别终端应用
        let terminal_app = Self::find_terminal_for_pid(session.pid);
        // 3. 获取工作目录名（用于 IDE 窗口标题匹配）
        let project_name = project_name_of(&session.working_dir).to_string();

        tracing::info!(
            "[Resumer] 会话 {} → 终端: {}, TTY: {:?}, 项目: {}",
            session.id,
            terminal_app,
            tty,
            project_name
        );

        // 4. 根据终端类型生成精确的 AppleScript
        let Some(script) = self.macos_script(&terminal_app, tty.as_deref(), &project_name, prompt)
        else {
            // 定位不到又没开盲敲：到此为止，连 osascript 都不启动
            return Err(self.i18n.t("resume.blind_refused").to_string());
        };

        // 5. 先问权限，再动手
        //
        // 这是「点了续跑，窗口跳过来了，然后什么都没发生」的根因所在：
        // 脚本前半段（activate / 选标签）不需要任何授权，照样成功；后半段的
        // `keystroke` 归「辅助功能」管，没授权就抛 -1719。用户看到的就是
        // 干净的一次跳转加一片安静。
        //
        // 更容易踩的是：**这条授权在应用更新后会失效**。TCC 记的是代码签名，
        // 换了二进制系统就不认了——设置里那个勾看着还在，实际已经不生效。
        //
        // 所以在跳窗口之前就查一次：没权限就别跳了，直接告诉用户去哪儿点。
        if script.contains("System Events") && !accessibility_granted(&self.i18n).await {
            return Err(self
                .i18n
                .t(accessibility_hint(
                    &self.i18n,
                    "resume.needs_accessibility",
                    "resume.needs_accessibility_adhoc",
                )
                .await)
                .to_string());
        }

        match run_osascript(&script, &self.i18n).await {
            // 脚本自己判断出「不知道该敲哪儿」，回传 refused
            Ok(raw) if raw == "refused" => Err(self.i18n.t("resume.blind_refused").to_string()),
            // 认出来是哪个应用，但那个应用已经退了（窗口标题匹配这条路才可能出现）
            Ok(raw) if raw == "no-app" => Err(self.i18n.t("resume.app_not_running").to_string()),
            Ok(raw) => {
                let terminal = if terminal_app.is_empty() {
                    self.i18n.t("resume.frontmost_app")
                } else {
                    terminal_app.as_str()
                };
                let tty_text = tty
                    .as_deref()
                    .unwrap_or_else(|| self.i18n.t("resume.tty_unknown"));
                Ok(self.i18n.tf(
                    "resume.sent",
                    &[
                        ("terminal", terminal),
                        ("outcome", &self.outcome_text(&raw)),
                        ("tty", tty_text),
                    ],
                ))
            }
            // 权限查询有可能被系统缓存骗过（授权刚被撤销、查询还答 true），
            // 所以真敲下去再撞上 -1719 时，同样翻译成那句能照着做的话，
            // 而不是把 AppleScript 的英文原文糊到界面上
            Err(stderr) if is_accessibility_error(&stderr) => Err(self
                .i18n
                .t(accessibility_hint(
                    &self.i18n,
                    "resume.needs_accessibility",
                    "resume.needs_accessibility_adhoc",
                )
                .await)
                .to_string()),
            Err(stderr) => Err(self.i18n.tf("resume.script_failed", &[("detail", &stderr)])),
        }
    }

    /// 生成投递提示词的 AppleScript；`None` = 定位不到且没有盲敲授权，一个字都别敲
    ///
    /// 每个分支都得先回答一个问题：**这串字符会落到哪儿？**
    /// 答得上来（TTY 对上了、窗口标题对上了）才敲；答不上来的一律交给
    /// `auto_follow_latest` 定夺，默认是放弃并告诉用户为什么。
    ///
    /// 还有第二个问题：**这串字符会不会被输入法改写？** 只有 iTerm2 的
    /// `write text` 是直接写伪终端的，其余分支都要走 `System Events`，
    /// 那条路必须用剪贴板粘贴，见 [`stage_clipboard`]。
    ///
    /// 抽成纯函数是为了能测：AppleScript 的语法错误只有真去编译一次才看得见，
    /// 而真去编译不能顺带把字敲进用户的终端。
    #[cfg(target_os = "macos")]
    fn macos_script(
        &self,
        terminal_app: &str,
        tty: Option<&str>,
        project_name: &str,
        prompt: &str,
    ) -> Option<String> {
        let escaped_prompt = prompt
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        let allow_blind = self.allow_blind();
        let stage = stage_clipboard(&escaped_prompt);
        let script = match (terminal_app, tty) {
            // iTerm2: 遍历所有 session，按 TTY 精确匹配
            // （`write text` 直接写入伪终端，对前台 TUI 有效；仍加 timeout 兜底防挂起）
            ("iTerm2", Some(tty_path)) => {
                // 遍历完都没对上 TTY，就说明这个会话不在 iTerm2 里（或者标签已经关了）。
                // 老代码在这里往 `current session` 里写——那正是把字敲进别人窗口的路径。
                let fallback = if allow_blind {
                    format!(
                        r#"        tell current session of current window
            write text "{escaped_prompt}"
        end tell
        return "fallback""#
                    )
                } else {
                    r#"        return "refused""#.to_string()
                };
                format!(
                    r#"with timeout of 8 seconds
    tell application "iTerm2"
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                repeat with aSession in sessions of aTab
                    if tty of aSession contains "{tty_path}" then
                        select aSession
                        tell aSession
                            write text "{escaped_prompt}"
                        end tell
                        return "matched"
                    end if
                end repeat
            end repeat
        end repeat
{fallback}
    end tell
end timeout"#
                )
            }

            // Terminal.app: 遍历所有 tab，按 TTY 精确匹配后聚焦，再用 System Events 键入
            //
            // 不能用 `do script ... in aTab`：当标签里有前台 TUI（Claude Code 等）在跑时，
            // `do script` 会等待 shell 提示符而挂起，直到 AppleEvent 默认 120s 超时（-1712）；
            // 即便返回也是把文本喂给 shell 而不是 Agent 的输入框。
            // 这里只用 Terminal 做「定位 + 聚焦」，键入交给 System Events，
            // 并用 `with timeout of 8 seconds` 把单个事件的等待上限压到 8s。
            //
            // 关键的一行是 `if matched is "unmatched" and allowBlind is false then return "refused"`：
            // 老脚本无论 TTY 有没有对上都会走到 keystroke，等于对着最前面的 Terminal 窗口盲敲。
            ("Terminal", Some(tty_path)) => format!(
                r#"set matched to "unmatched"
set allowBlind to {allow_blind}
with timeout of 8 seconds
    tell application "Terminal"
        activate
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                if tty of aTab contains "{tty_path}" then
                    set selected tab of aWindow to aTab
                    set index of aWindow to 1
                    set matched to "matched"
                    exit repeat
                end if
            end repeat
            if matched is "matched" then exit repeat
        end repeat
    end tell
end timeout
if matched is "unmatched" and allowBlind is false then return "refused"
{stage}
delay 0.4
with timeout of 8 seconds
    tell application "System Events"
        tell process "Terminal"
            set frontmost to true
            delay 0.2
            {PASTE_KEYS}
        end tell
    end tell
end timeout
{RESTORE_CLIPBOARD}
if matched is "unmatched" then return "fallback"
return matched"#
            ),

            // VS Code / Cursor / Windsurf / JetBrains 全家桶：靠窗口标题匹配项目名
            (app, _) if TITLE_MATCHED_APPS.contains(&app) => {
                self.title_matched_script(app, project_name, &escaped_prompt)
            }

            // Warp 没有可用的脚本接口：只能对着前台的 Warp 敲，
            // 定位不到具体是哪个标签，所以必须先拿到盲敲授权
            ("Warp", Some(_)) if allow_blind => format!(
                r#"{stage}
tell application "Warp"
    activate
end tell
delay 0.5
tell application "System Events"
    tell process "Warp"
        set frontmost to true
        delay 0.3
        {PASTE_KEYS}
    end tell
end tell
{RESTORE_CLIPBOARD}
return "warp""#
            ),

            // 通用回退：连是哪个终端都认不出来，字符会落进当时最前面的任何窗口
            _ if allow_blind => format!(
                r#"{stage}
tell application "System Events"
    set frontApp to name of first application process whose frontmost is true
end tell
tell application frontApp
    activate
end tell
delay 0.5
tell application "System Events"
    {PASTE_KEYS}
end tell
{RESTORE_CLIPBOARD}
return "generic""#
            ),

            // 定位不到又没开盲敲：一个字都不敲，连 osascript 都不启动
            _ => return None,
        };
        Some(script)
    }

    /// IDE / 编辑器内置终端的定位脚本（VS Code 系 + JetBrains 系）
    ///
    /// 通过窗口标题包含项目名来定位正确的窗口。这里有四处跟旧实现不同：
    ///
    /// 1. **不再发 `Ctrl-C`**。旧脚本键入前先来一下 Ctrl-C，本意是打断当前命令，
    ///    但焦点若在编辑器里，Ctrl-C 是复制——紧接着的提示词就被敲进了源文件。
    /// 2. **标题匹配不上就不敲**。窗口都定位不到时，字符会落进当时聚焦的任何面板；
    ///    没有盲敲授权就返回 `refused`。
    /// 3. **用剪贴板粘贴而不是逐字合成按键**，理由见 [`stage_clipboard`]。
    /// 4. **按进程名前缀找应用，而不是写死应用名**。`tell application "PyCharm"`
    ///    在装的是 PyCharm CE 的机器上直接报 -1728；VS Code Insiders、
    ///    IntelliJ IDEA Ultimate 同理。改成在 System Events 里找
    ///    「名字包含这个词的应用进程」，一条脚本覆盖所有变体。
    ///
    /// 仍然存在的不确定性要说明白：即使窗口对上了，我们也只能确认「是这个窗口」，
    /// 没法确认「焦点在集成终端而不是编辑器里」——这些应用的可访问性树不暴露这个。
    /// 所以 IDE 这条路径天生比 iTerm2 / Terminal 弱一档。
    #[cfg(target_os = "macos")]
    fn title_matched_script(&self, app_hint: &str, project_name: &str, prompt: &str) -> String {
        let allow_blind = self.allow_blind();
        let stage = stage_clipboard(prompt);

        if project_name.is_empty() {
            // 连项目名都没有，窗口无从匹配
            if !allow_blind {
                return r#"return "refused""#.to_string();
            }
            return format!(
                r#"tell application "System Events"
    set candidates to (every application process whose name contains "{app_hint}")
    if candidates is {{}} then return "no-app"
    set frontmost of (item 1 of candidates) to true
end tell
delay 0.5
{stage}
tell application "System Events"
    delay 0.2
    {PASTE_KEYS}
end tell
{RESTORE_CLIPBOARD}
return "no-project-match""#
            );
        }

        // 遍历窗口，找到标题包含项目名的窗口并置顶
        format!(
            r#"set matched to "unmatched"
set allowBlind to {allow_blind}
tell application "System Events"
    set candidates to (every application process whose name contains "{app_hint}")
    if candidates is {{}} then return "no-app"
    set targetProc to item 1 of candidates
    set frontmost of targetProc to true
    delay 0.5
    repeat with w in (every window of targetProc)
        if name of w contains "{project_name}" then
            perform action "AXRaise" of w
            set matched to "matched"
            delay 0.3
            exit repeat
        end if
    end repeat
end tell
if matched is "unmatched" and allowBlind is false then return "refused"
{stage}
tell application "System Events"
    delay 0.2
    {PASTE_KEYS}
end tell
{RESTORE_CLIPBOARD}
if matched is "unmatched" then return "no-project-match"
return "vscode-window""#
        )
    }

    /// 生成「只定位、不投递」的 AppleScript；`None` = 连是哪个终端都认不出
    ///
    /// 与 [`Self::macos_script`] 一一对应，但每个分支都砂掉了投递动作：
    /// iTerm2 不 `write text`、Terminal / IDE 不走剪贴板粘贴。纯只读探测，
    /// 所以连 `activate` 都不调——演练不该把用户的窗口焦点抢走。
    #[cfg(target_os = "macos")]
    fn macos_locate_script(
        &self,
        terminal_app: &str,
        tty: Option<&str>,
        project_name: &str,
    ) -> Option<String> {
        let script = match (terminal_app, tty) {
            // iTerm2: 遍历 session 比对 TTY（只定位，不 write text）
            ("iTerm2", Some(tty_path)) => format!(
                r#"with timeout of 8 seconds
    tell application "iTerm2"
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                repeat with aSession in sessions of aTab
                    if tty of aSession contains "{tty_path}" then
                        return "matched"
                    end if
                end repeat
            end repeat
        end repeat
        return "refused"
    end tell
end timeout"#
            ),
            // Terminal: 遍历 tab 比对 TTY（只定位，不聚焦不粘贴）
            ("Terminal", Some(tty_path)) => format!(
                r#"with timeout of 8 seconds
    tell application "Terminal"
        repeat with aWindow in windows
            repeat with aTab in tabs of aWindow
                if tty of aTab contains "{tty_path}" then
                    return "matched"
                end if
            end repeat
        end repeat
        return "refused"
    end tell
end timeout"#
            ),
            // VS Code 系 / JetBrains 系：靠窗口标题匹配项目名
            (app, _) if TITLE_MATCHED_APPS.contains(&app) => {
                self.title_locate_script(app, project_name)
            }
            // 其余：认不出是哪个终端，连 osascript 都不启动
            _ => return None,
        };
        Some(script)
    }

    /// IDE / 编辑器内置终端的「只定位」脚本（[`Self::title_matched_script`] 的演练版）
    ///
    /// 遍历窗口看标题里有没有项目名，有就回报 `vscode-window`；没有 `AXRaise`、
    /// 没有粘贴，纯查询。`no-app` 表示这个应用已经不在运行了。
    #[cfg(target_os = "macos")]
    fn title_locate_script(&self, app_hint: &str, project_name: &str) -> String {
        // 连项目名都没有，窗口无从匹配；至少还能报一下应用在不在
        if project_name.is_empty() {
            return format!(
                r#"tell application "System Events"
    if (every application process whose name contains "{app_hint}") is {{}} then return "no-app"
end tell
return "refused""#
            );
        }
        format!(
            r#"tell application "System Events"
    set candidates to (every application process whose name contains "{app_hint}")
    if candidates is {{}} then return "no-app"
    repeat with w in (every window of item 1 of candidates)
        if name of w contains "{project_name}" then
            return "vscode-window"
        end if
    end repeat
end tell
return "refused""#
        )
    }

    /// 获取进程所在的 TTY 设备路径
    #[cfg(unix)]
    pub fn get_tty_for_pid(pid: u32) -> Option<String> {
        let output = Command::new("ps")
            .arg("-o")
            .arg("tty=")
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .ok()?;

        let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if tty.is_empty() || tty == "??" || tty == "-" {
            // 进程没有直接 TTY，尝试查找父进程的 TTY
            return Self::get_parent_tty(pid);
        }
        Some(format!("/dev/{}", tty))
    }

    /// 向上查找父进程链的 TTY（claude 可能作为子进程没有自己的 TTY）
    #[cfg(unix)]
    pub fn get_parent_tty(pid: u32) -> Option<String> {
        let mut current_pid = pid;
        // 最多向上查找 5 层
        for _ in 0..5 {
            let output = Command::new("ps")
                .arg("-o")
                .arg("ppid=")
                .arg("-p")
                .arg(current_pid.to_string())
                .output()
                .ok()?;

            let ppid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let ppid: u32 = ppid_str.parse().ok()?;
            if ppid <= 1 {
                break;
            }

            // 检查父进程的 TTY
            let tty_out = Command::new("ps")
                .arg("-o")
                .arg("tty=")
                .arg("-p")
                .arg(ppid.to_string())
                .output()
                .ok()?;

            let tty = String::from_utf8_lossy(&tty_out.stdout).trim().to_string();
            if !tty.is_empty() && tty != "??" && tty != "-" {
                return Some(format!("/dev/{}", tty));
            }

            current_pid = ppid;
        }
        None
    }

    /// 通过 PID 查找所属的终端应用
    #[cfg(unix)]
    pub fn find_terminal_for_pid(pid: u32) -> String {
        let mut current_pid = pid;
        // 向上遍历进程树（最多 8 层），找到终端应用
        for _ in 0..8 {
            let output = Command::new("ps")
                .arg("-o")
                .arg("ppid=,comm=")
                .arg("-p")
                .arg(current_pid.to_string())
                .output();

            let (ppid, comm) = match output {
                Ok(out) => {
                    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let parts: Vec<&str> = line.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        let ppid: u32 = match parts[0].trim().parse() {
                            Ok(p) => p,
                            Err(_) => break,
                        };
                        (ppid, parts[1].trim().to_lowercase())
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            };

            if let Some(app) = classify_terminal(&comm) {
                return app.to_string();
            }

            if ppid <= 1 {
                break;
            }
            current_pid = ppid;
        }

        String::new()
    }

    /// Windows: 通过 PowerShell + Win32 API 定位终端窗口并发送续跑指令
    ///
    /// 策略：
    /// 1. 通过 PID 获取进程所属的控制台窗口句柄
    /// 2. 使用 SetForegroundWindow 激活目标窗口
    /// 3. 把提示词放进剪贴板，用 `Ctrl+V` 粘贴 + 回车，最后还原剪贴板
    ///
    /// 第 3 步以前是 `SendKeys::SendWait("整段提示词")`。SendKeys 是按当前键盘
    /// 布局逐字合成按键的，中文（以及任何非 ASCII）根本没有对应的键，中文输入法
    /// 开着时还会把按键重新解释一遍——macOS 上就是这么把
    /// 「你之前有一个活跃的 goal 目标」敲成「啊啊啊啊啊啊啊啊啊goal啊啊啊啊啊啊」的。
    /// 剪贴板不过键盘布局也不过输入法，所以这里只合成 `Ctrl+V` 这一个 ASCII 组合键。
    #[cfg(target_os = "windows")]
    async fn resume_windows(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        let project_name = project_name_of(&session.working_dir);
        let ps_script =
            Self::windows_resume_script(session.pid, prompt, project_name, self.allow_blind());

        tracing::info!(
            "[Resumer] 会话 {} → PID {}, 项目: {}",
            session.id,
            session.pid,
            project_name
        );

        let output = run_with_timeout(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &ps_script],
            25,
            &self.i18n,
        )
        .await?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // 脚本自己判断出「不知道该敲哪儿」时是正常退出的，所以先看内容再看退出码
        let raw = stdout.lines().last().unwrap_or_default().trim();
        match raw {
            "REFUSED" => return Err(self.i18n.t("resume.blind_refused").to_string()),
            "NO_FOCUS" => return Err(self.i18n.t("resume.focus_failed").to_string()),
            "NO_WINDOW" => {
                return Err(self
                    .i18n
                    .tf("resume.no_window", &[("pid", &session.pid.to_string())]))
            }
            _ => {}
        }

        if output.status.success() {
            Ok(self.i18n.tf(
                "resume.sent_simple",
                &[("outcome", &self.outcome_text(raw))],
            ))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(self.i18n.tf("resume.script_failed", &[("detail", &stderr)]))
        }
    }

    /// Windows 续跑脚本的生成器
    ///
    /// **故意不加 `#[cfg(target_os = "windows")]`**：脚本内容是纯字符串拼接，
    /// 不依赖任何 Windows API，放开 cfg 之后每个平台的 CI 都会编它、测它。
    /// 上一版的转义 bug 之所以能活那么久，就是因为只有 Windows 那一个 job 看得见它。
    ///
    /// 三件事跟旧版不同，每件都对应一个真会咬人的场景：
    ///
    /// 1. **沿父进程链往上找窗口，而不是只看一层。** agent 是控制台程序，
    ///    自己从来没有窗口；窗口属于宿主。旧版只在「进程根本不存在」时才去看父进程，
    ///    于是「进程活着但没窗口」——也就是所有正常情况——一律返回 `NO_WINDOW`。
    ///    cmd.exe 直接开的会有 conhost 窗口，Windows Terminal / VS Code 里开的
    ///    要往上走三四层才碰得到窗口。
    /// 2. **多标签宿主要核标题。** conhost 一个窗口就是一个控制台，认到窗口就等于认到会话；
    ///    Windows Terminal / VS Code / JetBrains 一个窗口下面挂着好几个标签，
    ///    认到窗口只能保证「应用对了」。所以这类宿主要求窗口标题里出现项目名，
    ///    对不上就按 [`Self::allow_blind`] 定夺。
    /// 3. **粘贴前确认目标窗口真的到了前台。** `SetForegroundWindow` 在后台进程里
    ///    经常被系统直接拒掉（返回 false 也不报错），而 `SendKeys` 打的是**当时的**
    ///    前台窗口。不核一下就等于把提示词敲进用户正在看的任何一个窗口。
    pub fn windows_resume_script(
        pid: u32,
        prompt: &str,
        project_name: &str,
        allow_blind: bool,
    ) -> String {
        // PowerShell 单引号字符串：不做任何变量展开，只需把单引号翻倍。
        // 双引号字符串会把提示词里的 `$` 当变量展开，那是另一种「敲出乱码」。
        let ps_literal = prompt.replace('\'', "''");
        let ps_project = project_name.replace('\'', "''");
        let ps_blind = if allow_blind { "$true" } else { "$false" };
        let multi_tab = WINDOWS_MULTI_TAB_HOSTS
            .iter()
            .map(|h| format!("'{h}'"))
            .collect::<Vec<_>>()
            .join(",");

        format!(
            r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinAPI {{
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}}
"@

$target = {pid}
$project = '{ps_project}'
$allowBlind = {ps_blind}

# 沿父进程链往上找第一个带窗口的祖先：agent 自己是控制台程序，窗口属于宿主
$hostProc = $null
$cur = $target
for ($i = 0; $i -lt 8; $i++) {{
    $p = Get-Process -Id $cur -ErrorAction SilentlyContinue
    if ($p -and $p.MainWindowHandle -ne [IntPtr]::Zero) {{ $hostProc = $p; break }}
    $ci = Get-CimInstance Win32_Process -Filter "ProcessId=$cur" -ErrorAction SilentlyContinue
    if (-not $ci) {{ break }}
    $cur = [int]$ci.ParentProcessId
    # 0=Idle, 4=System：再往上就没有用户进程了
    if ($cur -le 4) {{ break }}
}}

if (-not $hostProc) {{
    Write-Output "NO_WINDOW"
    exit 1
}}

# 一个窗口挂多个标签的宿主：认到窗口不等于认到标签，要核标题
$multiTab = @({multi_tab})
$hostName = $hostProc.ProcessName.ToLower()
$title = [string]$hostProc.MainWindowTitle
$located = "window"
if ($multiTab -contains $hostName) {{
    if ($project -ne '' -and $title.ToLower().Contains($project.ToLower())) {{
        $located = "title"
    }} else {{
        $located = "unlocated"
    }}
}}
if ($located -eq "unlocated" -and -not $allowBlind) {{
    Write-Output "REFUSED"
    exit 0
}}

$hwnd = $hostProc.MainWindowHandle
[void][WinAPI]::ShowWindow($hwnd, 9)  # SW_RESTORE
Start-Sleep -Milliseconds 300
[void][WinAPI]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 500

# SendKeys 打的是「当时的前台窗口」，而 SetForegroundWindow 在后台进程里经常被拒。
# 没切过去就别敲——否则这段提示词会落进用户正在看的那个窗口
if ([WinAPI]::GetForegroundWindow() -ne $hwnd) {{
    Write-Output "NO_FOCUS"
    exit 0
}}

# 借一下剪贴板：粘贴不过键盘布局，中文才能原样落地
$saved = $null
try {{ $saved = Get-Clipboard -Raw }} catch {{}}
Set-Clipboard -Value '{ps_literal}'

Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait("^v")
Start-Sleep -Milliseconds 300
[System.Windows.Forms.SendKeys]::SendWait("{{ENTER}}")

# 还给用户；粘贴是异步的，等一下再改回去
Start-Sleep -Milliseconds 500
if ($null -ne $saved) {{ try {{ Set-Clipboard -Value $saved }} catch {{}} }}

if ($located -eq "unlocated") {{ Write-Output "fallback" }} else {{ Write-Output "matched" }}
"#
        )
    }

    /// Windows 定位演练脚本的生成器
    ///
    /// **故意不加 `#[cfg(target_os = "windows")]`**：与 [`Self::windows_resume_script`]
    /// 同理——纯字符串拼接，不碰 Windows API，放开 cfg 之后每个平台的 CI 都能编它、测它。
    ///
    /// 它跟真续跑脚本共用同一套定位逻辑（沿父进程链找窗口 + 多标签宿主核标题），
    /// 但**砂掉了一切副作用**：不 `ShowWindow` / `SetForegroundWindow`，不碰剪贴板，
    /// 不 `SendKeys`。输出两行：`HOST=<进程名>` 供诊断，和一个结果码：
    /// `EXACT`（cmd / conhost 单窗口）/ `WINDOW`（多标签标题对上）/
    /// `BLIND`（标题没对但开了盲敲）/ `REFUSED`（标题没对且没盲敲）/ `NO_WINDOW`。
    pub fn windows_locate_script(pid: u32, project_name: &str, allow_blind: bool) -> String {
        let ps_project = project_name.replace('\'', "''");
        let ps_blind = if allow_blind { "$true" } else { "$false" };
        let multi_tab = WINDOWS_MULTI_TAB_HOSTS
            .iter()
            .map(|h| format!("'{h}'"))
            .collect::<Vec<_>>()
            .join(",");

        format!(
            r#"
$target = {pid}
$project = '{ps_project}'
$allowBlind = {ps_blind}

# 沿父进程链往上找第一个带窗口的祖先（与真续跑脚本同一条链路）
$hostProc = $null
$cur = $target
for ($i = 0; $i -lt 8; $i++) {{
    $p = Get-Process -Id $cur -ErrorAction SilentlyContinue
    if ($p -and $p.MainWindowHandle -ne [IntPtr]::Zero) {{ $hostProc = $p; break }}
    $ci = Get-CimInstance Win32_Process -Filter "ProcessId=$cur" -ErrorAction SilentlyContinue
    if (-not $ci) {{ break }}
    $cur = [int]$ci.ParentProcessId
    if ($cur -le 4) {{ break }}
}}

if (-not $hostProc) {{
    Write-Output "NO_WINDOW"
    exit 0
}}

$hostName = $hostProc.ProcessName.ToLower()
$title = [string]$hostProc.MainWindowTitle
Write-Output "HOST=$($hostProc.ProcessName)"

# 多标签宿主：认到窗口不等于认到标签，要核标题（但不前台化、不敲字）
$multiTab = @({multi_tab})
if ($multiTab -contains $hostName) {{
    if ($project -ne '' -and $title.ToLower().Contains($project.ToLower())) {{
        Write-Output "WINDOW"
    }} elseif ($allowBlind) {{
        Write-Output "BLIND"
    }} else {{
        Write-Output "REFUSED"
    }}
}} else {{
    # cmd / conhost 这类单窗口宿主：一个窗口就是一个会话
    Write-Output "EXACT"
}}
"#
        )
    }

    /// Linux: 通过 xdotool 定位终端窗口并发送续跑指令
    ///
    /// 策略：
    /// 1. 使用 xdotool search --pid 查找目标进程窗口
    /// 2. 如果找不到，向上遍历父进程
    /// 3. windowactivate + type + Return
    /// 4. Wayland（拿不到窗口）时才回退到 ydotool，而 ydotool 是对着当前焦点盲敲的，
    ///    所以那条路必须先有 `auto_follow_latest` 授权
    #[cfg(target_os = "linux")]
    async fn resume_linux(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        let pid = session.pid;

        // 尝试通过 xdotool 查找窗口
        let window_id = Self::find_x11_window_for_pid(pid);

        match window_id {
            Some(wid) => {
                // 激活窗口
                let _ = run_with_timeout(
                    "xdotool",
                    &["windowactivate", "--sync", &wid],
                    10,
                    &self.i18n,
                )
                .await;

                tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;

                // 输入提示词
                let type_result = run_with_timeout(
                    "xdotool",
                    &["type", "--clearmodifiers", "--delay", "20", prompt],
                    20,
                    &self.i18n,
                )
                .await?;

                if !type_result.status.success() {
                    return Err(self.i18n.tf("resume.tool_failed", &[("tool", "xdotool")]));
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                // 发送回车
                let _ = run_with_timeout("xdotool", &["key", "Return"], 10, &self.i18n).await;

                Ok(self.i18n.tf(
                    "resume.sent_window",
                    &[("tool", "xdotool"), ("window", &wid)],
                ))
            }
            None => {
                // 拿不到窗口（Wayland，或者进程根本没有窗口）：ydotool 只能敲给当前焦点
                if !self.allow_blind() {
                    return Err(self.i18n.t("resume.blind_refused").to_string());
                }
                self.resume_linux_ydotool(prompt).await
            }
        }
    }

    /// Linux X11: 通过 PID 查找窗口 ID（向上遍历父进程）
    #[cfg(target_os = "linux")]
    pub fn find_x11_window_for_pid(pid: u32) -> Option<String> {
        let mut current_pid = pid;

        for _ in 0..6 {
            // xdotool search --pid <pid>
            let output = Command::new("xdotool")
                .args(["search", "--pid", &current_pid.to_string()])
                .output()
                .ok()?;

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                // 取第一个窗口 ID
                let wid = stdout.lines().next()?.to_string();
                if !wid.is_empty() {
                    return Some(wid);
                }
            }

            // 向上查找父进程
            let ppid_output =
                std::fs::read_to_string(format!("/proc/{}/stat", current_pid)).ok()?;
            // stat 格式: pid (comm) state ppid ...
            let after_comm = ppid_output.rsplit(')').next()?;
            let fields: Vec<&str> = after_comm.split_whitespace().collect();
            // fields[0] = state, fields[1] = ppid
            let ppid: u32 = fields.get(1)?.parse().ok()?;
            if ppid <= 1 {
                break;
            }
            current_pid = ppid;
        }

        None
    }

    /// Linux Wayland: 通过 ydotool 发送按键（回退方案）
    ///
    /// 只在 [`Self::allow_blind`] 为真时才会走到这里：ydotool 敲给的是当前焦点，
    /// 定位不到窗口，所以它天生是盲敲。
    ///
    /// 中文提示词不能交给 `ydotool type`：它按固定的 US 键位表合成按键，汉字
    /// 根本没有对应键位，整段中文会被漏成空白（然后回车照发，等于什么都没续）。
    /// 所以非 ASCII 走剪贴板 + `Ctrl+Shift+V`——终端通用的粘贴键，且不过输入法。
    #[cfg(target_os = "linux")]
    async fn resume_linux_ydotool(&self, prompt: &str) -> Result<String, String> {
        if !Self::has_tool("ydotool") {
            return Err(self.i18n.t("resume.tool_missing").to_string());
        }

        if prompt.is_ascii() {
            let typed =
                run_with_timeout("ydotool", &["type", "--", prompt], 20, &self.i18n).await?;
            if !typed.status.success() {
                return Err(self.i18n.tf("resume.tool_failed", &[("tool", "ydotool")]));
            }
        } else {
            let mut paste_args = vec!["key"];
            paste_args.extend_from_slice(&YDOTOOL_PASTE_KEYS);
            Self::set_clipboard_linux(prompt, &self.i18n).await?;
            let pasted = run_with_timeout("ydotool", &paste_args, 10, &self.i18n).await?;
            if !pasted.status.success() {
                return Err(self.i18n.tf("resume.tool_failed", &[("tool", "ydotool")]));
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // ydotool key Enter (keycode 28)
        let _ = run_with_timeout("ydotool", &["key", "28:1", "28:0"], 10, &self.i18n).await;

        Ok(self.i18n.tf(
            "resume.sent_simple",
            &[("outcome", self.i18n.t("resume.followed"))],
        ))
    }

    /// 命令在不在 PATH 里
    #[cfg(target_os = "linux")]
    fn has_tool(name: &str) -> bool {
        Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// 把文本写进 Linux 剪贴板（Wayland 用 wl-copy，X11 用 xclip/xsel）
    ///
    /// 走 stdin 而不是命令行参数：提示词是用户可改的自由文本，
    /// 塞进 `sh -c` 里拼字符串就等于给自己开一个注入口子。
    #[cfg(target_os = "linux")]
    async fn set_clipboard_linux(text: &str, i18n: &I18n) -> Result<(), String> {
        use tokio::io::AsyncWriteExt;

        let Some((tool, args)) = LINUX_CLIPBOARD_TOOLS
            .into_iter()
            .find(|(tool, _)| Self::has_tool(tool))
        else {
            return Err(i18n.t("resume.clipboard_missing").to_string());
        };

        let mut child = tokio::process::Command::new(tool)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                i18n.tf(
                    "cmd.failed",
                    &[("program", tool), ("detail", &e.to_string())],
                )
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
        // wl-copy 会常驻做剪贴板服务，不能等它退出；给它一小会儿把内容接过去
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        Ok(())
    }
}

/// 写 Linux 剪贴板的候选命令，按优先级排列（Wayland 的 wl-copy 优先）
///
/// **故意不加 `#[cfg(target_os = "linux")]`**：这是纯数据，放开 cfg 之后
/// 每个平台的 CI 都能测它。Windows 那个转义 bug 活了那么久，
/// 就是因为只有一个 job 看得见它。
pub const LINUX_CLIPBOARD_TOOLS: [(&str, &[&str]); 3] = [
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];

/// `ydotool key` 版的 Ctrl+Shift+V —— 终端里通用的粘贴键
///
/// evdev 键码：29=LEFTCTRL，42=LEFTSHIFT，47=V；`:1` 按下，`:0` 松开。
/// 顺序必须是「全部按下，再反序松开」，漏一个松开事件会让修饰键卡住，
/// 之后用户敲的每个字都带着 Ctrl。
pub const YDOTOOL_PASTE_KEYS: [&str; 6] = ["29:1", "42:1", "47:1", "47:0", "42:0", "29:0"];

#[cfg(test)]
mod tests {
    use super::*;

    // ── tmux / screen 投递通道 ──
    //
    // 这条路的全部价值在于「不经过输入法、不需要窗口在前台」，所以这里
    // 盯的是两件事：提示词必须作为**独立的 argv 元素**传下去（一旦被拼进
    // shell 字符串，带引号或反引号的提示词就成了命令注入），以及回车必须
    // 是单独一次 send-keys（跟文本混在一行会被当成字面量）。

    #[test]
    fn tmux_sends_text_and_enter_separately() {
        let target = MuxTarget {
            tool: "tmux",
            target: "%7".to_string(),
            tty: "/dev/ttys004".to_string(),
        };
        let cmds = mux_commands(&target, "继续 `whoami` \"干活\"");
        assert_eq!(cmds.len(), 2, "文本和回车必须分两次发");

        let (program, args) = &cmds[0];
        assert_eq!(program, "tmux");
        // `-l` = literal，`--` 收住选项解析：以 `-` 开头的提示词不会被当成选项
        assert_eq!(args[..5], ["send-keys", "-t", "%7", "-l", "--"]);
        // 提示词是最后一个独立元素，原样不动——没有任何转义或拼接
        assert_eq!(args[5], "继续 `whoami` \"干活\"");
        assert_eq!(args.len(), 6);

        assert_eq!(cmds[1].1, ["send-keys", "-t", "%7", "Enter"]);
        assert!(target.is_exact(), "pane id 是精确寻址");
    }

    #[test]
    fn screen_stuffs_with_trailing_cr() {
        let target = MuxTarget {
            tool: "screen",
            target: "12345.pts-0.host".to_string(),
            tty: String::new(),
        };
        let cmds = mux_commands(&target, "继续");
        assert_eq!(cmds.len(), 1, "screen 的 stuff 一次就带上回车");
        assert_eq!(cmds[0].0, "screen");
        assert_eq!(
            cmds[0].1,
            ["-S", "12345.pts-0.host", "-X", "stuff", "继续\r"]
        );
        assert!(
            !target.is_exact(),
            "screen 只能投到会话当前选中的窗口，不算精确"
        );
    }

    #[test]
    fn tmux_pane_is_matched_by_tty() {
        let listing = "%0\t/dev/ttys001\n%3\t/dev/ttys004\n%9\t/dev/ttys007\n";
        assert_eq!(
            match_tmux_pane(listing, "/dev/ttys004"),
            Some("%3".to_string())
        );
        // `ps` 有时只给 `ttys004`，两边都要能对上
        assert_eq!(match_tmux_pane(listing, "ttys004"), Some("%3".to_string()));
        assert_eq!(match_tmux_pane(listing, "/dev/ttys999"), None);
        // 空 tty 绝不能匹配到第一行——那就是往陌生 pane 里敲字
        assert_eq!(match_tmux_pane(listing, ""), None);
        assert_eq!(match_tmux_pane("", "/dev/ttys004"), None);
    }

    #[test]
    fn screen_session_is_matched_by_pid() {
        let ls =
            "There are screens on:\n\t4242.pts-1.mac\t(Detached)\n\t99.pts-0.mac\t(Attached)\n";
        assert_eq!(
            match_screen_session(ls, 4242),
            Some("4242.pts-1.mac".to_string())
        );
        // 不能被前缀蒙混：424 不是 4242
        assert_eq!(match_screen_session(ls, 424), None);
        assert_eq!(match_screen_session(ls, 1), None);
    }

    // ── 辅助功能权限 ──
    //
    // 「点了续跑，窗口跳过来了，然后什么都没发生」就是这条权限缺失的样子。
    // 这两个测试守着「认得出这个错」和「哪些通道不受它影响」。

    #[test]
    fn accessibility_errors_are_recognised() {
        for stderr in [
            "execution error: System Events got an error: osascript is not allowed to send keystrokes. (-1719)",
            "execution error: Not authorized to send Apple events (-25211)",
            "System Events got an error: AgentPulse is not allowed assistive access.",
        ] {
            assert!(is_accessibility_error(stderr), "没认出来：{stderr}");
        }
        // 别把「窗口找不到」也当成权限问题——那两件事的解法完全不同
        assert!(!is_accessibility_error(
            "execution error: Can't get window 1 of process \"Code\". (-1728)"
        ));
        assert!(!is_accessibility_error(""));
    }

    /// 这两段是从真机上抄下来的：一个是本应用（临时签名），
    /// 一个是 Terminal.app（正式证书）。
    ///
    /// 差别就是「勾了为什么还是不管用」的答案：adhoc 的指定要求认的是
    /// 一个具体哈希，改一行代码重新构建就换了身份，旧授权自然失效。
    #[test]
    fn adhoc_signature_is_not_stable() {
        // 本应用现在的样子：只有 cdhash
        assert!(!signature_is_stable(
            r#"designated => cdhash H"d4493a69ce70aca1e479fdcf225a852a2e74e91f""#
        ));
        // 正式证书签过的：认名字 + 证书链
        assert!(signature_is_stable(
            r#"designated => identifier "com.apple.Terminal" and anchor apple"#
        ));
        assert!(signature_is_stable(
            r#"designated => identifier "com.agentpulse.app" and anchor apple generic and certificate leaf[subject.OU] = "ABCDE12345""#
        ));
        // 查不出来的时候上层会当成稳定，这里只保证空串不会被误判成「有证书」
        assert!(!signature_is_stable(""));
    }

    #[test]
    fn pty_channels_need_no_accessibility() {
        // 这三条直接写伪终端，没授权也照样敲得进去
        for ok in [
            "probe.channel_tmux",
            "probe.channel_screen",
            "probe.channel_iterm2",
        ] {
            assert!(!channel_needs_accessibility(ok), "{ok} 不该要权限");
        }
        // 其余全靠合成按键
        for needs in [
            "probe.channel_terminal",
            "probe.channel_ide",
            "probe.channel_frontmost",
            "probe.channel_unknown",
        ] {
            assert!(channel_needs_accessibility(needs), "{needs} 必须要权限");
        }
    }

    #[test]
    fn windows_locate_output_parses_host_and_code() {
        // 脚本先打 HOST=，再打结果码；中间可能夹空行（PowerShell 爱加）
        let (host, code) = parse_windows_locate_output("HOST=WindowsTerminal\n\nWINDOW\n");
        assert_eq!(host, "WindowsTerminal");
        assert_eq!(code, "WINDOW");

        // 一行都没有也不能 panic：拿不到就是拿不到
        let (host, code) = parse_windows_locate_output("");
        assert!(host.is_empty() && code.is_empty());
    }

    #[test]
    fn windows_locate_codes_map_to_certainty() {
        assert_eq!(windows_locate_certainty("EXACT"), "exact");
        assert_eq!(windows_locate_certainty("WINDOW"), "window");
        // BLIND 是「标题没对上但开了盲敲」：会敲，但不是定位成功，
        // 报成 window 就等于骗用户说找到了
        for code in ["BLIND", "REFUSED", "NO_WINDOW", ""] {
            assert_eq!(
                windows_locate_certainty(code),
                "none",
                "{code} 不该算定位成功"
            );
        }
    }

    // ── 终端识别 ──
    //
    // 这一层认错终端的代价不是「没续跑」，而是**把提示词敲进别的应用**。
    // 所以下面每条都是「宁可返回 None」的用例。

    #[cfg(unix)]
    #[test]
    fn helper_processes_are_never_terminals() {
        // 一个 .app 里往往还嵌着若干 Helper.app，它们没有窗口也没有 TTY
        assert_eq!(
            classify_terminal("/system/library/privateframeworks/com.apple.codesigninghelper"),
            None
        );
        assert_eq!(
            classify_terminal(
                "/applications/qoder.app/contents/frameworks/qoder helper.app/contents/macos/qoder helper"
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn bundle_name_decides_not_the_executable() {
        // macOS 上 `ps -o comm=` 给的是完整路径，可执行文件名常常叫 electron，
        // 真正能认人的是 .app 的名字
        assert_eq!(
            classify_terminal("/applications/visual studio code.app/contents/macos/electron"),
            Some("Code")
        );
        assert_eq!(
            classify_terminal("/applications/iterm.app/contents/macos/iterm2"),
            Some("iTerm2")
        );
        assert_eq!(
            classify_terminal(
                "/system/applications/utilities/terminal.app/contents/macos/terminal"
            ),
            Some("Terminal")
        );
    }
    // ── Windows 投递脚本 ──
    //
    // Windows 上一个字都验不了（这台机器上没有），所以脚本生成器故意没加 cfg，
    // 每个平台都跑下面这几条。

    fn win_script(project: &str, blind: bool) -> String {
        Resumer::windows_resume_script(4242, "继续完成刚才的任务", project, blind)
    }

    #[test]
    fn windows_walks_up_to_the_host_window() {
        // agent 是控制台程序，自己永远没有窗口：cmd.exe 直接开的会有 conhost 窗口，
        // Windows Terminal / VS Code 里开的要往上走三四层才碰得到。
        // 旧版只在「进程根本不存在」时才看父进程，于是正常情况一律 NO_WINDOW。
        let script = win_script("agent-pulse", false);
        assert!(
            script.contains("for ($i = 0; $i -lt 8; $i++)"),
            "要沿父进程链往上找"
        );
        assert!(script.contains("Win32_Process -Filter \"ProcessId=$cur\""));
        assert!(
            script.contains("if ($cur -le 4) { break }"),
            "走到 System/Idle 就该停"
        );
    }

    #[test]
    fn windows_confirms_the_window_actually_came_forward() {
        // SendKeys 打的是「当时的前台窗口」，SetForegroundWindow 在后台进程里经常被拒。
        // 不核一下，这段提示词就会落进用户正在看的窗口
        let script = win_script("agent-pulse", true);
        let focus_at = script
            .find("GetForegroundWindow() -ne $hwnd")
            .expect("要核前台窗口");
        let paste_at = script.find("SendWait(\"^v\")").unwrap();
        assert!(focus_at < paste_at, "确认前台必须在按键之前");
        // 剪贴板也一样：切不过去就别动用户的剪贴板
        let stage_at = script.find("Set-Clipboard -Value").unwrap();
        assert!(focus_at < stage_at, "确认前台之前不要碰剪贴板");
    }

    #[test]
    fn windows_multi_tab_hosts_need_a_title_match() {
        // conhost 一个窗口就是一个控制台；Windows Terminal / VS Code / IDEA
        // 一个窗口挂好几个标签，认到窗口只等于认到应用
        let script = win_script("agent-pulse", false);
        for host in ["windowsterminal", "code", "cursor", "idea64", "pycharm64"] {
            assert!(
                script.contains(&format!("'{host}'")),
                "{host} 应当算多标签宿主"
            );
        }
        assert!(
            !WINDOWS_MULTI_TAB_HOSTS.contains(&"conhost")
                && !WINDOWS_MULTI_TAB_HOSTS.contains(&"cmd"),
            "cmd / conhost 一个窗口就是一个会话，不该被要求核标题"
        );
        let refuse_at = script
            .find(r#"Write-Output "REFUSED""#)
            .expect("默认要能拒绝");
        let stage_at = script.find("Set-Clipboard -Value").unwrap();
        assert!(refuse_at < stage_at, "拒绝之前不要碰剪贴板");
    }

    #[test]
    fn windows_blind_permission_is_the_only_way_past_an_unmatched_tab() {
        assert!(win_script("agent-pulse", false).contains("$allowBlind = $false"));
        assert!(win_script("agent-pulse", true).contains("$allowBlind = $true"));
    }

    #[test]
    fn windows_prompts_go_through_the_clipboard_not_sendkeys() {
        // SendKeys 逐字合成按键，中文没有对应键位——这正是「啊啊啊啊」的来源
        let script = win_script("agent-pulse", true);
        assert!(script.contains("Set-Clipboard -Value '继续完成刚才的任务'"));
        assert!(!script.contains("SendWait(\"继续完成刚才的任务\")"));
        // 借了就要还
        let paste_at = script.find("SendWait(\"^v\")").unwrap();
        let restore_at = script.find("Set-Clipboard -Value $saved").unwrap();
        assert!(paste_at < restore_at);
    }

    #[test]
    fn windows_prompt_quotes_cannot_break_out_of_the_literal() {
        // PowerShell 单引号串不做变量展开，只需把单引号翻倍。
        // 双引号串会把 `$` 当变量，那是另一种「敲出乱码」
        let script = Resumer::windows_resume_script(1, "别用 $HOME，用 'pwd' 的结果", "p", false);
        assert!(script.contains("Set-Clipboard -Value '别用 $HOME，用 ''pwd'' 的结果'"));
    }

    #[test]
    fn project_name_survives_both_path_separators() {
        // Windows 上 cwd 是 `C:\code\agent-pulse`；只按 `/` 切会把整条路径当项目名，
        // 于是窗口标题永远对不上，每次续跑都被判成「定位不到」
        assert_eq!(project_name_of(r"C:\code\git\agent-pulse"), "agent-pulse");
        assert_eq!(
            project_name_of("/Users/sky/code/agent-pulse"),
            "agent-pulse"
        );
        assert_eq!(
            project_name_of("/Users/sky/code/agent-pulse/"),
            "agent-pulse"
        );
        assert_eq!(project_name_of(r"C:\code\agent-pulse\"), "agent-pulse");
        assert_eq!(project_name_of(""), "");
    }

    #[cfg(unix)]
    #[test]
    fn ide_terminals_are_recognized_too() {
        // IDE 内置终端是 agent 的常见落脚点，认不出来就等于不敢续
        assert_eq!(
            classify_terminal("/applications/intellij idea.app/contents/macos/idea"),
            Some("IntelliJ IDEA")
        );
        assert_eq!(
            classify_terminal("/applications/pycharm ce.app/contents/macos/pycharm"),
            Some("PyCharm")
        );
        assert_eq!(
            classify_terminal("/applications/windsurf.app/contents/macos/electron"),
            Some("Windsurf")
        );
        assert_eq!(
            classify_terminal("/applications/android studio.app/contents/macos/studio"),
            Some("Android Studio")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ide_paths_match_by_process_prefix_not_exact_app_name() {
        // 写死 `tell application "PyCharm"` 在装 PyCharm CE 的机器上直接报 -1728；
        // VS Code Insiders、IDEA Ultimate 同理。只能按「名字包含」找应用进程
        let script = resumer_with(false)
            .macos_script("PyCharm", None, "agent-pulse", "继续")
            .expect("IDE 分支要能生成脚本");
        assert!(script.contains(r#"every application process whose name contains "PyCharm""#));
        assert!(!script.contains(r#"tell application "PyCharm""#));
    }

    // TESTS_PLACEHOLDER_RESUMER

    #[cfg(unix)]
    #[test]
    fn names_that_merely_contain_code_are_not_vs_code() {
        // 老实现拿裸 contains 撞 "code"，于是这两个都会被认成 VS Code；
        // 认错之后续跑的按键就敲去了 Xcode，或者一个根本不存在的窗口
        assert_eq!(classify_terminal("/usr/bin/xcodebuild"), None);
        assert_eq!(
            classify_terminal("/applications/xcode.app/contents/macos/xcode"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn plain_command_names_still_work() {
        // Linux 上 comm 就是个裸命令名，没有 .app 可看
        assert_eq!(classify_terminal("code"), Some("Code"));
        assert_eq!(classify_terminal("code-insiders"), Some("Code"));
        assert_eq!(classify_terminal("gnome-terminal-server"), Some("Terminal"));
        assert_eq!(classify_terminal("alacritty"), Some("Alacritty"));
        assert_eq!(classify_terminal("zsh"), None);
    }

    #[cfg(unix)]
    #[test]
    fn app_bundle_name_only_matches_real_bundles() {
        assert_eq!(app_bundle_name("/applications/warp.app"), Some("warp"));
        assert_eq!(
            app_bundle_name("/applications/warp.app/contents/macos/stable"),
            Some("warp")
        );
        // ".appimage" 不是 bundle，不能被 ".app" 前缀骗过去
        assert_eq!(app_bundle_name("/opt/cursor.appimage"), None);
    }

    // ── Linux 投递参数 ──
    //
    // Linux 的投递代码只有 ubuntu 那个 CI job 能编，所以把「参数长什么样」
    // 这部分挪成了平台无关的常量，让每个平台都跑一遍。

    #[test]
    fn ydotool_releases_every_modifier_it_presses() {
        // 漏一个松开事件 = 修饰键卡住，用户之后敲的每个字都带 Ctrl
        let (mut pressed, mut released) = (Vec::new(), Vec::new());
        for ev in YDOTOOL_PASTE_KEYS {
            let (code, state) = ev.split_once(':').expect("键码格式应为 code:state");
            match state {
                "1" => pressed.push(code),
                "0" => released.push(code),
                other => panic!("未知的按键状态 {other}"),
            }
        }
        released.reverse();
        assert_eq!(pressed, released, "按下的键必须原样反序松开");
        assert_eq!(pressed, vec!["29", "42", "47"], "应当是 Ctrl+Shift+V");
    }

    #[test]
    fn clipboard_tools_cover_wayland_and_x11() {
        let names: Vec<&str> = LINUX_CLIPBOARD_TOOLS.iter().map(|(n, _)| *n).collect();
        // wl-copy 必须排在最前：Wayland 上 xclip 会「成功」，
        // 但写进的是一个没人读的 X 剪贴板
        assert_eq!(names, vec!["wl-copy", "xclip", "xsel"]);
        for (tool, args) in LINUX_CLIPBOARD_TOOLS {
            // 选区必须写死：xclip/xsel 默认写 PRIMARY（鼠标选中区），
            // 而 Ctrl+Shift+V 读的是 CLIPBOARD
            if tool != "wl-copy" {
                assert!(
                    args.iter().any(|a| a.contains("clipboard")),
                    "{tool} 没有指定 clipboard 选区"
                );
            }
        }
    }
    // TESTS_PLACEHOLDER_RESUMER

    // ── 盲敲授权 ──

    /// 只有 macOS 用得上这个 helper：它存在的意义是构造一个带盲敲授权的
    /// `Resumer` 去调 `macos_script` / `title_matched_script`。Windows 的脚本
    /// 生成器是个自由函数（`win_script` 直接调它），Linux 那边则没有可单测的
    /// 纯字符串入口。cfg 必须跟调用点严格对齐——多挂一个平台，那个平台的
    /// `-D warnings` 就会因为 dead_code 而红。
    #[cfg(target_os = "macos")]
    fn resumer_with(auto_follow_latest: bool) -> Resumer {
        Resumer::new(AppConfig {
            auto_follow_latest,
            language: "zh".to_string(),
            ..Default::default()
        })
    }

    /// 盲敲默认必须是关的——三个平台都得验，跟 [`Resumer::allow_blind`] 同门
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    #[test]
    fn blind_typing_is_off_by_default() {
        assert!(!Resumer::new(AppConfig::default()).allow_blind());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vscode_refuses_when_there_is_no_window_to_match() {
        // 项目名为空 = 无从匹配窗口。默认必须整段放弃，
        // 脚本连 activate 都不该有——activate 之后的按键就已经无处追回了。
        let script = resumer_with(false).title_matched_script("Cursor", "", "继续");
        assert_eq!(script.trim(), r#"return "refused""#);

        // 用户显式打开「跟随最新会话」才允许赌一把
        let blind = resumer_with(true).title_matched_script("Cursor", "", "继续");
        assert!(blind.contains(r#"set the clipboard to "继续""#));
        assert!(blind.contains(r#"keystroke "v" using command down"#));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vscode_bails_out_before_typing_when_the_title_missed() {
        let script = resumer_with(false).title_matched_script("Cursor", "agent-pulse", "继续");
        // 拒绝的判断必须排在 keystroke 之前，否则「匹配失败」只是个返回值
        let refuse_at = script.find("if matched is \"unmatched\" and allowBlind is false");
        let type_at = script.find("keystroke");
        assert!(refuse_at.is_some() && type_at.is_some());
        assert!(refuse_at < type_at, "拒绝判断必须在键入之前");
        assert!(script.contains("set allowBlind to false"));
        // 剪贴板也得等拒绝判断之后再动：refused 的分支不该顺手清掉用户的剪贴板
        let stage_at = script.find("set the clipboard to");
        assert!(refuse_at < stage_at, "拒绝之前不要碰剪贴板");
    }

    // ── 输入法：合成按键会被拼音改写，剪贴板不会 ──

    #[cfg(target_os = "macos")]
    #[test]
    fn prompts_go_through_the_clipboard_not_the_keyboard() {
        // 用户实测：中文输入法开着时，`keystroke "你之前有一个活跃的 goal 目标…"`
        // 落进终端变成「啊啊啊啊啊啊啊啊啊goal啊啊啊啊啊啊，aaaaaaaaaa…」——
        // 每个汉字塌成一个「啊」或一个「a」，只有 ASCII 的 goal 活了下来。
        let prompt = "你之前有一个活跃的 goal 目标还未完成";
        for project in ["", "agent-pulse"] {
            let script = resumer_with(true).title_matched_script("Code", project, prompt);
            assert!(
                !script.contains(&format!(r#"keystroke "{prompt}""#)),
                "{project}：提示词不能走合成按键"
            );
            assert!(
                script.contains(&format!(r#"set the clipboard to "{prompt}""#)),
                "{project}：提示词要先进剪贴板"
            );
            assert!(script.contains(r#"keystroke "v" using command down"#));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_users_clipboard_is_given_back() {
        // 借剪贴板是手段，不是副作用：粘完要还
        let script = resumer_with(true).title_matched_script("Code", "agent-pulse", "继续");
        let paste_at = script.find(r#"keystroke "v""#).unwrap();
        let restore_at = script.find("set the clipboard to savedClipboard").unwrap();
        assert!(paste_at < restore_at, "还原必须在粘贴之后");
        assert!(script.contains("set savedClipboard to the clipboard as text"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn iterm_writes_straight_to_the_pty() {
        // iTerm2 的 `write text` 不过键盘，也就不过输入法，没必要动用户的剪贴板
        let script = resumer_with(false)
            .macos_script("iTerm2", Some("/dev/ttys003"), "agent-pulse", "继续")
            .unwrap();
        assert!(script.contains(r#"write text "继续""#));
        assert!(!script.contains("set the clipboard"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unknown_terminal_without_permission_generates_nothing() {
        // 连脚本都不该生成：脚本一旦跑起来就已经 activate 了别人的窗口
        assert!(resumer_with(false)
            .macos_script("", None, "agent-pulse", "继续")
            .is_none());
        assert!(resumer_with(true)
            .macos_script("", None, "agent-pulse", "继续")
            .is_some());
    }

    /// 生成的 AppleScript 必须真的能编译
    ///
    /// 这是唯一能提前发现语法错的办法：脚本跑起来才报的错，代价是把一句中文
    /// 敲进用户的终端。`osacompile` 只编译不执行，正好。
    ///
    /// 一处环境依赖躲不掉：`tell application "iTerm2"` 里的 `write text` 要靠
    /// iTerm2 自己的词典才能编译，机器上没装就没法验。所以先问一句
    /// `id of application`（只查 bundle，不会启动应用），装了的才编。
    #[cfg(target_os = "macos")]
    #[test]
    fn every_generated_script_compiles() {
        use std::io::Write;

        fn tool_exists(name: &str) -> bool {
            Command::new("which")
                .arg(name)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        fn app_installed(name: &str) -> bool {
            Command::new("osascript")
                .args(["-e", &format!(r#"id of application "{name}""#)])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        if !tool_exists("osacompile") {
            return; // 没有 osacompile 就跳过，别让环境差异变成测试失败
        }

        // 中文提示词 + 带引号和反斜杠的刁钻输入，一起过一遍
        let prompts = ["继续", r#"接着干 "goal" \ 别重来"#];
        // (进程名, TTY, 项目名, 编译前要确认已安装的应用)
        let cases: Vec<(&str, Option<&str>, &str, Option<&str>)> = vec![
            (
                "iTerm2",
                Some("/dev/ttys003"),
                "agent-pulse",
                Some("iTerm2"),
            ),
            (
                "Terminal",
                Some("/dev/ttys003"),
                "agent-pulse",
                Some("Terminal"),
            ),
            ("Code", None, "agent-pulse", None),
            ("Code", None, "", None),
            ("Cursor", Some("/dev/ttys001"), "agent-pulse", None),
            // IDE 内置终端也走窗口标题这条路，语法必须一起验
            ("IntelliJ IDEA", None, "agent-pulse", None),
            ("PyCharm", Some("/dev/ttys004"), "agent-pulse", None),
            ("Android Studio", None, "agent-pulse", None),
            ("Windsurf", None, "agent-pulse", None),
            ("Warp", Some("/dev/ttys002"), "agent-pulse", None),
            ("SomethingElse", None, "agent-pulse", None),
        ];

        let dir = std::env::temp_dir().join(format!("agent-pulse-osa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut compiled = 0usize;

        for prompt in prompts {
            for blind in [false, true] {
                for (app, tty, project, needs) in &cases {
                    if needs.map(|n| !app_installed(n)).unwrap_or(false) {
                        continue;
                    }
                    let Some(script) = resumer_with(blind).macos_script(app, *tty, project, prompt)
                    else {
                        continue;
                    };
                    let src = dir.join("candidate.applescript");
                    let mut f = std::fs::File::create(&src).unwrap();
                    f.write_all(script.as_bytes()).unwrap();
                    drop(f);

                    let out = Command::new("osacompile")
                        .arg("-o")
                        .arg(dir.join("candidate.scpt"))
                        .arg(&src)
                        .output()
                        .unwrap();
                    assert!(
                        out.status.success(),
                        "{app}/{project}/blind={blind} 编译失败：{}\n--- 脚本 ---\n{script}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                    compiled += 1;
                }
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
        // Terminal.app 和「通用回退」两条路在任何 macOS 上都该被验到，
        // 否则这个测试就是在空转
        assert!(compiled >= 8, "只编译了 {compiled} 个脚本，覆盖太少");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vscode_script_never_sends_ctrl_c() {
        // 焦点若在编辑器里，Ctrl-C 是复制，紧接着的提示词会被写进源文件
        for blind in [false, true] {
            for project in ["", "agent-pulse"] {
                let script = resumer_with(blind).title_matched_script("Code", project, "继续");
                assert!(!script.contains("using control down"), "{project}/{blind}");
            }
        }
    }

    // ── 定位演练（dry-run）──
    //
    // 演练的核心承诺是「只看不动」：走和真续跑同一条定位链路，
    // 但不抢焦点、不动剪贴板、不敲一个字。下面每条都在钉这个承诺。

    fn win_locate(project: &str, blind: bool) -> String {
        Resumer::windows_locate_script(4242, project, blind)
    }

    #[test]
    fn windows_locate_walks_up_to_the_host_window() {
        // 与真续跑脚本同一条链路：agent 是控制台程序，窗口属于宿主，要往上找
        let script = win_locate("agent-pulse", false);
        assert!(
            script.contains("for ($i = 0; $i -lt 8; $i++)"),
            "要沿父进程链往上找"
        );
        assert!(
            script.contains("Write-Output \"HOST="),
            "要报宿主进程名供诊断"
        );
    }

    #[test]
    fn windows_locate_never_touches_the_window_or_keyboard() {
        // 演练绝不能有副作用：不前台化、不碰剪贴板、不发按键
        for blind in [false, true] {
            let script = win_locate("agent-pulse", blind);
            assert!(
                !script.contains("SetForegroundWindow"),
                "不许前台化 {blind}"
            );
            assert!(!script.contains("ShowWindow"), "不许还原窗口 {blind}");
            assert!(!script.contains("SendKeys"), "不许发按键 {blind}");
            assert!(!script.contains("Set-Clipboard"), "不许写剪贴板 {blind}");
            assert!(!script.contains("Get-Clipboard"), "不许读剪贴板 {blind}");
        }
    }

    #[test]
    fn windows_locate_distinguishes_single_and_multi_tab_hosts() {
        let script = win_locate("agent-pulse", false);
        // 单窗口宿主（cmd / conhost）一个窗口就是一个会话 → EXACT；
        // 多标签宿主认到窗口不等于认到标签，要核标题 → WINDOW / REFUSED
        assert!(script.contains("Write-Output \"EXACT\""));
        assert!(script.contains("Write-Output \"WINDOW\""));
        assert!(script.contains("Write-Output \"REFUSED\""));
        // 多标签名单与真续跑脚本共用同一张表
        assert!(script.contains("'windowsterminal'"));
        assert!(script.contains("'code'"));
    }

    #[test]
    fn windows_locate_blind_branch_needs_permission() {
        // 「标题没对上但仍会敲」的 BLIND 分支只在开了盲敲时才该出现
        let on = win_locate("agent-pulse", true);
        assert!(on.contains("Write-Output \"BLIND\""));
        assert!(on.contains("$allowBlind = $true"));
        let off = win_locate("agent-pulse", false);
        assert!(off.contains("$allowBlind = $false"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_locate_scripts_never_type() {
        // 演练的核心承诺：只看不动。所有定位脚本都不能含任何投递动作。
        let r = resumer_with(false);
        let scripts = [
            r.macos_locate_script("iTerm2", Some("/dev/ttys003"), "agent-pulse"),
            r.macos_locate_script("Terminal", Some("/dev/ttys003"), "agent-pulse"),
            Some(r.title_locate_script("Code", "agent-pulse")),
            Some(r.title_locate_script("Code", "")),
        ];
        for script in scripts.iter().flatten() {
            assert!(!script.contains("write text"), "iTerm2 不许 write text");
            assert!(!script.contains("keystroke"), "不许合成按键");
            assert!(!script.contains("the clipboard"), "不许动剪贴板");
            assert!(!script.contains("AXRaise"), "不许移动窗口");
            assert!(!script.contains("activate"), "不许抢焦点");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_locate_matches_iterm_by_tty() {
        let script = resumer_with(false)
            .macos_locate_script("iTerm2", Some("/dev/ttys003"), "agent-pulse")
            .expect("iTerm2 + TTY 该有定位脚本");
        assert!(script.contains(r#"tty of aSession contains "/dev/ttys003""#));
        assert!(script.contains(r#"return "matched""#));
        assert!(script.contains(r#"return "refused""#));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_locate_unknown_terminal_has_no_script() {
        // 连是哪个终端都认不出时，连 osascript 都不启动
        assert!(resumer_with(false)
            .macos_locate_script("", None, "agent-pulse")
            .is_none());
        assert!(resumer_with(true)
            .macos_locate_script("SomethingElse", None, "agent-pulse")
            .is_none());
    }

    /// 演练脚本也是生成的 AppleScript，语法同样要真编译一遍才放心
    #[cfg(target_os = "macos")]
    #[test]
    fn every_locate_script_compiles() {
        use std::io::Write;

        fn tool_exists(name: &str) -> bool {
            Command::new("which")
                .arg(name)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        fn app_installed(name: &str) -> bool {
            Command::new("osascript")
                .args(["-e", &format!(r#"id of application "{name}""#)])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
        if !tool_exists("osacompile") {
            return;
        }

        let cases: Vec<(&str, Option<&str>, &str, Option<&str>)> = vec![
            (
                "iTerm2",
                Some("/dev/ttys003"),
                "agent-pulse",
                Some("iTerm2"),
            ),
            (
                "Terminal",
                Some("/dev/ttys003"),
                "agent-pulse",
                Some("Terminal"),
            ),
            ("Code", None, "agent-pulse", None),
            ("Code", None, "", None),
            ("Cursor", None, "agent-pulse", None),
            ("PyCharm", None, "agent-pulse", None),
        ];

        let dir = std::env::temp_dir().join(format!("agent-pulse-locate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut compiled = 0usize;

        for (app, tty, project, needs) in &cases {
            if needs.map(|n| !app_installed(n)).unwrap_or(false) {
                continue;
            }
            let Some(script) = resumer_with(false).macos_locate_script(app, *tty, project) else {
                continue;
            };
            let src = dir.join("locate.applescript");
            let mut f = std::fs::File::create(&src).unwrap();
            f.write_all(script.as_bytes()).unwrap();
            drop(f);
            let out = Command::new("osacompile")
                .arg("-o")
                .arg(dir.join("locate.scpt"))
                .arg(&src)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{app}/{project} 定位脚本编译失败：{}\n--- 脚本 ---\n{script}",
                String::from_utf8_lossy(&out.stderr)
            );
            compiled += 1;
        }

        let _ = std::fs::remove_dir_all(&dir);
        // Code / Cursor / PyCharm 走 System Events 不依赖安装，至少这几个该被验到
        assert!(compiled >= 3, "只编译了 {compiled} 个定位脚本，覆盖太少");
    }

    // ── 投递核验的四种结论 ──
    //
    // 这四种结论把「脚本没报错」和「字真的进去了」分开了。分类一旦搞错，
    // 后果是整条自动续跑链自己把自己关掉，所以这里逐条钉住。

    #[test]
    fn only_real_deliveries_consume_the_budget() {
        assert!(ResumeOutcome::Landed.counts_as_nudge());
        assert!(
            ResumeOutcome::Unverifiable.counts_as_nudge(),
            "核验不了不等于失败：不落记录文件的 agent 不该被判死刑"
        );
        assert!(
            !ResumeOutcome::Failed.counts_as_nudge(),
            "没送达就不算催过——否则「敲不进去」会被算成「已经敲够了」"
        );
        assert!(
            !ResumeOutcome::Silent.counts_as_nudge(),
            "脚本成功但会话没动，等于没催"
        );
    }

    #[test]
    fn silent_is_a_failure_too() {
        assert!(ResumeOutcome::Failed.is_failure());
        assert!(
            ResumeOutcome::Silent.is_failure(),
            "从用户角度看，「按键进了别的窗口」和「脚本报错」是同一件事"
        );
        assert!(!ResumeOutcome::Landed.is_failure());
        assert!(!ResumeOutcome::Unverifiable.is_failure());
    }

    #[test]
    fn every_outcome_has_localized_text() {
        for outcome in [
            ResumeOutcome::Failed,
            ResumeOutcome::Landed,
            ResumeOutcome::Silent,
            ResumeOutcome::Unverifiable,
        ] {
            let key = outcome.i18n_key();
            let zh = I18n::from_code("zh").t(key);
            let en = I18n::from_code("en").t(key);
            assert_ne!(zh, key, "{key} 没有中文词条，会把裸键名显示给用户");
            assert_ne!(en, key, "{key} 没有英文词条");
            assert_ne!(zh, en, "{key} 两种语言的文案一模一样，八成漏翻了");
        }
    }

    #[test]
    fn fingerprint_is_none_without_a_transcript() {
        // 没有记录文件 → 核验不可用，而不是核验失败
        let s = AgentSession::default();
        assert!(activity_fingerprint(&s).is_none());
        let missing = AgentSession {
            session_file: Some("/definitely/not/a/real/path.jsonl".to_string()),
            ..Default::default()
        };
        assert!(activity_fingerprint(&missing).is_none());
    }

    #[test]
    fn fingerprint_changes_when_the_transcript_grows() {
        let dir = std::env::temp_dir().join(format!("agentpulse-fp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("session.jsonl");
        std::fs::write(&path, b"{\"type\":\"user\"}\n").unwrap();

        let session = AgentSession {
            session_file: Some(path.to_string_lossy().to_string()),
            ..Default::default()
        };
        let before = activity_fingerprint(&session).expect("刚写完的文件该有指纹");

        // 追加一行就是「agent 动了」的信号；只比字节数就够，不必读内容
        std::fs::write(&path, b"{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n").unwrap();
        let after = activity_fingerprint(&session).expect("文件还在");
        assert_ne!(before, after, "记录长了指纹就得变，否则核验永远判「没动」");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
