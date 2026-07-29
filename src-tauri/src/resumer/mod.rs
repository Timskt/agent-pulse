use crate::adapters::AgentSession;
use crate::config::AppConfig;
use std::process::Command;

/// 续跑执行器 — 向中断的 Agent 发送续跑指令
///
/// 平台策略：
/// - macOS: AppleScript 发送按键到终端应用
/// - Windows: PowerShell SendKeys (TODO v0.2.0)
/// - Linux: xdotool (TODO v0.2.0)
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

    /// macOS: 通过 AppleScript 向终端发送续跑指令
    #[cfg(target_os = "macos")]
    async fn resume_macos(&self, session: &AgentSession, prompt: &str) -> Result<String, String> {
        // 转义 AppleScript 中的特殊字符
        let escaped_prompt = prompt
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");

        // 策略: 通过 PID 找到对应的终端窗口并发送文本
        // 先尝试找到拥有该进程的终端应用
        let terminal_app = self.find_terminal_for_pid(session.pid);

        let script = match terminal_app.as_str() {
            "iTerm2" => format!(
                r#"tell application "iTerm2"
    activate
    delay 0.5
    tell current session of current window
        write text "{}"
    end tell
end tell"#,
                escaped_prompt
            ),
            "Terminal" => format!(
                r#"tell application "Terminal"
    activate
    delay 0.5
    do script "{}" in front window
end tell"#,
                escaped_prompt
            ),
            "Code" | "Cursor" => {
                // VS Code / Cursor 内置终端：通过激活窗口 + 模拟输入
                let app_name = if terminal_app == "Cursor" { "Cursor" } else { "Visual Studio Code" };
                format!(
                    r#"tell application "{}"
    activate
end tell
delay 1
tell application "System Events"
    tell process "{}"
        set frontmost to true
        delay 0.3
        -- 先按 Ctrl+C 确保回到输入状态
        keystroke "c" using control down
        delay 0.5
        keystroke "{}"
        delay 0.3
        key code 36
    end tell
end tell"#,
                    app_name, app_name, escaped_prompt
                )
            }
            _ => {
                // 通用方案：激活最前面的终端类应用
                format!(
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
end tell"#,
                    escaped_prompt
                )
            }
        };

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("执行 AppleScript 失败: {e}"))?;

        if output.status.success() {
            Ok(format!(
                "已通过 {} 发送续跑指令到会话 {}",
                if terminal_app.is_empty() { "前台终端" } else { &terminal_app },
                session.id
            ))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("AppleScript 执行出错: {stderr}"))
        }
    }

    /// 通过 PID 查找所属的终端应用
    #[cfg(target_os = "macos")]
    fn find_terminal_for_pid(&self, pid: u32) -> String {
        // 使用 ps 查找进程的父进程链，确定终端应用
        let output = Command::new("ps")
            .arg("-o")
            .arg("ppid=")
            .arg("-p")
            .arg(pid.to_string())
            .output();

        if let Ok(out) = output {
            if let Ok(ppid_str) = String::from_utf8(out.stdout) {
                if let Ok(ppid) = ppid_str.trim().parse::<u32>() {
                    // 查找父进程名称
                    if let Ok(name_out) = Command::new("ps")
                        .arg("-o")
                        .arg("comm=")
                        .arg("-p")
                        .arg(ppid.to_string())
                        .output()
                    {
                        let name = String::from_utf8_lossy(&name_out.stdout)
                            .trim()
                            .to_string();
                        let lower = name.to_lowercase();
                        if lower.contains("iterm") {
                            return "iTerm2".to_string();
                        } else if lower.contains("terminal") {
                            return "Terminal".to_string();
                        } else if lower.contains("code") {
                            return "Code".to_string();
                        } else if lower.contains("cursor") {
                            return "Cursor".to_string();
                        } else if lower.contains("warp") {
                            return "Warp".to_string();
                        }
                    }
                }
            }
        }

        String::new()
    }

    /// Windows: 通过 PowerShell 发送按键 (v0.2.0)
    #[cfg(target_os = "windows")]
    async fn resume_windows(&self, _session: &AgentSession, _prompt: &str) -> Result<String, String> {
        // TODO: 使用 SendInput API 或 PowerShell SendKeys
        Err("Windows 平台支持将在 v0.2.0 中实现".to_string())
    }

    /// Linux: 通过 xdotool 发送按键 (v0.2.0)
    #[cfg(target_os = "linux")]
    async fn resume_linux(&self, _session: &AgentSession, _prompt: &str) -> Result<String, String> {
        // TODO: 使用 xdotool type + key Return
        Err("Linux 平台支持将在 v0.2.0 中实现".to_string())
    }
}
