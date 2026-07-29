use super::{AgentAdapter, AgentSession, SessionStatus};
use chrono::Local;
use std::path::PathBuf;
use sysinfo::System;

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

    fn discover_sessions(&self) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let system = System::new_all();
        for (pid, process) in system.processes() {
            let proc_name = process.name().to_string_lossy().to_lowercase();
            let cmd = process
                .cmd()
                .iter()
                .map(|c| c.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" ");

            let is_codex = proc_name == "codex"
                || (proc_name.contains("node") && cmd.contains("codex"));

            if !is_codex || cmd.contains("agent-pulse") {
                continue;
            }

            let cwd = process
                .cwd()
                .map(|c| c.to_string_lossy().to_string())
                .unwrap_or_default();

            sessions.push(AgentSession {
                id: format!("cx-{}", pid.as_u32()),
                adapter_id: self.id().to_string(),
                agent_name: self.name().to_string(),
                pid: pid.as_u32(),
                command: cmd,
                working_dir: cwd,
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
