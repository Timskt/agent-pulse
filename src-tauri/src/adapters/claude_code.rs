use super::{to_glob_pattern, AgentAdapter, AgentSession, ProcessSnapshot, SessionStatus};
use chrono::Local;
use std::fs;
use std::io::{BufReader, Seek, SeekFrom};
use std::path::PathBuf;

/// Claude Code (claude CLI) 适配器
///
/// 检测策略：
/// 1. 进程扫描：查找 `claude` 进程
/// 2. 会话文件：~/.claude/projects/*/sessions/*.jsonl
/// 3. 输出分析：读取会话 JSONL 最后几行判断状态
pub struct ClaudeCodeAdapter {
    claude_dir: PathBuf,
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            claude_dir: home.join(".claude"),
        }
    }

    /// 生成工作目录的所有可能编码形式
    ///
    /// Claude Code 将 cwd 编码为目录名（分隔符替换为 -），
    /// 但各平台/版本对盘符冒号的处理不一致，因此穷举常见变体：
    /// - macOS/Linux: /Users/sky/code → -Users-sky-code
    /// - Windows: C:\Users\sky → C:-Users-sky / C--Users-sky / C-Users-sky
    fn encode_dir_candidates(working_dir: &str) -> Vec<String> {
        let mut candidates = Vec::new();

        // 变体 1: 仅替换路径分隔符，保留冒号
        let v1 = working_dir.replace(['\\', '/'], "-");
        // 变体 2: 冒号也替换为 -
        let v2 = v1.replace(':', "-");
        // 变体 3: 移除冒号
        let v3 = v1.replace(':', "");

        for v in [&v1, &v2, &v3] {
            // 原始形式 + 去除前导 - 的形式
            if !candidates.contains(v) {
                candidates.push(v.clone());
            }
            let trimmed = v.trim_start_matches('-').to_string();
            if !trimmed.is_empty() && !candidates.contains(&trimmed) {
                candidates.push(trimmed);
            }
        }

        candidates
    }

    /// 查找最新的会话 JSONL 文件
    fn find_latest_session_file(&self) -> Vec<PathBuf> {
        let base = self.claude_dir.join("projects");
        let pattern = to_glob_pattern(&base, "/**/*.jsonl");

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

    fn discover_sessions(&self, processes: &[ProcessSnapshot]) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        // 从进程快照中查找 claude 相关进程
        for proc in processes {
            let is_claude = proc.name == "claude"
                || proc.name == "claude.exe"
                || proc.name.starts_with("claude-code")
                || (proc.name.contains("node") && proc.cmd.contains("claude"));

            if !is_claude {
                continue;
            }

            // 排除自身和 grep 类进程
            if proc.cmd.contains("agent-pulse") || proc.cmd.contains("grep") {
                continue;
            }

            sessions.push(AgentSession {
                id: format!("cc-{}", proc.pid),
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

        // 多实例关联：按工作目录匹配对应的会话文件
        // Claude Code 将会话存储在 ~/.claude/projects/<encoded-cwd>/ 下
        for session in &mut sessions {
            if session.working_dir.is_empty() {
                continue;
            }

            // 生成所有可能的编码目录名（兼容各平台编码差异）
            let encoded_candidates = Self::encode_dir_candidates(&session.working_dir);

            for encoded in &encoded_candidates {
                let project_dir = self.claude_dir.join("projects").join(encoded);
                if !project_dir.exists() {
                    continue;
                }
                // 找到该项目目录下最新的 .jsonl 文件
                let pattern = to_glob_pattern(&project_dir, "/**/*.jsonl");
                let mut files: Vec<PathBuf> = glob::glob(&pattern)
                    .map(|paths| paths.filter_map(|p| p.ok()).collect())
                    .unwrap_or_default();

                files.sort_by(|a, b| {
                    let ta = fs::metadata(a).and_then(|m| m.modified()).ok();
                    let tb = fs::metadata(b).and_then(|m| m.modified()).ok();
                    tb.cmp(&ta)
                });

                if let Some(latest) = files.first() {
                    session.session_file = Some(latest.to_string_lossy().to_string());
                    if let Ok(meta) = fs::metadata(latest) {
                        if let Ok(modified) = meta.modified() {
                            let datetime: chrono::DateTime<Local> = modified.into();
                            session.last_activity =
                                datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                        }
                    }
                }
                break; // 找到匹配的目录就停止
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
