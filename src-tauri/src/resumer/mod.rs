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
    pub async fn resume(&self, session: &AgentSession) -> Result<String, String> {
        let prompt = &self.config.resume_prompt;

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

    /// Windows: 通过 PowerShell 发送按键 (v0.2.0)
    #[cfg(target_os = "windows")]
    async fn resume_windows(&self, _session: &AgentSession, _prompt: &str) -> Result<String, String> {
        Err("Windows 平台支持将在 v0.2.0 中实现".to_string())
    }

    /// Linux: 通过 xdotool 发送按键 (v0.2.0)
    #[cfg(target_os = "linux")]
    async fn resume_linux(&self, _session: &AgentSession, _prompt: &str) -> Result<String, String> {
        Err("Linux 平台支持将在 v0.2.0 中实现".to_string())
    }
}
