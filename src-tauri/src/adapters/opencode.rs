use super::{AgentAdapter, AgentSession, ProcessSnapshot, SessionStatus};
use chrono::Local;
use std::path::PathBuf;

/// OpenCode 适配器
///
/// OpenCode 是一个开源的终端 AI 编程助手（类似 Claude Code）
/// 检测策略：
/// 1. 进程扫描：查找 `opencode` 进程
/// 2. 会话目录：~/.opencode/sessions/
pub struct OpenCodeAdapter;

impl AgentAdapter for OpenCodeAdapter {
    fn id(&self) -> &str {
        "opencode"
    }

    fn name(&self) -> &str {
        "OpenCode"
    }

    fn discover_sessions(&self, processes: &[ProcessSnapshot]) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        for proc in processes {
            // 匹配 opencode 进程（Go 编译的二进制，Windows 下为 opencode.exe）
            let is_opencode = proc.name == "opencode"
                || proc.name == "opencode.exe"
                || proc.name.contains("opencode");

            if !is_opencode || proc.cmd.contains("agent-pulse") || proc.cmd.contains("grep") {
                continue;
            }

            sessions.push(AgentSession {
                id: format!("oc-{}", proc.pid),
                adapter_id: self.id().to_string(),
                agent_name: self.name().to_string(),
                pid: proc.pid,
                command: proc.cmd.clone(),
                working_dir: proc.cwd.clone(),
                session_file: None,
                discovered_at: now.clone(),
                last_activity: now.clone(),
                status: SessionStatus::Active,
                resume_count: 0,
                last_resume_at: None,
            });
        }

        sessions
    }

    fn session_files(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        // OpenCode 可能使用 .opencode/ 或项目内 .opencode/ 目录
        let global_dir = home.join(".opencode").join("sessions");
        if global_dir.exists() {
            std::fs::read_dir(global_dir)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        }
    }

    fn recent_output(&self, _session: &AgentSession) -> Option<String> {
        // OpenCode 暂无稳定的会话文件格式，依赖进程状态 + 心跳检测
        None
    }
}
