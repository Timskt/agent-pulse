use crate::adapters::AgentSession;
use crate::config::AppConfig;
use std::process::Command;

/// 续跑执行器 — 向中断的 Agent 发送续跑指令
///
/// 多窗口精确定位策略：
/// 1. 通过 PID 获取进程所在 TTY（如 /dev/ttys003）
/// 2. 通过 PID 父子关系识别终端应用（iTerm2/Terminal/VS Code/Cursor）
/// 3. 使用 AppleScript 遍历所有窗口/标签，精确匹配 TTY 后发送
///
/// 平台支持：
/// - macOS: AppleScript + TTY 匹配（v0.1.0）
/// - Windows: SendInput API / PowerShell SendKeys (v0.2.0)
/// - Linux: xdotool / ydotool (v0.2.0)
pub struct Resumer {
    config: AppConfig,
}

impl Resumer {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
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
            Err("不支持的平台".to_string())
        }
    }

    /// macOS: 通过 TTY 精确定位终端窗口并发送续跑指令
    #[cfg(target_os = "macos")]
    async fn resume_macos(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        let escaped_prompt = prompt
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");

        // 1. 获取目标进程的 TTY
        let tty = self.get_tty_for_pid(session.pid);
        // 2. 识别终端应用
        let terminal_app = self.find_terminal_for_pid(session.pid);
        // 3. 获取工作目录名（用于 VS Code 窗口标题匹配）
        let project_name = session
            .working_dir
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();

        tracing::info!(
            "[Resumer] 会话 {} → 终端: {}, TTY: {:?}, 项目: {}",
            session.id,
            terminal_app,
            tty,
            project_name
        );

        // 4. 根据终端类型生成精确的 AppleScript
        let script = match (terminal_app.as_str(), &tty) {
            // iTerm2: 遍历所有 session，按 TTY 精确匹配
            ("iTerm2", Some(tty_path)) => format!(
                r#"tell application "iTerm2"
    repeat with aWindow in windows
        repeat with aTab in tabs of aWindow
            repeat with aSession in sessions of aTab
                if tty of aSession contains "{}" then
                    select aSession
                    tell aSession
                        write text "{}"
                    end tell
                    return "matched"
                end if
            end repeat
        end repeat
    end repeat
    -- 未匹配到 TTY，回退到当前 session
    tell current session of current window
        write text "{}"
    end tell
    return "fallback"
end tell"#,
                tty_path, escaped_prompt, escaped_prompt
            ),

            // Terminal.app: 遍历所有 tab，按 TTY 精确匹配
            ("Terminal", Some(tty_path)) => format!(
                r#"tell application "Terminal"
    repeat with aWindow in windows
        repeat with aTab in tabs of aWindow
            if tty of aTab contains "{}" then
                set selected tab of aWindow to aTab
                set index of aWindow to 1
                activate
                delay 0.3
                do script "{}" in aTab
                return "matched"
            end if
        end repeat
    end repeat
    -- 回退到前台窗口
    activate
    delay 0.3
    do script "{}" in front window
    return "fallback"
end tell"#,
                tty_path, escaped_prompt, escaped_prompt
            ),

            // VS Code / Cursor: 通过窗口标题匹配项目名
            ("Code", _) => self.vscode_window_script("Visual Studio Code", &project_name, &escaped_prompt),
            ("Cursor", _) => self.vscode_window_script("Cursor", &project_name, &escaped_prompt),

            // Warp 终端: 类似 iTerm2 的方式
            ("Warp", Some(_)) => format!(
                r#"tell application "Warp"
    activate
end tell
delay 0.5
tell application "System Events"
    tell process "Warp"
        set frontmost to true
        delay 0.3
        keystroke "{}"
        delay 0.3
        key code 36
    end tell
end tell
return "warp""#,
                escaped_prompt
            ),

            // 通用回退：激活前台应用发送
            _ => format!(
                r#"tell application "System Events"
    set frontApp to name of first application process whose frontmost is true
end tell
tell application frontApp
    activate
end tell
delay 0.5
tell application "System Events"
    keystroke "{}"
    delay 0.3
    key code 36
end tell
return "generic""#,
                escaped_prompt
            ),
        };

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("执行 AppleScript 失败: {e}"))?;

        if output.status.success() {
            let result_msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(format!(
                "已通过 {} 发送续跑指令 (匹配: {}, TTY: {})",
                if terminal_app.is_empty() { "前台终端" } else { &terminal_app },
                result_msg,
                tty.as_deref().unwrap_or("N/A")
            ))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("AppleScript 执行出错: {stderr}"))
        }
    }

    /// VS Code / Cursor 多窗口定位脚本
    /// 通过窗口标题包含项目名来定位正确的窗口
    #[cfg(target_os = "macos")]
    fn vscode_window_script(&self, app_name: &str, project_name: &str, prompt: &str) -> String {
        if project_name.is_empty() {
            // 无法确定项目名，回退到激活应用
            return format!(
                r#"tell application "{}"
    activate
end tell
delay 1
tell application "System Events"
    tell process "{}"
        set frontmost to true
        delay 0.3
        keystroke "c" using control down
        delay 0.5
        keystroke "{}"
        delay 0.3
        key code 36
    end tell
end tell
return "no-project-match""#,
                app_name, app_name, prompt
            );
        }

        // 遍历窗口，找到标题包含项目名的窗口并置顶
        format!(
            r#"tell application "{}"
    activate
end tell
delay 0.5
tell application "System Events"
    tell process "{}"
        set frontmost to true
        set windowList to every window
        repeat with w in windowList
            if name of w contains "{}" then
                perform action "AXRaise" of w
                delay 0.3
                exit repeat
            end if
        end repeat
        delay 0.3
        keystroke "c" using control down
        delay 0.5
        keystroke "{}"
        delay 0.3
        key code 36
    end tell
end tell
return "vscode-window""#,
            app_name, app_name, project_name, prompt
        )
    }

    /// 获取进程所在的 TTY 设备路径
    #[cfg(target_os = "macos")]
    fn get_tty_for_pid(&self, pid: u32) -> Option<String> {
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
            return self.get_parent_tty(pid);
        }
        Some(format!("/dev/{}", tty))
    }

    /// 向上查找父进程链的 TTY（claude 可能作为子进程没有自己的 TTY）
    #[cfg(target_os = "macos")]
    fn get_parent_tty(&self, pid: u32) -> Option<String> {
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
    #[cfg(target_os = "macos")]
    fn find_terminal_for_pid(&self, pid: u32) -> String {
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

            if comm.contains("iterm") {
                return "iTerm2".to_string();
            } else if comm.contains("terminal") && !comm.contains("helper") {
                return "Terminal".to_string();
            } else if comm.contains("code") && !comm.contains("helper") {
                return "Code".to_string();
            } else if comm.contains("cursor") && !comm.contains("helper") {
                return "Cursor".to_string();
            } else if comm.contains("warp") {
                return "Warp".to_string();
            } else if comm.contains("kitty") {
                return "Kitty".to_string();
            } else if comm.contains("alacritty") {
                return "Alacritty".to_string();
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
    /// 3. 通过 SendKeys 发送续跑提示词 + 回车
    #[cfg(target_os = "windows")]
    async fn resume_windows(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        // 转义 PowerShell 特殊字符
        let escaped_prompt = prompt
            .replace('`', "``")
            .replace('"', "`\"")
            .replace('{', "`{")
            .replace('}', "`}")
            .replace('+', "`+")
            .replace('^', "`^")
            .replace('%', "`%")
            .replace('~', "`~")
            .replace('(', "`(")
            .replace(')', "`)");

        let pid = session.pid;

        // PowerShell 脚本：通过 PID 定位窗口并发送按键
        let ps_script = format!(
            r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinAPI {{
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
}}
"@

$pid_target = {pid}
$proc = Get-Process -Id $pid_target -ErrorAction SilentlyContinue
if (-not $proc) {{
    # 尝试父进程
    $procs = Get-CimInstance Win32_Process | Where-Object {{ $_.ProcessId -eq $pid_target }}
    if ($procs) {{
        $parent = Get-Process -Id $procs.ParentProcessId -ErrorAction SilentlyContinue
        if ($parent -and $parent.MainWindowHandle -ne [IntPtr]::Zero) {{
            $proc = $parent
        }}
    }}
}}

if (-not $proc -or $proc.MainWindowHandle -eq [IntPtr]::Zero) {{
    Write-Output "NO_WINDOW"
    exit 1
}}

$hwnd = $proc.MainWindowHandle
[WinAPI]::ShowWindow($hwnd, 9)  # SW_RESTORE
Start-Sleep -Milliseconds 300
[WinAPI]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 500

Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait("{escaped_prompt}")
Start-Sleep -Milliseconds 200
[System.Windows.Forms.SendKeys]::SendWait("{{ENTER}}")

Write-Output "SENT_TO_$($proc.ProcessName)"
"#
        );

        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .output()
            .map_err(|e| format!("执行 PowerShell 失败: {e}"))?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(format!("已通过 Windows 发送续跑指令 ({result})"))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.contains("NO_WINDOW") {
                Err(format!("未找到 PID {} 对应的终端窗口", pid))
            } else {
                Err(format!("Windows 续跑失败: {stderr}"))
            }
        }
    }

    /// Linux: 通过 xdotool 定位终端窗口并发送续跑指令
    ///
    /// 策略：
    /// 1. 使用 xdotool search --pid 查找目标进程窗口
    /// 2. 如果找不到，向上遍历父进程
    /// 3. windowactivate + type + Return
    /// 4. Wayland 回退到 ydotool
    #[cfg(target_os = "linux")]
    async fn resume_linux(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        let pid = session.pid;

        // 尝试通过 xdotool 查找窗口
        let window_id = self.find_x11_window_for_pid(pid);

        match window_id {
            Some(wid) => {
                // 激活窗口
                let _ = Command::new("xdotool")
                    .args(["windowactivate", "--sync", &wid])
                    .output();

                tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;

                // 输入提示词
                let type_result = Command::new("xdotool")
                    .args(["type", "--clearmodifiers", "--delay", "20", prompt])
                    .output()
                    .map_err(|e| format!("xdotool type 失败: {e}"))?;

                if !type_result.status.success() {
                    return Err("xdotool 输入失败".to_string());
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                // 发送回车
                let _ = Command::new("xdotool")
                    .args(["key", "Return"])
                    .output();

                Ok(format!("已通过 xdotool 发送续跑指令 (window: {wid})"))
            }
            None => {
                // 回退到 ydotool（Wayland 环境）
                self.resume_linux_ydotool(prompt).await
            }
        }
    }

    /// Linux X11: 通过 PID 查找窗口 ID（向上遍历父进程）
    #[cfg(target_os = "linux")]
    fn find_x11_window_for_pid(&self, pid: u32) -> Option<String> {
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
    #[cfg(target_os = "linux")]
    async fn resume_linux_ydotool(&self, prompt: &str) -> Result<String, String> {
        // 检查 ydotool 是否可用
        let check = Command::new("which")
            .arg("ydotool")
            .output();

        if check.is_err() || !check.unwrap().status.success() {
            return Err(
                "未找到 xdotool 或 ydotool。请安装: sudo apt install xdotool 或 sudo apt install ydotool".to_string()
            );
        }

        // ydotool type 输入文本
        let type_result = Command::new("ydotool")
            .args(["type", "--", prompt])
            .output()
            .map_err(|e| format!("ydotool type 失败: {e}"))?;

        if !type_result.status.success() {
            return Err("ydotool 输入失败".to_string());
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // ydotool key Enter (keycode 28)
        let _ = Command::new("ydotool")
            .args(["key", "28:1", "28:0"])
            .output();

        Ok("已通过 ydotool (Wayland) 发送续跑指令".to_string())
    }
}
