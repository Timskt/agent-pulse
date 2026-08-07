use super::{AgentAdapter, AgentSession, ProcessSnapshot, SessionStatus, TurnState};
use chrono::{DateTime, Local};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// OpenAI Codex CLI 适配器。
///
/// Codex 的 cwd 不是会话身份：同一目录可以同时运行多个会话。只有命令行明确包含
/// `codex resume <UUID>`，并且该 UUID 能唯一对应一份带相同 `session_meta` 的 rollout
/// JSONL 时，才把进程和记录关联起来；任何歧义都保持未知。
pub struct CodexAdapter {
    sessions_dir: PathBuf,
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexAdapter {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            sessions_dir: home.join(".codex").join("sessions"),
        }
    }

    #[cfg(test)]
    fn with_sessions_dir(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }

    /// 只接受 Codex 的顶层 `resume <UUID>` 子命令。不能扫描 cwd，也不能在整条命令中
    /// 随便找 `resume`/UUID：`codex exec` 的 prompt 本身完全可能包含这两个 token，误认后
    /// 就会把当前进程关联到另一段真实对话。
    ///
    /// 这里故意采用保守白名单解析全局参数。遇到未知参数或任何更早的 positional/subcommand
    /// 就放弃稳定身份，退回 PID + 启动代际；宁可历史多一条，也不能串会话。
    fn resume_session_id(command: &str) -> Option<String> {
        const VALUE_OPTIONS: &[&str] = &[
            "--config",
            "-c",
            "--model",
            "-m",
            "--profile",
            "-p",
            "--sandbox",
            "-s",
            "--ask-for-approval",
            "-a",
            "--cd",
            "-C",
            "--add-dir",
            "--oss-provider",
            "--enable",
            "--disable",
        ];
        const FLAG_OPTIONS: &[&str] = &[
            "--search",
            "--oss",
            "--full-auto",
            "--dangerously-bypass-approvals-and-sandbox",
            "--no-alt-screen",
        ];

        let parts: Vec<&str> = command.split_whitespace().map(Self::unquote).collect();
        let executable = parts.iter().position(|part| {
            Path::new(part)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, "codex" | "codex.exe"))
        })?;
        let mut index = executable + 1;
        while index < parts.len() {
            let part = parts[index];
            if part == "resume" {
                let candidate = *parts.get(index + 1)?;
                return Uuid::parse_str(candidate)
                    .ok()
                    .map(|id| id.hyphenated().to_string());
            }
            if part == "--" {
                return None;
            }
            if part.starts_with("--") && part.contains('=') {
                index += 1;
                continue;
            }
            if VALUE_OPTIONS.contains(&part) {
                // 缺值时也必须拒绝；不能把后面的 `resume` 误当成全局参数值后再继续猜。
                parts.get(index + 1)?;
                index += 2;
                continue;
            }
            if FLAG_OPTIONS.contains(&part) {
                index += 1;
                continue;
            }

            // 任何其他 token 都可能是 `exec`/`review` 等子命令或它的 prompt。
            return None;
        }
        None
    }

    fn unquote(value: &str) -> &str {
        value.trim_matches(|c| matches!(c, '\'' | '"'))
    }

    /// 按 UUID 找出唯一 transcript。
    ///
    /// 文件名匹配之外还核对 `session_meta.payload.id/session_id`，防止同名复制品、损坏
    /// 文件或未来目录布局变化造成误关联。多个候选同样拒绝猜测。
    fn find_session_file(&self, session_id: &str) -> Option<PathBuf> {
        let expected = Uuid::parse_str(session_id).ok()?.hyphenated().to_string();
        let suffix = format!("{expected}.jsonl");

        let mut matches = Self::rollout_files(&self.sessions_dir)
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
            })
            .filter(|path| Self::transcript_session_id(path).as_deref() == Some(expected.as_str()));

        let found = matches.next()?;
        matches.next().is_none().then_some(found)
    }

    fn rollout_files(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];

        while let Some(dir) = pending.pop() {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if file_type.is_dir() {
                    pending.push(path);
                } else if file_type.is_file()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("rollout-") && name.ends_with(".jsonl")
                        })
                {
                    files.push(path);
                }
            }
        }
        files
    }

    fn transcript_session_id(path: &Path) -> Option<String> {
        let reader = BufReader::new(fs::File::open(path).ok()?);
        for line in reader.lines().take(32).flatten() {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) != Some("session_meta") {
                continue;
            }
            let raw = value
                .pointer("/payload/id")
                .or_else(|| value.pointer("/payload/session_id"))
                .and_then(Value::as_str)?;
            return Uuid::parse_str(raw)
                .ok()
                .map(|id| id.hyphenated().to_string());
        }
        None
    }

    /// 读取 JSONL 尾部。大工具结果可能很长，因此保留 512 KiB 回退窗口；从文件
    /// 中部起读时丢掉首个残行，并用 lossy UTF-8 避免切在多字节字符中间后整段失效。
    fn read_tail_lines(path: &Path, count: usize) -> Vec<String> {
        const WINDOW: u64 = 512 * 1024;

        let Ok(mut file) = fs::File::open(path) else {
            return Vec::new();
        };
        let Ok(size) = file.metadata().map(|metadata| metadata.len()) else {
            return Vec::new();
        };
        let seek_pos = size.saturating_sub(WINDOW);
        if file.seek(SeekFrom::Start(seek_pos)).is_err() {
            return Vec::new();
        }

        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        let content = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
        if seek_pos > 0 && !lines.is_empty() {
            lines.remove(0);
        }

        let keep_from = lines.len().saturating_sub(count);
        lines.split_off(keep_from)
    }

    fn content_text(content: &Value) -> Option<String> {
        if let Some(text) = content.as_str() {
            return Some(text.to_owned());
        }
        let texts: Vec<&str> = content
            .as_array()?
            .iter()
            .filter(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("output_text" | "text")
                )
            })
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect();
        (!texts.is_empty()).then(|| texts.join("\n"))
    }

    /// 只把 Agent 真正输出给用户的正文交给关键词检测器。developer/user 消息、工具
    /// 入参和工具输出都不能混进来，否则读取过一段“rate limit”文字就会制造假告警。
    fn extract_recent_output(lines: &[String]) -> String {
        let mut texts = Vec::new();
        for line in lines {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let text = match (
                value.get("type").and_then(Value::as_str),
                value.pointer("/payload/type").and_then(Value::as_str),
            ) {
                (Some("event_msg"), Some("agent_message")) => value
                    .pointer("/payload/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                (Some("response_item"), Some("message"))
                    if value.pointer("/payload/role").and_then(Value::as_str)
                        == Some("assistant") =>
                {
                    value
                        .pointer("/payload/content")
                        .and_then(Self::content_text)
                }
                _ => None,
            };
            if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
                // Codex 通常会把同一条回答同时写成 response_item/message 和
                // event_msg/agent_message；相邻去重，避免历史面板与关键词权重翻倍。
                if texts.last() != Some(&text) {
                    texts.push(text);
                }
            }
        }
        texts.join("\n")
    }

    fn error_message(value: &Value) -> Option<String> {
        if let Some(message) = value.as_str() {
            return Some(message.to_owned());
        }
        value
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| value.get("error").and_then(Self::error_message))
    }

    /// 只读 Codex 明确标成错误的结构化事件，不把 assistant 散文或工具输出当故障。
    fn extract_error_output(lines: &[String]) -> String {
        let mut errors = Vec::new();
        for line in lines {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let top = value.get("type").and_then(Value::as_str);
            let event = value.pointer("/payload/type").and_then(Value::as_str);
            let error = match (top, event) {
                (Some("event_msg"), Some("task_complete")) => value
                    .pointer("/payload/error")
                    .filter(|error| !error.is_null())
                    .and_then(Self::error_message),
                (Some("event_msg"), Some("error" | "turn_error" | "stream_error"))
                | (Some("response_item"), Some("error")) => {
                    value.pointer("/payload").and_then(Self::error_message)
                }
                (Some("error"), _) => Self::error_message(&value),
                _ => None,
            };
            if let Some(error) = error.filter(|error| !error.trim().is_empty()) {
                if errors.last() != Some(&error) {
                    errors.push(error);
                }
            }
        }
        errors.join("\n")
    }

    fn classify_turn(lines: &[String]) -> TurnState {
        for line in lines.iter().rev() {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let top = value.get("type").and_then(Value::as_str);
            let event = value.pointer("/payload/type").and_then(Value::as_str);

            match (top, event) {
                (Some("event_msg"), Some("task_complete" | "turn_aborted")) => {
                    return TurnState::AwaitingUser;
                }
                (Some("event_msg"), Some("mcp_tool_call_begin")) => {
                    return TurnState::ToolRunning;
                }
                (Some("event_msg"), Some("task_started" | "user_message")) => {
                    return TurnState::Busy;
                }
                (Some("event_msg"), Some("agent_message" | "agent_reasoning"))
                | (Some("event_msg"), Some("mcp_tool_call_end" | "patch_apply_end"))
                | (Some("event_msg"), Some("context_compacted")) => {
                    return TurnState::Busy;
                }
                (Some("response_item"), Some("function_call")) => {
                    return TurnState::ToolRunning;
                }
                (Some("response_item"), Some("custom_tool_call")) => {
                    return if value.pointer("/payload/status").and_then(Value::as_str)
                        == Some("completed")
                    {
                        TurnState::Busy
                    } else {
                        TurnState::ToolRunning
                    };
                }
                (
                    Some("response_item"),
                    Some("function_call_output" | "custom_tool_call_output"),
                )
                | (Some("response_item"), Some("reasoning" | "message"))
                | (Some("compacted"), _) => return TurnState::Busy,
                // token_count、thread_settings、turn_context、session_meta 等都是簿记事件，
                // 不能覆盖更早但真正描述回合边界的记录。
                _ => continue,
            }
        }
        TurnState::Unknown
    }

    fn latest_activity(path: &Path, lines: &[String]) -> Option<String> {
        let event_time = lines.iter().rev().find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            let timestamp = value.get("timestamp").and_then(Value::as_str)?;
            DateTime::parse_from_rfc3339(timestamp).ok()
        });

        let local = event_time
            .map(|time| time.with_timezone(&Local))
            .or_else(|| {
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .map(DateTime::<Local>::from)
            })?;
        Some(local.format("%Y-%m-%d %H:%M:%S").to_string())
    }
}

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

            let Some(process_created_at_ticks) = super::validated_process_creation_ticks(proc)
            else {
                continue;
            };
            let external_session_id = Self::resume_session_id(&proc.cmd);
            let session_file = external_session_id
                .as_deref()
                .and_then(|session_id| self.find_session_file(session_id));
            let logical_id = external_session_id
                .as_deref()
                .filter(|_| session_file.is_some())
                .map(|session_id| format!("cx-{session_id}"))
                .unwrap_or_else(|| {
                    super::process_session_id_with_creation_ticks(
                        "cx",
                        proc,
                        process_created_at_ticks,
                    )
                });
            let last_activity = session_file
                .as_deref()
                .and_then(|path| {
                    let lines = Self::read_tail_lines(path, 200);
                    Self::latest_activity(path, &lines)
                })
                .unwrap_or_else(|| now.clone());

            sessions.push(AgentSession {
                id: logical_id,
                adapter_id: self.id().to_string(),
                agent_name: self.name().to_string(),
                pid: proc.pid,
                process_started_at: proc.started_at,
                process_created_at_ticks,
                command: proc.cmd.clone(),
                working_dir: proc.cwd.clone(),
                session_file: session_file.map(|path| path.to_string_lossy().into_owned()),
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
        let mut files = Self::rollout_files(&self.sessions_dir);
        files.sort_by(|a, b| {
            let a_time = fs::metadata(a)
                .and_then(|metadata| metadata.modified())
                .ok();
            let b_time = fs::metadata(b)
                .and_then(|metadata| metadata.modified())
                .ok();
            b_time.cmp(&a_time)
        });
        files.into_iter().take(50).collect()
    }

    fn recent_output(&self, session: &AgentSession) -> Option<String> {
        let path = session.session_file.as_deref().map(Path::new)?;
        let text = Self::extract_recent_output(&Self::read_tail_lines(path, 200));
        (!text.is_empty()).then_some(text)
    }

    fn error_output(&self, session: &AgentSession) -> Option<String> {
        let path = session.session_file.as_deref().map(Path::new)?;
        let text = Self::extract_error_output(&Self::read_tail_lines(path, 200));
        (!text.is_empty()).then_some(text)
    }

    fn turn_state(&self, session: &AgentSession) -> TurnState {
        let Some(path) = session.session_file.as_deref().map(Path::new) else {
            return TurnState::Unknown;
        };
        Self::classify_turn(&Self::read_tail_lines(path, 200))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_ID: &str = "019fcdeb-55e6-7bd3-80f4-9570ce6c555e";

    fn temp_sessions_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent-pulse-codex-{label}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).expect("创建 fixture 目录");
        dir
    }

    fn write_transcript(root: &Path, id: &str, body: &[&str]) -> PathBuf {
        let day = root.join("2026").join("08").join("07");
        fs::create_dir_all(&day).expect("创建日期目录");
        let path = day.join(format!("rollout-2026-08-07T10-00-00-{id}.jsonl"));
        let mut lines = vec![format!(
            r#"{{"timestamp":"2026-08-07T10:00:00.000Z","type":"session_meta","payload":{{"id":"{id}","cwd":"/fixture"}}}}"#
        )];
        lines.extend(body.iter().map(|line| (*line).to_owned()));
        fs::write(&path, format!("{}\n", lines.join("\n"))).expect("写 fixture transcript");
        path
    }

    fn process(command: &str) -> ProcessSnapshot {
        ProcessSnapshot {
            pid: 42,
            started_at: 123,
            name: "codex".to_owned(),
            cmd: command.to_owned(),
            argv: command.split_whitespace().map(str::to_owned).collect(),
            cwd: "/same/project".to_owned(),
        }
    }

    #[test]
    fn extracts_only_uuid_immediately_after_resume() {
        assert_eq!(
            CodexAdapter::resume_session_id(&format!("codex --model o3 resume {SESSION_ID}")),
            Some(SESSION_ID.to_owned())
        );
        assert_eq!(
            CodexAdapter::resume_session_id(&format!("codex resume \"{SESSION_ID}\"")),
            Some(SESSION_ID.to_owned())
        );
        assert_eq!(
            CodexAdapter::resume_session_id(&format!("codex exec prompt-{SESSION_ID}")),
            None
        );
        assert_eq!(
            CodexAdapter::resume_session_id(&format!("codex exec resume {SESSION_ID}")),
            None,
            "exec prompt 里的 resume + UUID 不能被误认成顶层 resume 子命令"
        );
        assert_eq!(
            CodexAdapter::resume_session_id(&format!("codex review resume {SESSION_ID}")),
            None,
            "其他子命令后的 token 同样不能建立逻辑会话身份"
        );
        assert_eq!(
            CodexAdapter::resume_session_id(&format!("codex --unknown value resume {SESSION_ID}")),
            None,
            "未知全局参数宁可退回运行代际，也不能猜它是否带值"
        );
        assert_eq!(CodexAdapter::resume_session_id("codex resume --last"), None);
    }

    #[test]
    fn discovery_links_exact_resume_uuid_and_reads_event_activity() {
        let root = temp_sessions_dir("discover");
        let expected = write_transcript(
            &root,
            SESSION_ID,
            &[
                r#"{"timestamp":"2026-08-07T10:03:04.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-1"}}"#,
            ],
        );
        let other_id = "019fda04-da1c-7500-ac61-585ae8a0d12c";
        write_transcript(&root, other_id, &[]);
        let adapter = CodexAdapter::with_sessions_dir(root.clone());

        let sessions = adapter.discover_sessions(&[process(&format!("codex resume {SESSION_ID}"))]);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, format!("cx-{SESSION_ID}"));
        assert_eq!(
            sessions[0].session_file.as_deref(),
            Some(expected.to_string_lossy().as_ref())
        );
        let expected_activity = DateTime::parse_from_rfc3339("2026-08-07T10:03:04.000Z")
            .unwrap()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(sessions[0].last_activity, expected_activity);

        let cwd_only = adapter.discover_sessions(&[process("codex")]);
        assert_eq!(cwd_only.len(), 1);
        assert!(
            cwd_only[0].session_file.is_none(),
            "即使 cwd 相同且目录里有 transcript，也不能猜会话身份"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ambiguous_or_metadata_mismatched_transcripts_remain_unknown() {
        let root = temp_sessions_dir("ambiguous");
        let first = write_transcript(&root, SESSION_ID, &[]);
        let duplicate_dir = root.join("copy");
        fs::create_dir_all(&duplicate_dir).unwrap();
        fs::copy(&first, duplicate_dir.join(first.file_name().unwrap())).unwrap();
        let adapter = CodexAdapter::with_sessions_dir(root.clone());
        assert!(adapter.find_session_file(SESSION_ID).is_none());

        fs::remove_dir_all(&root).unwrap();
        fs::create_dir_all(&root).unwrap();
        let wrong_id = "019fda04-da1c-7500-ac61-585ae8a0d12c";
        let path = write_transcript(&root, wrong_id, &[]);
        let renamed = path.with_file_name(format!("rollout-copy-{SESSION_ID}.jsonl"));
        fs::rename(path, renamed).unwrap();
        assert!(adapter.find_session_file(SESSION_ID).is_none());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn recent_output_keeps_only_agent_text_and_deduplicates_mirrors() {
        let lines = vec![
            r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"rate limit 429"}]}}"#.to_owned(),
            r#"{"type":"response_item","payload":{"type":"function_call_output","output":"500 failed"}}"#.to_owned(),
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"已完成修复"}]}}"#.to_owned(),
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"已完成修复"}}"#.to_owned(),
        ];
        assert_eq!(CodexAdapter::extract_recent_output(&lines), "已完成修复");
    }

    #[test]
    fn structured_errors_do_not_include_prose_or_tool_failures() {
        let lines = vec![
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"讨论 429 和 500"}}"#.to_owned(),
            r#"{"type":"response_item","payload":{"type":"function_call_output","output":"Process exited with code 1"}}"#.to_owned(),
            r#"{"type":"event_msg","payload":{"type":"task_complete","error":{"message":"unexpected status 503 Service Unavailable","codex_error_info":"other"}}}"#.to_owned(),
        ];
        assert_eq!(
            CodexAdapter::extract_error_output(&lines),
            "unexpected status 503 Service Unavailable"
        );
    }

    #[test]
    fn turn_state_uses_structural_boundaries() {
        let started = vec![
            r#"{"type":"event_msg","payload":{"type":"task_started"}}"#.to_owned(),
            r#"{"type":"event_msg","payload":{"type":"token_count"}}"#.to_owned(),
        ];
        assert_eq!(CodexAdapter::classify_turn(&started), TurnState::Busy);

        let tool = vec![
            r#"{"type":"event_msg","payload":{"type":"task_started"}}"#.to_owned(),
            r#"{"type":"response_item","payload":{"type":"function_call","call_id":"call-1"}}"#
                .to_owned(),
        ];
        assert_eq!(CodexAdapter::classify_turn(&tool), TurnState::ToolRunning);

        let completed = vec![
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[]}}"#.to_owned(),
            r#"{"type":"event_msg","payload":{"type":"task_complete"}}"#.to_owned(),
            r#"{"type":"event_msg","payload":{"type":"token_count"}}"#.to_owned(),
        ];
        assert_eq!(
            CodexAdapter::classify_turn(&completed),
            TurnState::AwaitingUser
        );
        assert_eq!(
            CodexAdapter::classify_turn(&["not json".to_owned()]),
            TurnState::Unknown
        );
    }

    #[test]
    fn adapter_methods_read_fixture_without_real_home() {
        let root = temp_sessions_dir("methods");
        let path = write_transcript(
            &root,
            SESSION_ID,
            &[
                r#"{"timestamp":"2026-08-07T10:00:01Z","type":"event_msg","payload":{"type":"agent_message","message":"准备重试"}}"#,
                r#"{"timestamp":"2026-08-07T10:00:02Z","type":"event_msg","payload":{"type":"task_complete","error":{"message":"rate limit 429"}}}"#,
            ],
        );
        let adapter = CodexAdapter::with_sessions_dir(root.clone());
        let session = AgentSession {
            session_file: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        assert_eq!(adapter.recent_output(&session).as_deref(), Some("准备重试"));
        assert_eq!(
            adapter.error_output(&session).as_deref(),
            Some("rate limit 429")
        );
        assert_eq!(adapter.turn_state(&session), TurnState::AwaitingUser);
        fs::remove_dir_all(root).ok();
    }
}
