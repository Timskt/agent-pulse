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
    pub async fn resume(&self, session: &AgentSession, use_goal_prompt: bool) -> Result<String, String> {
        let prompt = if use_goal_prompt {
            &self.config.goal_resume_prompt
        } else {
            &self.config.resume_prompt
        };

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
        let Some(script) =
            self.macos_script(&terminal_app, tty.as_deref(), &project_name, prompt)
        else {
            // 定位不到又没开盲敲：到此为止，连 osascript 都不启动
            return Err(self.i18n.t("resume.blind_refused").to_string());
        };

        match run_osascript(&script, &self.i18n).await {
            // 脚本自己判断出「不知道该敲哪儿」，回传 refused
            Ok(raw) if raw == "refused" => {
                Err(self.i18n.t("resume.blind_refused").to_string())
            }
            // 认出来是哪个应用，但那个应用已经退了（窗口标题匹配这条路才可能出现）
            Ok(raw) if raw == "no-app" => {
                Err(self.i18n.t("resume.app_not_running").to_string())
            }
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
            Err(stderr) => Err(self
                .i18n
                .tf("resume.script_failed", &[("detail", &stderr)])),
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
        let ps_script = Self::windows_resume_script(
            session.pid,
            prompt,
            project_name,
            self.allow_blind(),
        );

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
            Ok(self
                .i18n
                .tf("resume.sent_simple", &[("outcome", &self.outcome_text(raw))]))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(self
                .i18n
                .tf("resume.script_failed", &[("detail", &stderr)]))
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
                let _ =
                    run_with_timeout("xdotool", &["windowactivate", "--sync", &wid], 10, &self.i18n)
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
            let ppid_output = std::fs::read_to_string(format!("/proc/{}/stat", current_pid)).ok()?;
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
            let pasted =
                run_with_timeout("ydotool", &paste_args, 10, &self.i18n).await?;
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
        assert!(script.contains("for ($i = 0; $i -lt 8; $i++)"), "要沿父进程链往上找");
        assert!(script.contains("Win32_Process -Filter \"ProcessId=$cur\""));
        assert!(script.contains("if ($cur -le 4) { break }"), "走到 System/Idle 就该停");
    }

    #[test]
    fn windows_confirms_the_window_actually_came_forward() {
        // SendKeys 打的是「当时的前台窗口」，SetForegroundWindow 在后台进程里经常被拒。
        // 不核一下，这段提示词就会落进用户正在看的窗口
        let script = win_script("agent-pulse", true);
        let focus_at = script.find("GetForegroundWindow() -ne $hwnd").expect("要核前台窗口");
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
            assert!(script.contains(&format!("'{host}'")), "{host} 应当算多标签宿主");
        }
        assert!(
            !WINDOWS_MULTI_TAB_HOSTS.contains(&"conhost") && !WINDOWS_MULTI_TAB_HOSTS.contains(&"cmd"),
            "cmd / conhost 一个窗口就是一个会话，不该被要求核标题"
        );
        let refuse_at = script.find(r#"Write-Output "REFUSED""#).expect("默认要能拒绝");
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
        assert_eq!(project_name_of("/Users/sky/code/agent-pulse"), "agent-pulse");
        assert_eq!(project_name_of("/Users/sky/code/agent-pulse/"), "agent-pulse");
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

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn resumer_with(auto_follow_latest: bool) -> Resumer {
        Resumer::new(AppConfig {
            auto_follow_latest,
            language: "zh".to_string(),
            ..Default::default()
        })
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
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
            ("iTerm2", Some("/dev/ttys003"), "agent-pulse", Some("iTerm2")),
            ("Terminal", Some("/dev/ttys003"), "agent-pulse", Some("Terminal")),
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
                    let Some(script) =
                        resumer_with(blind).macos_script(app, *tty, project, prompt)
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
}
