use super::{AgentAdapter, AgentSession, SessionStatus};
use chrono::Local;
use std::fs;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use sysinfo::System;

/// Claude Code (claude CLI) 适配器
///
/// 检测策略：
/// 1. 进程扫描：查找 `claude` 进程
/// 2. 会话文件：~/.claude/projects/*/sessions/*.jsonl
/// 3. 输出分析：读取会话 JSONL 最后几行判断状态
pub struct ClaudeCodeAdapter {
    claude_dir: PathBuf,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            claude_dir: home.join(".claude"),
        }
    }

    /// 查找最新的会话 JSONL 文件
    fn find_latest_session_file(&self) -> Vec<PathBuf> {
        let pattern = self
            .claude_dir
            .join("projects")
            .join("**")
            .join("*.jsonl")
            .to_string_lossy()
            .to_string();

        let mut files: Vec<PathBuf> = glob::glob(&pattern)
            .map(|paths| paths.filter_map(|p| p.ok()).collect())
            .unwrap_or_default();

        // 按修改时间降序排列
        files.sort_by(|a, b| {
            let ta = fs::metadata(a).and_then(|m| m.modified()).ok();
            let tb = fs::metadata(b).and_then(|m| m.modified()).ok();
            tb.cmp(&ta)
        });

        files.into_iter().take(10).collect()
    }

    /// 读取 JSONL 文件最后 N 行
    fn read_tail_lines(path: &PathBuf, n: usize) -> Vec<String> {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return vec![],
        };

        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return vec![],
        };

        let file_size = metadata.len();
        // 从文件末尾回退读取（最多读 64KB）
        let seek_pos = file_size.saturating_sub(64 * 1024);

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(seek_pos)).is_err() {
            return vec![];
        }

        let mut lines = Vec::new();
        let mut content = String::new();
        use std::io::Read;
        if reader.read_to_string(&mut content).is_err() {
            return vec![];
        }

        for line in content.lines() {
            lines.push(line.to_string());
        }

        // 只保留最后 n 行
        let len = lines.len();
        if len > n {
            lines.split_off(len - n)
        } else {
            lines
        }
    }

    /// 从 JSONL 行中提取 assistant 消息文本
    fn extract_text_from_jsonl(lines: &[String]) -> String {
        let mut texts = Vec::new();
        for line in lines {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                // Claude Code JSONL 格式: {"type": "assistant", "message": {...}}
                if let Some(msg_type) = value.get("type").and_then(|t| t.as_str()) {
                    match msg_type {
                        "assistant" => {
                            if let Some(content) = value
                                .pointer("/message/content")
                                .and_then(|c| c.as_array())
                            {
                                for block in content {
                                    if block.get("type").and_then(|t| t.as_str())
                                        == Some("text")
                                    {
                                        if let Some(text) =
                                            block.get("text").and_then(|t| t.as_str())
                                        {
                                            texts.push(text.to_string());
                                        }
                                    }
                                }
                            }
                        }
                        "result" => {
                            if let Some(result) = value.get("result").and_then(|r| r.as_str()) {
                                texts.push(result.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        texts.join("\n")
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn id(&self) -> &str {
        "claude-code"
    }

    fn name(&self) -> &str {
        "Claude Code"
    }

    fn discover_sessions(&self) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // 扫描 claude 相关进程
        let system = System::new_all();
        for (pid, process) in system.processes() {
            let proc_name = process.name().to_string_lossy().to_lowercase();
            let cmd = process
                .cmd()
                .iter()
                .map(|c| c.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(" ");

            let is_claude = proc_name == "claude"
                || (proc_name.contains("node") && cmd.contains("claude"))
                || proc_name.contains("claude-code");

            if !is_claude {
                continue;
            }

            // 排除自身和 grep 类进程
            if cmd.contains("agent-pulse") || cmd.contains("grep") {
                continue;
            }

            let cwd = process
                .cwd()
                .map(|c| c.to_string_lossy().to_string())
                .unwrap_or_default();

            sessions.push(AgentSession {
                id: format!("cc-{}", pid.as_u32()),
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

        // 关联会话文件：将最新的 session 文件分配给会话
        let session_files = self.find_latest_session_file();
        if let (Some(session), Some(file)) = (sessions.first_mut(), session_files.first()) {
            session.session_file = Some(file.to_string_lossy().to_string());
            if let Ok(meta) = fs::metadata(file) {
                if let Ok(modified) = meta.modified() {
                    let datetime: chrono::DateTime<Local> = modified.into();
                    session.last_activity =
                        datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                }
            }
        }

        sessions
    }

    fn session_files(&self) -> Vec<PathBuf> {
        self.find_latest_session_file()
    }

    fn recent_output(&self, session: &AgentSession) -> Option<String> {
        let path = session.session_file.as_ref()?;
        let path_buf = PathBuf::from(path);
        let lines = Self::read_tail_lines(&path_buf, 20);
        if lines.is_empty() {
            return None;
        }
        let text = Self::extract_text_from_jsonl(&lines);
        if text.is_empty() {
            // 如果 JSONL 解析失败，返回原始行
            Some(lines.join("\n"))
        } else {
            Some(text)
        }
    }
}
