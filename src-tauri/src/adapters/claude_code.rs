use super::{
    to_glob_pattern, AgentAdapter, AgentSession, ProcessSnapshot, SessionStatus, TurnState,
};
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
/// 4. 回合结构：看最后一个回合有没有收尾，区分「在干活」和「在等人」
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

    /// 从 Claude CLI 命令行提取可证明的会话 UUID。
    ///
    /// 只有 `--session-id <uuid>` / `--session-id=<uuid>` 与
    /// `--resume <uuid>` / `--resume=<uuid>`（含短参数 `-r`）才携带稳定身份。
    /// `--continue`、裸 `--resume` 和普通交互会话都不能证明当前进程对应哪份
    /// transcript，必须保持未关联，不能按 cwd 猜最新文件。
    fn explicit_session_id(args: &[String]) -> Option<String> {
        let mut ids = Vec::new();
        let mut index = 0;

        while index < args.len() {
            let arg = args[index].as_str();
            if arg == "--" {
                break;
            }
            let inline = arg
                .strip_prefix("--session-id=")
                .or_else(|| arg.strip_prefix("--resume="));
            let candidate = if let Some(value) = inline {
                Some(value)
            } else if matches!(arg, "--session-id" | "--resume" | "-r") {
                args.get(index + 1)
                    .map(String::as_str)
                    .filter(|value| !value.starts_with('-'))
            } else {
                None
            };

            if let Some(value) = candidate {
                if let Ok(id) = uuid::Uuid::parse_str(value) {
                    ids.push(id.hyphenated().to_string());
                }
            }
            index += 1;
        }

        ids.sort();
        ids.dedup();
        (ids.len() == 1).then(|| ids.remove(0))
    }

    /// 将显式会话 UUID 关联到当前 cwd 下唯一的一份 transcript。
    ///
    /// 即便命令行里有 UUID，也必须在 cwd 对应的 Claude project 目录中恰好找到
    /// 一个同名 JSONL 才接受。零个或多个候选都 fail closed，避免路径编码差异或
    /// 异常数据把两个真实会话并成同一个历史键。
    fn transcript_for_explicit_session(
        &self,
        working_dir: &str,
        args: &[String],
    ) -> Option<PathBuf> {
        let session_id = Self::explicit_session_id(args)?;
        if working_dir.is_empty() {
            return None;
        }

        let mut matches = Vec::new();
        for encoded in Self::encode_dir_candidates(working_dir) {
            let project_dir = self.claude_dir.join("projects").join(encoded);
            if !project_dir.is_dir() {
                continue;
            }
            let pattern = to_glob_pattern(&project_dir, &format!("/**/{session_id}.jsonl"));
            for path in glob::glob(&pattern).into_iter().flatten().flatten() {
                if path.is_file() && !matches.contains(&path) {
                    matches.push(path);
                }
            }
        }

        (matches.len() == 1).then(|| matches.remove(0))
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
    ///
    /// 回退读取的窗口开得比较大（512KB）：一次工具调用就能写进去几万字符，
    /// 窗口太小的话真正的最后几行会被挤出去，反而读不到最新状态。
    ///
    /// 注意两个坑，都是踩过的：
    /// 1. 回退位置几乎必然落在某一行中间，**第一行是残行**，必须丢掉，
    ///    否则它会被当成解析失败的 JSON。
    /// 2. 回退位置同样可能落在一个多字节字符中间。这里必须按字节读再
    ///    `from_utf8_lossy`——之前用 `read_to_string` 会直接报错返回空数组，
    ///    结果是**中文记录文件一超过窗口大小就彻底读不出内容**。
    fn read_tail_lines(path: &PathBuf, n: usize) -> Vec<String> {
        const WINDOW: u64 = 512 * 1024;

        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return vec![],
        };

        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return vec![],
        };

        let file_size = metadata.len();
        let seek_pos = file_size.saturating_sub(WINDOW);

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(seek_pos)).is_err() {
            return vec![];
        }

        let mut bytes = Vec::new();
        use std::io::Read;
        if reader.read_to_end(&mut bytes).is_err() {
            return vec![];
        }
        let content = String::from_utf8_lossy(&bytes);

        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        // 不是从文件开头读的 → 首行是被截断的残行，丢掉
        if seek_pos > 0 && !lines.is_empty() {
            lines.remove(0);
        }

        // 只保留最后 n 行
        let len = lines.len();
        if len > n {
            lines.split_off(len - n)
        } else {
            lines
        }
    }

    /// 取出一条记录里的正文，兼容 content 是字符串和数组两种写法
    fn message_text(value: &serde_json::Value) -> Option<String> {
        let content = value.pointer("/message/content")?;
        if let Some(s) = content.as_str() {
            return Some(s.to_string());
        }
        let blocks = content.as_array()?;
        let joined: Vec<String> = blocks
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .map(|s| s.to_string())
            .collect();
        if joined.is_empty() {
            None
        } else {
            Some(joined.join("\n"))
        }
    }

    /// 从 JSONL 行中提取给检测器看的文本
    ///
    /// 只取**真正是 agent 输出**的内容：assistant 的 text 块、result、以及 API 报错行。
    /// 特别不取工具调用的入参和工具结果——那里面装的是命令行、文件内容、搜索结果，
    /// 拿它们去撞关键词等于让检测器读自己写的代码然后报警。
    fn extract_text_from_jsonl(lines: &[String]) -> String {
        let mut texts = Vec::new();
        for line in lines {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            // 真正的报错信号：Claude Code 把 API 错误单独写成带 `isApiErrorMessage`
            // 的行，正文形如 `API Error: 502 Upstream request failed` /
            // `API Error: Unable to connect to API (ECONNRESET)` / `Prompt is too long`。
            // 老实现只看 assistant 的 text 块，这些行一条都读不到——
            // 于是真出错时一声不响，反倒是聊到「rate limit」时乱报警。
            if value.get("isApiErrorMessage").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(text) = Self::message_text(&value) {
                    texts.push(text);
                }
                continue;
            }

            // 同一件事的另一种写法：顶层直接挂 HTTP 状态
            if let Some(status) = value.get("apiErrorStatus") {
                let code = status
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| status.as_u64().map(|n| n.to_string()));
                if let Some(code) = code {
                    texts.push(format!("API Error: {code}"));
                }
            }

            match value.get("type").and_then(|t| t.as_str()) {
                Some("assistant") => {
                    if let Some(text) = Self::message_text(&value) {
                        texts.push(text);
                    }
                }
                Some("result") => {
                    if let Some(result) = value.get("result").and_then(|r| r.as_str()) {
                        texts.push(result.to_string());
                    }
                }
                // 系统行里只有 `level: "error"` 的值得看；info 级别装的是
                // 「Conversation compacted」这类流程事件，不是故障
                Some("system") if value.get("level").and_then(|l| l.as_str()) == Some("error") => {
                    if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
                        texts.push(content.to_string());
                    }
                }
                _ => {}
            }
        }
        texts.join("\n")
    }

    /// 只挑出**运行时自己标成故障**的行
    ///
    /// 与 [`Self::extract_text_from_jsonl`] 的区别就是不要 assistant 的正文。
    /// agent 复述一句「刚才 500 了」和 API 真的返回 500，在散文里没有任何区别，
    /// 但在记录结构里有：故障行带 `isApiErrorMessage` / `apiErrorStatus`，
    /// 或者是 `level: "error"` 的系统行。判「出错 / 限流」只能认这些。
    fn extract_error_text(lines: &[String]) -> String {
        let mut texts = Vec::new();
        for line in lines {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            if value.get("isApiErrorMessage").and_then(|v| v.as_bool()) == Some(true) {
                if let Some(text) = Self::message_text(&value) {
                    texts.push(text);
                }
            }

            if let Some(status) = value.get("apiErrorStatus") {
                let code = status
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| status.as_u64().map(|n| n.to_string()));
                if let Some(code) = code {
                    texts.push(format!("API Error: {code}"));
                }
            }

            if value.get("type").and_then(|t| t.as_str()) == Some("system")
                && value.get("level").and_then(|l| l.as_str()) == Some("error")
            {
                if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
                    texts.push(content.to_string());
                }
            }
        }
        texts.join("\n")
    }

    /// 从记录结构判断回合有没有收尾
    fn classify_turn(lines: &[String]) -> TurnState {
        for line in lines.iter().rev() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(kind) = value.get("type").and_then(|t| t.as_str()) else {
                continue;
            };

            match kind {
                // 簿记类记录：改模式、存快照、写标题，什么时候都可能落盘，
                // 跟回合进行到哪一步无关，跳过继续往前找
                "mode"
                | "permission-mode"
                | "file-history-snapshot"
                | "file-history-delta"
                | "last-prompt"
                | "queue-operation"
                | "ai-title"
                | "summary" => continue,

                "assistant" => {
                    let has_tool_use = value
                        .pointer("/message/content")
                        .and_then(|c| c.as_array())
                        .map(|blocks| {
                            blocks
                                .iter()
                                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        })
                        .unwrap_or(false);
                    // 工具调用已经发出去，结果还没写回来 → 命令正在跑
                    return if has_tool_use {
                        TurnState::ToolRunning
                    } else {
                        TurnState::AwaitingUser
                    };
                }

                // 工具结果刚回来，或真人刚把提示词敲进去：两种都是 agent 正要开工
                "user" | "attachment" => return TurnState::Busy,

                // 压缩边界之类的系统事件，紧接着 agent 会继续跑
                "system" => return TurnState::Busy,

                _ => continue,
            }
        }
        TurnState::Unknown
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

            let Some(process_created_at_ticks) = super::validated_process_creation_ticks(proc)
            else {
                continue;
            };
            let transcript = self.transcript_for_explicit_session(&proc.cwd, &proc.argv);
            let last_activity = transcript
                .as_ref()
                .and_then(|path| fs::metadata(path).ok())
                .and_then(|meta| meta.modified().ok())
                .map(|modified| {
                    let datetime: chrono::DateTime<Local> = modified.into();
                    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
                })
                .unwrap_or_else(|| now.clone());
            sessions.push(AgentSession {
                id: super::process_session_id_with_creation_ticks(
                    "cc",
                    proc,
                    process_created_at_ticks,
                ),
                adapter_id: self.id().to_string(),
                agent_name: self.name().to_string(),
                pid: proc.pid,
                process_started_at: proc.started_at,
                process_created_at_ticks,
                command: proc.cmd.clone(),
                working_dir: proc.cwd.clone(),
                session_file: transcript.map(|path| path.to_string_lossy().to_string()),
                discovered_at: now.clone(),
                last_activity,
                status: SessionStatus::Active,
                resume_count: 0,
                last_resume_at: None,
                ..Default::default()
            });
        }

        sessions
    }

    fn session_files(&self) -> Vec<PathBuf> {
        self.find_latest_session_file()
    }

    /// 读取最近的 agent 输出
    ///
    /// **解析不出正文就返回 `None`，绝不回退到原始 JSONL。**
    /// 老实现在这里 `Some(lines.join("\n"))`，把几万字符的原始 JSON（内含工具入参、
    /// 文件内容、搜索结果）交给关键词匹配——只要 agent 读过一个写着 "rate limit"
    /// 的文件，检测器就会认定线上限流。这次事故就是这么来的：它读到了自己的
    /// 关键词词典，然后判定天下大乱，并往用户正在用的会话里敲了一行字。
    ///
    /// 宁可这一轮没有文本可看（还有进程存活和结构信号兜着），
    /// 也不能拿一坨来源不明的文本去撞关键词。
    fn recent_output(&self, session: &AgentSession) -> Option<String> {
        let path = session.session_file.as_ref()?;
        let lines = Self::read_tail_lines(&PathBuf::from(path), 40);
        if lines.is_empty() {
            return None;
        }
        let text = Self::extract_text_from_jsonl(&lines);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn error_output(&self, session: &AgentSession) -> Option<String> {
        let path = session.session_file.as_ref()?;
        let lines = Self::read_tail_lines(&PathBuf::from(path), 40);
        let text = Self::extract_error_text(&lines);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn turn_state(&self, session: &AgentSession) -> TurnState {
        let Some(path) = session.session_file.as_ref() else {
            return TurnState::Unknown;
        };
        let lines = Self::read_tail_lines(&PathBuf::from(path), 40);
        if lines.is_empty() {
            return TurnState::Unknown;
        }
        Self::classify_turn(&lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    // ── 会话身份 ──

    #[test]
    fn only_explicit_uuid_flags_prove_a_claude_session_identity() {
        let id = "66ae0f75-bbb7-4de6-a26e-26960df14bec";
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude", "--session-id", id])),
            Some(id.to_string())
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude", &format!("--resume={id}"),])),
            Some(id.to_string())
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude", "-r", id])),
            Some(id.to_string())
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude", "--continue"])),
            None
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude", "--resume"])),
            None
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude"])),
            None
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&[
                "claude",
                "--session-id",
                id,
                "--resume",
                "674e2443-86dd-412f-bcb3-a7ec7e8fed78",
            ])),
            None,
            "冲突的显式身份也必须 fail closed"
        );
        assert_eq!(
            ClaudeCodeAdapter::explicit_session_id(&args(&["claude", "--", "--resume", id])),
            None,
            "prompt 参数中的文字不能冒充 CLI 会话身份"
        );
    }

    #[test]
    fn explicit_uuid_must_uniquely_match_the_cwd_project() {
        let root = std::env::temp_dir().join(format!(
            "agent-pulse-claude-identity-{}",
            uuid::Uuid::new_v4()
        ));
        let cwd = "/workspace/shared-project";
        let id = "66ae0f75-bbb7-4de6-a26e-26960df14bec";
        let project = root
            .join("projects")
            .join(ClaudeCodeAdapter::encode_dir_candidates(cwd).remove(0));
        fs::create_dir_all(&project).expect("建 Claude project 目录");
        let transcript = project.join(format!("{id}.jsonl"));
        fs::write(&transcript, "{}\n").expect("写 transcript");

        let adapter = ClaudeCodeAdapter {
            claude_dir: root.clone(),
        };
        assert_eq!(
            adapter.transcript_for_explicit_session(cwd, &args(&["claude", "--resume", id])),
            Some(transcript.clone())
        );
        assert_eq!(
            adapter.transcript_for_explicit_session(cwd, &args(&["claude", "--continue"])),
            None
        );
        assert_eq!(
            adapter
                .transcript_for_explicit_session("/workspace/other", &args(&["claude", "-r", id]),),
            None,
            "不能跨 cwd 仅凭同名文件关联"
        );

        fs::remove_dir_all(root).ok();
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn same_cwd_processes_without_explicit_ids_remain_distinct_runtime_sessions() {
        let root = std::env::temp_dir().join(format!(
            "agent-pulse-claude-parallel-{}",
            uuid::Uuid::new_v4()
        ));
        let cwd = "/workspace/shared-project";
        let project = root
            .join("projects")
            .join(ClaudeCodeAdapter::encode_dir_candidates(cwd).remove(0));
        fs::create_dir_all(&project).expect("建 Claude project 目录");
        for id in [
            "66ae0f75-bbb7-4de6-a26e-26960df14bec",
            "674e2443-86dd-412f-bcb3-a7ec7e8fed78",
        ] {
            fs::write(project.join(format!("{id}.jsonl")), "{}\n").expect("写 transcript");
        }

        let adapter = ClaudeCodeAdapter {
            claude_dir: root.clone(),
        };
        let sessions = adapter.discover_sessions(&[
            ProcessSnapshot {
                pid: 41001,
                started_at: 1001,
                name: "claude".to_string(),
                cmd: "claude".to_string(),
                argv: args(&["claude"]),
                cwd: cwd.to_string(),
            },
            ProcessSnapshot {
                pid: 41002,
                started_at: 1002,
                name: "claude".to_string(),
                cmd: "claude --continue".to_string(),
                argv: args(&["claude", "--continue"]),
                cwd: cwd.to_string(),
            },
        ]);

        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .all(|session| session.session_file.is_none()));
        assert_ne!(sessions[0].history_key(), sessions[1].history_key());
        fs::remove_dir_all(root).ok();
    }

    // ── 回合结构 ──

    #[test]
    fn pending_tool_call_means_still_working() {
        let l = lines(&[
            r#"{"type":"user","message":{"content":"跑一下测试"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}"#,
        ]);
        assert_eq!(
            ClaudeCodeAdapter::classify_turn(&l),
            TurnState::ToolRunning,
            "命令发出去了、结果还没回来"
        );
    }

    #[test]
    fn text_only_reply_means_it_stopped_for_a_human() {
        let l = lines(&[
            r#"{"type":"user","message":{"content":"跑一下测试"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"测试过了"}]}}"#,
        ]);
        assert_eq!(
            ClaudeCodeAdapter::classify_turn(&l),
            TurnState::AwaitingUser
        );
    }
    // TESTS_PLACEHOLDER_ADAPTER

    #[test]
    fn bookkeeping_lines_do_not_change_the_turn() {
        // 快照、标题、模式这些随时会落盘，跟回合进行到哪一步无关
        let l = lines(&[
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"好了"}]}}"#,
            r#"{"type":"file-history-snapshot","id":"a"}"#,
            r#"{"type":"ai-title","title":"修 bug"}"#,
            r#"{"type":"mode","mode":"acceptEdits"}"#,
        ]);
        assert_eq!(
            ClaudeCodeAdapter::classify_turn(&l),
            TurnState::AwaitingUser
        );
    }

    #[test]
    fn tool_result_and_compaction_both_count_as_busy() {
        // 工具结果刚回来 → agent 正要接着干
        let after_tool = lines(&[
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#,
        ]);
        assert_eq!(
            ClaudeCodeAdapter::classify_turn(&after_tool),
            TurnState::Busy
        );

        // 压缩边界：这之后 agent 会继续跑，期间记录长时间不动
        let compacted = lines(&[
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"稍等"}]}}"#,
            r#"{"type":"system","level":"info","subtype":"compact_boundary","content":"Conversation compacted"}"#,
        ]);
        assert_eq!(
            ClaudeCodeAdapter::classify_turn(&compacted),
            TurnState::Busy
        );
    }

    #[test]
    fn unreadable_lines_yield_unknown() {
        assert_eq!(ClaudeCodeAdapter::classify_turn(&[]), TurnState::Unknown);
        let junk = lines(&["not json at all", r#"{"type":"summary"}"#]);
        assert_eq!(ClaudeCodeAdapter::classify_turn(&junk), TurnState::Unknown);
    }
    // TESTS_PLACEHOLDER_ADAPTER

    // ── 给检测器看的文本 ──

    #[test]
    fn api_error_lines_are_picked_up() {
        // 这些行不是 assistant 类型，老实现一条都读不到，于是真出错时一声不响
        let l = lines(&[
            r#"{"isApiErrorMessage":true,"message":{"content":"API Error: 502 Upstream request failed"}}"#,
            r#"{"apiErrorStatus":429}"#,
        ]);
        let text = ClaudeCodeAdapter::extract_text_from_jsonl(&l);
        assert!(text.contains("502"));
        assert!(text.contains("429"));
    }

    #[test]
    fn tool_payloads_never_reach_the_keyword_matcher() {
        // 事故根因：工具入参和结果里装着命令行、文件内容、搜索结果——
        // 包括本项目自己的关键词词典。拿它们去撞关键词等于让检测器读自己的配置。
        let l = lines(&[
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"rate limit\noverloaded\n500"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"connection error"}}]}}"#,
            r#"{"type":"system","level":"info","content":"Conversation compacted"}"#,
        ]);
        assert!(
            ClaudeCodeAdapter::extract_text_from_jsonl(&l).is_empty(),
            "只有 agent 真正说出口的话才算输出"
        );
    }

    #[test]
    fn error_level_system_lines_are_kept() {
        let l = lines(&[r#"{"type":"system","level":"error","content":"ECONNRESET"}"#]);
        assert_eq!(ClaudeCodeAdapter::extract_text_from_jsonl(&l), "ECONNRESET");
    }
    // TESTS_PLACEHOLDER_ADAPTER

    // ── 回退读取 ──

    #[test]
    fn tail_survives_multibyte_at_the_window_edge() {
        // 512KB 的回退点必然落在某个汉字中间。老实现在这里用 read_to_string
        // 会直接报错返回空——结果是中文记录一超过窗口就彻底读不出内容，
        // 检测器于是永远看不到最新状态。
        let dir = std::env::temp_dir().join(format!("agent-pulse-tail-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("transcript.jsonl");

        let mut body = String::new();
        for i in 0..8000 {
            body.push_str(&format!(
                r#"{{"type":"assistant","seq":{i},"message":{{"content":[{{"type":"text","text":"这是一行中文记录，用来把文件撑到回退窗口以外"}}]}}}}"#
            ));
            body.push('\n');
        }
        assert!(body.len() > 512 * 1024, "得比回退窗口大才测得到");
        fs::write(&path, &body).expect("写临时记录");

        let tail = ClaudeCodeAdapter::read_tail_lines(&path, 5);
        fs::remove_dir_all(&dir).ok();

        assert_eq!(tail.len(), 5);
        assert!(tail.last().unwrap().contains("\"seq\":7999"));
        // 每一行都必须是完整 JSON：残行会被当成解析失败，白扔掉一行
        for line in &tail {
            serde_json::from_str::<serde_json::Value>(line).expect("尾部不该有残行");
        }
        assert!(ClaudeCodeAdapter::extract_text_from_jsonl(&tail).contains("中文记录"));
    }

    #[test]
    fn tail_reads_short_files_whole() {
        let dir = std::env::temp_dir().join(format!("agent-pulse-short-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("short.jsonl");
        fs::write(&path, "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n").expect("写临时记录");

        let tail = ClaudeCodeAdapter::read_tail_lines(&path, 40);
        fs::remove_dir_all(&dir).ok();

        // 没回退就不能丢首行
        assert_eq!(tail.len(), 2);
        assert!(tail[0].contains("user"));
    }
}
