use super::{AgentAdapter, AgentSession, ProcessSnapshot, SessionStatus};
use chrono::Local;
use std::path::PathBuf;

/// OpenAI Codex CLI 适配器
///
/// 检测策略：
/// 1. 进程扫描：查找 `codex` 进程
/// 2. 会话目录：~/.codex/sessions/
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn id(&self) -> &str {
        "codex"
    }

    fn name(&self) -> &str {
        "Codex CLI"
    }

    fn discover_sessions(&self, processes: &[ProcessSnapshot]) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        for proc in processes {
            let is_codex = proc.name == "codex"
                || proc.name == "codex.exe"
                || (proc.name.contains("node") && proc.cmd.contains("codex"));

            if !is_codex || proc.cmd.contains("agent-pulse") {
                continue;
            }

            sessions.push(AgentSession {
                id: format!("cx-{}", proc.pid),
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
                ..Default::default()
            });
        }

        sessions
    }

    fn session_files(&self) -> Vec<PathBuf> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let sessions_dir = home.join(".codex").join("sessions");
        if sessions_dir.exists() {
            std::fs::read_dir(sessions_dir)
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
        // Codex CLI 暂无稳定的会话文件格式，依赖进程状态检测
        None
    }
}
